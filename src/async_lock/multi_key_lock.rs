use std::{
    sync::{Arc, atomic::Ordering},
    time::SystemTime,
};

use tokio::{
    sync::{mpsc::error::TrySendError, oneshot},
    time,
};
use uuid::Uuid;

use super::{RLock, operations::*};
use crate::{
    AcquireError, AcquireOptions, ReleaseError,
    release_request::{ReleaseRequest, ReleaseRequestKey},
};

/// A guard object representing an acquired Redis lock (multi-key).
///
/// # Behavior
///
/// * Dropping the guard automatically releases the lock unless the buffer of the lock release manager is full (in that case, the lock is released after its TTL expires in Redis)
/// * If the renewal interval is not set to `None`, a background task (aka the renewal task) is spawned to extend the lock's TTL. The task is terminated when the lock is released.
#[derive(Debug)]
pub struct RMultiKeyLockGuard {
    rlock:       RLock,
    is_released: bool,
    keys:        Arc<Vec<String>>,
    uuid:        Arc<String>,
    stop_tx:     Option<oneshot::Sender<()>>,
}

impl RMultiKeyLockGuard {
    /// Releases the lock. If the lock is released, returns `Ok(())`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use std::sync::Arc;
    ///
    /// use rlock::async_lock::RLock;
    ///
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn main() {
    /// let rlock = RLock::new("redis://127.0.0.1:6379/0").await.unwrap();
    ///
    /// let mut lock = rlock
    ///     .acquire_multi_key_mutex(Arc::new(vec![
    ///         String::from("rlock:example"),
    ///         String::from("rlock:example2"),
    ///     ]))
    ///     .await
    ///     .unwrap();
    ///
    /// // critial section
    ///
    /// lock.release().await.unwrap();
    /// # }
    /// ```
    #[inline]
    pub async fn release(&mut self) -> Result<(), ReleaseError> {
        if self.is_released {
            return Ok(());
        }

        release_multi_key_lock_async(
            &mut self.rlock.inner.connection_manager.clone(),
            &self.keys,
            self.uuid.as_str(),
        )
        .await?;

        self.is_released = true;

        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
        }

        Ok(())
    }

    /// Determines whether the lock is released.
    #[inline]
    pub const fn is_released(&self) -> bool {
        self.is_released
    }
}

impl Drop for RMultiKeyLockGuard {
    #[inline]
    fn drop(&mut self) {
        if self.is_released {
            return;
        }

        if let Err(error) = self.rlock.request_tx.try_send(Some(ReleaseRequest {
            key:  ReleaseRequestKey::Multiple(self.keys.clone()),
            uuid: self.uuid.clone(),
        })) {
            if matches!(error, TrySendError::Full(_)) {
                tracing::warn!(
                    uuid = %self.uuid,
                    "the lock release manager is full, the lock will be released after its TTL \
                     expires",
                );
            } else {
                tracing::warn!(
                    uuid = %self.uuid,
                    "cannot send a lock release request: {error}",
                );
            }
        }
    }
}

impl RLock {
    /// Acquires a mutex lock with mutiple keys.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use std::sync::Arc;
    ///
    /// use rlock::async_lock::RLock;
    ///
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn main() {
    /// let rlock = RLock::new("redis://127.0.0.1:6379/0").await.unwrap();
    ///
    /// let lock = rlock
    ///     .acquire_multi_key_mutex(Arc::new(vec![
    ///         String::from("rlock:example"),
    ///         String::from("rlock:example2"),
    ///     ]))
    ///     .await
    ///     .unwrap();
    ///
    /// // critial section
    ///
    /// // drop(lock);
    /// # }
    /// ```
    #[inline]
    pub async fn acquire_multi_key_mutex(
        &self,
        keys: impl Into<Arc<Vec<String>>>,
    ) -> Result<RMultiKeyLockGuard, AcquireError> {
        self.acquire_multi_key_mutex_with_options(keys, AcquireOptions::default()).await
    }

    /// Acquires a mutex lock with mutiple keys and options.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use std::{sync::Arc, time::Duration};
    ///
    /// use rlock::{AcquireOptions, async_lock::RLock};
    ///
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn main() {
    /// let rlock = RLock::new("redis://127.0.0.1:6379/0").await.unwrap();
    ///
    /// let lock = rlock
    ///     .acquire_multi_key_mutex_with_options(
    ///         Arc::new(vec![
    ///             String::from("rlock:example"),
    ///             String::from("rlock:example2"),
    ///         ]),
    ///         AcquireOptions::new().ttl(Duration::from_secs(3)),
    ///     )
    ///     .await
    ///     .unwrap();
    ///
    /// // critial section
    ///
    /// // drop(lock);
    /// # }
    /// ```
    pub async fn acquire_multi_key_mutex_with_options(
        &self,
        keys: impl Into<Arc<Vec<String>>>,
        options: AcquireOptions,
    ) -> Result<RMultiKeyLockGuard, AcquireError> {
        if self.inner.is_shutdown.load(Ordering::Relaxed) {
            return Err(AcquireError::Shutdown);
        }

        let keys = keys.into();

        let uuid = Arc::new(Uuid::new_v4().to_string());

        let start_time = SystemTime::now();

        loop {
            let result = acquire_multi_key_lock_async(
                &mut self.inner.connection_manager.clone(),
                &keys,
                uuid.as_ref(),
                options.ttl,
            )
            .await?;

            if result {
                break;
            }

            // an lock with the same key exists, so we cannot acquire

            // check lock_timeout
            if let Some(lock_timeout) = options.lock_timeout {
                if SystemTime::now().duration_since(start_time).unwrap() >= lock_timeout {
                    return Err(AcquireError::LockTimeout);
                }
            }

            // sleep retry_interval to allow other tasks to run
            time::sleep(options.retry_interval).await;
        }

        // a new lock has been created

        let stop_tx = if let Some(renew_interval) = options.renew_interval {
            // renew automatically
            let (stop_tx, mut stop_rx) = oneshot::channel::<()>();

            let keys = keys.clone();
            let uuid = uuid.clone();
            let mut connection_manager = self.inner.connection_manager.clone();

            tokio::spawn(async move {
                tracing::trace!(uuid = %uuid, "spawned a task in order to automatically renew the lock");

                loop {
                    tokio::select! {
                        _ = &mut stop_rx => {
                            tracing::trace!(uuid = %uuid, "a spawned renewal task for the lock is stopped because the lock is being released");

                            break
                        },
                        _ = time::sleep(renew_interval.unwrap_or_else(|| options.ttl / 2)) => {
                            match renew_multi_key_lock_async(&mut connection_manager, &keys, uuid.as_str(), options.ttl).await {
                                Ok(false) => {
                                    tracing::trace!(uuid = %uuid, "cannot find the lock, close the renewal task");
                                    break;
                                }
                                Ok(true) => {
                                    tracing::trace!(uuid = %uuid, "renewed the lock");

                                    continue;
                                }
                                Err(error) => {
                                    tracing::error!(uuid = %uuid, "an error occured when renewing the lock: {error}");
                                    tracing::trace!(uuid = %uuid, "close the renewal task for the lock");
                                    break;
                                }
                            }
                        },
                    }
                }
            });

            Some(stop_tx)
        } else {
            None
        };

        Ok(RMultiKeyLockGuard {
            rlock: self.clone(),
            is_released: false,
            keys,
            uuid,
            stop_tx,
        })
    }
}
