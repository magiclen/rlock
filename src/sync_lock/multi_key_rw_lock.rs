#[cfg(debug_assertions)]
use std::collections::HashSet;
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

/// A guard object representing an acquired Redis read lock (multi-key).
///
/// # Behavior
///
/// * Multiple read guards can hold the same keys at the same time, while a writer is excluded until all live readers are gone.
/// * Dropping the guard automatically releases the lock unless the buffer of the lock release manager is full (in that case, the lock is released after its TTL expires in Redis)
/// * If the renewal interval is not set to `None`, a background thread (aka the renewal thread) is spawned to extend the lock's TTL. The thread is terminated when the lock is released.
#[derive(Debug)]
pub struct RMultiKeyReadLockGuard {
    rlock:       RLock,
    is_released: bool,
    keys:        Arc<Vec<String>>,
    uuid:        Arc<String>,
    stop_tx:     Option<Sender<()>>,
}

impl RMultiKeyReadLockGuard {
    /// Releases the lock. If the lock is released, returns `Ok(())`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use std::sync::Arc;
    ///
    /// use rlock::sync_lock::RLock;
    ///
    /// let rlock = RLock::new("redis://127.0.0.1:6379/0").unwrap();
    ///
    /// let mut lock = rlock
    ///     .acquire_multi_key_read(Arc::new(vec![
    ///         String::from("rlock:example"),
    ///         String::from("rlock:example2"),
    ///     ]))
    ///     .unwrap();
    ///
    /// // shared section
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

