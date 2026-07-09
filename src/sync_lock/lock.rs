use std::{
    sync::{
        Arc,
        atomic::Ordering,
        mpsc::{self, RecvTimeoutError, Sender, TrySendError},
    },
    thread,
    time::Instant,
};

use uuid::Uuid;

use super::{RLock, operations::*};
use crate::{
    AcquireError, AcquireOptions, ReleaseError,
    release_request::{ReleaseRequest, ReleaseRequestKey},
};

/// A guard object representing an acquired Redis lock.
///
/// # Behavior
///
/// * Dropping the guard automatically releases the lock unless the buffer of the lock release manager is full (in that case, the lock is released after its TTL expires in Redis)
/// * If the renewal interval is not set to `None`, a background thread (aka the renewal thread) is spawned to extend the lock's TTL. The thread is terminated when the lock is released.
#[derive(Debug)]
pub struct RLockGuard {
    rlock:       RLock,
    is_released: bool,
    key:         Arc<String>,
    uuid:        Arc<String>,
    stop_tx:     Option<Sender<()>>,
}

impl RLockGuard {
    /// Releases the lock. If the lock is released, returns `Ok(())`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rlock::sync_lock::RLock;
    ///
    /// let rlock = RLock::new("redis://127.0.0.1:6379/0").unwrap();
    ///
    /// let mut lock = rlock.acquire_mutex("rlock:example").unwrap();
    ///
    /// // critial section
    ///
    /// lock.release().unwrap();
    /// ```
    #[inline]
    pub fn release(&mut self) -> Result<(), ReleaseError> {
        if self.is_released {
            return Ok(());
        }

        {
            let mut conn = self.rlock.inner.pool.checkout()?;

            release_lock_sync(&mut *conn, self.key.as_str(), self.uuid.as_str())?;
        }

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

impl Drop for RLockGuard {
    #[inline]
    fn drop(&mut self) {
        if self.is_released {
            return;
        }

        if let Err(error) = self.rlock.request_tx.try_send(Some(ReleaseRequest {
            key:  ReleaseRequestKey::Single(self.key.clone()),
            uuid: self.uuid.clone(),
        })) {
            if matches!(error, TrySendError::Full(_)) {
                tracing::warn!(
                    uuid = %self.uuid,
                    "the lock release manager is full, the lock will be released after its \
                     TTL expires"
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
    /// Acquires a mutex lock with a key.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rlock::sync_lock::RLock;
    ///
    /// let rlock = RLock::new("redis://127.0.0.1:6379/0").unwrap();
    ///
    /// let lock = rlock.acquire_mutex("rlock:example").unwrap();
    ///
    /// // critial section
    ///
    /// // drop(lock);
    /// ```
    #[inline]
    pub fn acquire_mutex(&self, key: impl AsRef<str>) -> Result<RLockGuard, AcquireError> {
        self.acquire_mutex_with_options(key, AcquireOptions::default())
    }

    /// Acquires a mutex lock with a key and options.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use std::time::Duration;
    ///
    /// use rlock::{AcquireOptions, sync_lock::RLock};
    ///
    /// let rlock = RLock::new("redis://127.0.0.1:6379/0").unwrap();
    ///
    /// let lock = rlock
    ///     .acquire_mutex_with_options(
    ///         "rlock:example",
    ///         AcquireOptions::builder()
    ///             .ttl(Duration::from_secs(3))
    ///             .build()
    ///             .unwrap(),
    ///     )
    ///     .unwrap();
    ///
    /// // critial section
    ///
    /// // drop(lock);
    /// ```
    pub fn acquire_mutex_with_options(
        &self,
        key: impl AsRef<str>,
        options: AcquireOptions,
    ) -> Result<RLockGuard, AcquireError> {
        if self.inner.is_shutdown.load(Ordering::Relaxed) {
            return Err(AcquireError::Shutdown);
        }

        let key: &str = key.as_ref();
        let retry_interval = options.retry_interval();
        let lock_timeout = options.lock_timeout();
        let ttl = options.ttl();
        let renew_interval = options.renew_interval();

        let uuid = Arc::new(Uuid::new_v4().to_string());

        let start_time = Instant::now();

        loop {
            // the checked-out connection returns to the pool at the end of this block, so it is not held while sleeping
            let result = {
                let mut conn = self.inner.pool.checkout()?;

                acquire_lock_sync(&mut *conn, key, uuid.as_ref(), ttl)?
            };

            if result.is_some() {
                break;
            }

            // an lock with the same key exists, so we cannot acquire

            // check lock_timeout
            if let Some(lock_timeout) = lock_timeout
                && start_time.elapsed() >= lock_timeout
            {
                return Err(AcquireError::LockTimeout);
            }

            // sleep retry_interval to allow other threads to run
            thread::sleep(retry_interval);
        }

        let key = Arc::new(String::from(key));

        // a new lock has been created

        let stop_tx = if let Some(renew_interval) = renew_interval {
            // renew automatically
            let (stop_tx, stop_rx) = mpsc::channel::<()>();

            let key = key.clone();
            let uuid = uuid.clone();
            let pool = self.inner.pool.clone();

            thread::spawn(move || {
                tracing::trace!(uuid = %uuid, "spawned a thread in order to automatically renew the lock");

                loop {
                    // recv_timeout works as the sleep, and it also wakes up early when the lock is released or the guard is dropped
                    match stop_rx.recv_timeout(renew_interval.unwrap_or_else(|| ttl / 2)) {
                        Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                            tracing::trace!(uuid = %uuid, "a spawned renewal thread for the lock is stopped because the lock is being released");

                            break;
                        },
                        Err(RecvTimeoutError::Timeout) => {
                            let result = pool.checkout().and_then(|mut conn| {
                                renew_lock_sync(&mut *conn, key.as_str(), uuid.as_str(), ttl)
                            });

                            match result {
                                Ok(false) => {
                                    tracing::trace!(uuid = %uuid, "cannot find the lock, close the renewal thread");
                                    break;
                                },
                                Ok(true) => {
                                    tracing::trace!(uuid = %uuid, "renewed the lock");

                                    continue;
                                },
                                Err(error) => {
                                    tracing::error!(uuid = %uuid, "an error occured when renewing the lock: {error}");
                                    tracing::trace!(uuid = %uuid, "close the renewal thread for the lock");
                                    break;
                                },
                            }
                        },
                    }
                }
            });

            Some(stop_tx)
        } else {
            None
        };

        Ok(RLockGuard {
            rlock: self.clone(),
            is_released: false,
            key,
            uuid,
            stop_tx,
        })
    }
}
