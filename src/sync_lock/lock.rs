use std::sync::Arc;

use super::{RLock, guard::GuardData};
use crate::{AcquireError, AcquireOptions, LeaseState, ReleaseError, lock_request::LockTarget};

/// A guard object representing an acquired Redis mutex lock.
///
/// # Behavior
///
/// * Dropping the guard releases the lock in the background; if the queue of the lock release manager is full or unavailable, the lock is released after its TTL expires in Redis.
/// * Unless renewal is disabled, a background thread (aka the renewal thread) extends the lock's TTL until the lock is released.
/// * The guard can outlive its lease, so use `lease_state` to observe a lost lease.
#[derive(Debug)]
#[must_use = "Keep this guard alive while using the protected resource."]
pub struct RLockGuard {
    inner: GuardData,
}

impl RLockGuard {
    /// Releases the lock and waits for the Redis operation to complete.
    /// Starting release stops renewal and marks the lease lost until release completes.
    /// An error leaves the lease lost and allows release to be retried.
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
    /// // critical section
    ///
    /// lock.release().unwrap();
    /// ```
    #[inline]
    pub fn release(&mut self) -> Result<(), ReleaseError> {
        self.inner.release()
    }

    /// Returns whether explicit release completed successfully.
    /// This does not indicate whether an unreleased lease is still active.
    #[inline]
    pub const fn is_released(&self) -> bool {
        self.inner.is_released
    }

    /// Returns the local lease state using its conservative deadline.
    /// `LeaseState::Active` does not guarantee that a later external write is safe.
    #[inline]
    pub fn lease_state(&self) -> LeaseState {
        self.inner.lease.state()
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
    /// // critical section
    ///
    /// // drop(lock);
    /// ```
    #[inline]
    pub fn acquire_mutex(&self, key: impl AsRef<str>) -> Result<RLockGuard, AcquireError> {
        self.acquire_mutex_with_options(key, AcquireOptions::default())
    }

    /// Acquires a mutex lock with a key and options.
    /// A lock timeout covers retries and Redis I/O, and a zero timeout sends no command.
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
    /// // critical section
    ///
    /// // drop(lock);
    /// ```
    pub fn acquire_mutex_with_options(
        &self,
        key: impl AsRef<str>,
        options: AcquireOptions,
    ) -> Result<RLockGuard, AcquireError> {
        let inner = self
            .acquire_internal(
                LockTarget::Single(Arc::new(key.as_ref().to_owned())),
                options,
                false,
            )?
            .expect("retrying acquisition returns a guard");

        Ok(RLockGuard {
            inner,
        })
    }

    /// Attempts to acquire a mutex lock once with a key.
    /// Contention returns `Ok(None)`.
    #[inline]
    pub fn try_acquire_mutex(
        &self,
        key: impl AsRef<str>,
    ) -> Result<Option<RLockGuard>, AcquireError> {
        self.try_acquire_mutex_with_options(key, AcquireOptions::default())
    }

    /// Attempts to acquire a mutex lock once with a key and options.
    /// Contention returns `Ok(None)`, while Redis failures and timeouts return an error.
    pub fn try_acquire_mutex_with_options(
        &self,
        key: impl AsRef<str>,
        options: AcquireOptions,
    ) -> Result<Option<RLockGuard>, AcquireError> {
        self.acquire_internal(LockTarget::Single(Arc::new(key.as_ref().to_owned())), options, true)
            .map(|guard| {
                guard.map(|inner| RLockGuard {
                    inner,
                })
            })
    }
}