            release_multi_key_read_lock_sync(&mut *conn, &self.keys, self.uuid.as_str())?;
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

impl Drop for RMultiKeyReadLockGuard {
    #[inline]
    fn drop(&mut self) {
        if self.is_released {
            return;
        }

        if let Err(error) = self.rlock.request_tx.try_send(Some(ReleaseRequest {
            key:  ReleaseRequestKey::MultipleRead(self.keys.clone()),
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

/// A guard object representing an acquired Redis write lock (multi-key).
///
/// # Behavior
///
/// * A write guard is exclusive: it excludes other writers and all readers on the same keys.
/// * Dropping the guard automatically releases the lock unless the buffer of the lock release manager is full (in that case, the lock is released after its TTL expires in Redis)
/// * If the renewal interval is not set to `None`, a background thread (aka the renewal thread) is spawned to extend the lock's TTL. The thread is terminated when the lock is released.
#[derive(Debug)]
pub struct RMultiKeyWriteLockGuard {
    rlock:       RLock,
    is_released: bool,
    keys:        Arc<Vec<String>>,
    uuid:        Arc<String>,
    stop_tx:     Option<Sender<()>>,
}

impl RMultiKeyWriteLockGuard {
    /// Releases the lock. If the lock is released, returns `Ok(())`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use std::sync::Arc;
    ///
    /// use rlock::sync_lock::RLock;
    ///
    /// let rlock = RLock::new("redis://127.0.0.1:6379/0").unwrap();
    ///
    /// let mut lock = rlock
    ///     .acquire_multi_key_write(Arc::new(vec![
    ///         String::from("rlock:example"),
    ///         String::from("rlock:example2"),
    ///     ]))
    ///     .unwrap();
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

            release_multi_key_write_lock_sync(&mut *conn, &self.keys, self.uuid.as_str())?;
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

impl Drop for RMultiKeyWriteLockGuard {
    #[inline]
    fn drop(&mut self) {
        if self.is_released {
            return;
        }

        if let Err(error) = self.rlock.request_tx.try_send(Some(ReleaseRequest {
            key:  ReleaseRequestKey::MultipleWrite(self.keys.clone()),
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
    /// Acquires a read (shared) lock with mutiple keys.
    ///
    /// The key list must not be empty and each key must be unique.
    /// Debug builds assert these programmer invariants.
    /// Release builds skip these checks for performance because callers are expected to pass keys chosen by the program rather than direct user input.
    ///
    /// See [`RLock::acquire_read`] for the behavior of read locks.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use std::sync::Arc;
    ///
    /// use rlock::sync_lock::RLock;
    ///
    /// let rlock = RLock::new("redis://127.0.0.1:6379/0").unwrap();
    ///
    /// let lock = rlock
    ///     .acquire_multi_key_read(Arc::new(vec![
    ///         String::from("rlock:example"),
    ///         String::from("rlock:example2"),
    ///     ]))
    ///     .unwrap();
    ///
    /// // shared section
    ///
    /// // drop(lock);
    /// ```
    #[inline]
    pub fn acquire_multi_key_read(
        &self,
        keys: impl Into<Arc<Vec<String>>>,
    ) -> Result<RMultiKeyReadLockGuard, AcquireError> {
        self.acquire_multi_key_read_with_options(keys, AcquireOptions::default())
    }

    /// Acquires a read (shared) lock with mutiple keys and options.
    ///
    /// See [`RLock::acquire_multi_key_read`].
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use std::{sync::Arc, time::Duration};
    ///
    /// use rlock::{AcquireOptions, sync_lock::RLock};
    ///
    /// let rlock = RLock::new("redis://127.0.0.1:6379/0").unwrap();
    ///
    /// let lock = rlock
    ///     .acquire_multi_key_read_with_options(
    ///         Arc::new(vec![
    ///             String::from("rlock:example"),
    ///             String::from("rlock:example2"),
    ///         ]),
    ///         AcquireOptions::builder()
    ///             .ttl(Duration::from_secs(3))
    ///             .build()
    ///             .unwrap(),
    ///     )
    ///     .unwrap();
    ///
    /// // shared section
    ///
    /// // drop(lock);
    /// ```
    pub fn acquire_multi_key_read_with_options(
        &self,
        keys: impl Into<Arc<Vec<String>>>,
        options: AcquireOptions,
    ) -> Result<RMultiKeyReadLockGuard, AcquireError> {
        if self.inner.is_shutdown.load(Ordering::Relaxed) {
            return Err(AcquireError::Shutdown);
        }

        let keys = keys.into();

        #[cfg(debug_assertions)]
        {
            debug_assert!(!keys.is_empty(), "multi-key read lock requires at least one key");

            let mut unique_keys = HashSet::with_capacity(keys.len());

            for key in keys.iter() {
                debug_assert!(
                    unique_keys.insert(key.as_str()),
                    "multi-key read lock requires unique keys"
                );
            }
        }

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

                acquire_multi_key_read_lock_sync(&mut *conn, &keys, uuid.as_ref(), ttl)?
            };

            if result {
                break;
            }

            // a writer holds the lock, so we cannot acquire

            // check lock_timeout
            if let Some(lock_timeout) = lock_timeout
                && start_time.elapsed() >= lock_timeout
            {
                return Err(AcquireError::LockTimeout);
            }

            // sleep retry_interval to allow other threads to run
            thread::sleep(retry_interval);
        }

        // a new lock has been created

        let stop_tx = if let Some(renew_interval) = renew_interval {
            // renew automatically
            let (stop_tx, stop_rx) = mpsc::channel::<()>();

            let keys = keys.clone();
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
                                renew_multi_key_read_lock_sync(
                                    &mut *conn,
                                    &keys,
                                    uuid.as_str(),
                                    ttl,
                                )
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

        Ok(RMultiKeyReadLockGuard {
            rlock: self.clone(),
            is_released: false,
            keys,
            uuid,
            stop_tx,
        })
    }

    /// Acquires a write (exclusive) lock with mutiple keys.
    ///
    /// The key list must not be empty and each key must be unique.
    /// Debug builds assert these programmer invariants.
    /// Release builds skip these checks for performance because callers are expected to pass keys chosen by the program rather than direct user input.
    ///
    /// See [`RLock::acquire_write`] for the behavior of write locks.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use std::sync::Arc;
    ///
    /// use rlock::sync_lock::RLock;
    ///
    /// let rlock = RLock::new("redis://127.0.0.1:6379/0").unwrap();
    ///
    /// let lock = rlock
    ///     .acquire_multi_key_write(Arc::new(vec![
    ///         String::from("rlock:example"),
    ///         String::from("rlock:example2"),
    ///     ]))
    ///     .unwrap();
    ///
    /// // critial section
    ///
    /// // drop(lock);
    /// ```
    #[inline]
    pub fn acquire_multi_key_write(
        &self,
        keys: impl Into<Arc<Vec<String>>>,
    ) -> Result<RMultiKeyWriteLockGuard, AcquireError> {
        self.acquire_multi_key_write_with_options(keys, AcquireOptions::default())
    }

    /// Acquires a write (exclusive) lock with mutiple keys and options.
    ///
    /// See [`RLock::acquire_multi_key_write`].
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use std::{sync::Arc, time::Duration};
    ///
    /// use rlock::{AcquireOptions, sync_lock::RLock};
    ///
    /// let rlock = RLock::new("redis://127.0.0.1:6379/0").unwrap();
    ///
    /// let lock = rlock
    ///     .acquire_multi_key_write_with_options(
    ///         Arc::new(vec![
    ///             String::from("rlock:example"),
    ///             String::from("rlock:example2"),
    ///         ]),
    ///         AcquireOptions::builder()
    ///             .lock_timeout(Some(Duration::from_secs(3)))
    ///             .build()
    ///             .unwrap(),
    ///     )
    ///     .unwrap();
    ///
    /// // critial section
    ///
    /// // drop(lock);
    /// ```
    pub fn acquire_multi_key_write_with_options(
        &self,
        keys: impl Into<Arc<Vec<String>>>,
        options: AcquireOptions,
    ) -> Result<RMultiKeyWriteLockGuard, AcquireError> {
        if self.inner.is_shutdown.load(Ordering::Relaxed) {
            return Err(AcquireError::Shutdown);
        }

        let keys = keys.into();

        #[cfg(debug_assertions)]
        {
            debug_assert!(!keys.is_empty(), "multi-key write lock requires at least one key");

            let mut unique_keys = HashSet::with_capacity(keys.len());

            for key in keys.iter() {
                debug_assert!(
                    unique_keys.insert(key.as_str()),
                    "multi-key write lock requires unique keys"
                );
            }
        }

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

                acquire_multi_key_write_lock_sync(&mut *conn, &keys, uuid.as_ref(), ttl)?
            };

            if result {
                break;
            }

            // a writer or live readers hold the lock, so we cannot acquire

            // check lock_timeout
            if let Some(lock_timeout) = lock_timeout
                && start_time.elapsed() >= lock_timeout
            {
                return Err(AcquireError::LockTimeout);
            }

            // sleep retry_interval to allow other threads to run
            thread::sleep(retry_interval);
        }

        // a new lock has been created

        let stop_tx = if let Some(renew_interval) = renew_interval {
            // renew automatically
            let (stop_tx, stop_rx) = mpsc::channel::<()>();

            let keys = keys.clone();
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
                                renew_multi_key_write_lock_sync(
                                    &mut *conn,
                                    &keys,
                                    uuid.as_str(),
                                    ttl,
                                )
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

        Ok(RMultiKeyWriteLockGuard {
            rlock: self.clone(),
            is_released: false,
            keys,
            uuid,
            stop_tx,
        })
    }
}
