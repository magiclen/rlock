use std::sync::Arc;

use super::{RLock, guard::GuardData};
use crate::{AcquireError, AcquireOptions, LeaseState, ReleaseError, lock_request::LockTarget};

/// A guard object representing an acquired Redis read lock.
///
/// # Behavior
///
/// * Multiple read guards can hold the same key at the same time, while a writer is excluded until all live readers are gone.
/// * Dropping the guard releases the lock in the background; if the queue of the lock release manager is full or unavailable, the lock is released after its TTL expires in Redis.
/// * Unless renewal is disabled, a background thread (aka the renewal thread) extends the lock's TTL until the lock is released.
/// * The guard can outlive its lease, so use `lease_state` to observe a lost lease.
#[derive(Debug)]
#[must_use = "Keep this guard alive while using the protected resource."]
pub struct RReadLockGuard {
    inner: GuardData,
}

impl RReadLockGuard {
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
    /// let mut lock = rlock.acquire_read("rlock:example").unwrap();
    ///
    /// // shared section
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

/// A guard object representing an acquired Redis write lock.
///
/// # Behavior
///
/// * A write guard is exclusive: it excludes other writers and all readers on the same key.
/// * Dropping the guard releases the lock in the background; if the queue of the lock release manager is full or unavailable, the lock is released after its TTL expires in Redis.
/// * Unless renewal is disabled, a background thread (aka the renewal thread) extends the lock's TTL until the lock is released.
/// * The guard can outlive its lease, so use `lease_state` to observe a lost lease.
#[derive(Debug)]
#[must_use = "Keep this guard alive while using the protected resource."]
pub struct RWriteLockGuard {
    inner: GuardData,
}

impl RWriteLockGuard {
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
    /// let mut lock = rlock.acquire_write("rlock:example").unwrap();
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
    /// Acquires a read (shared) lock with a key.
    ///
    /// # Behavior
    ///
    /// * Multiple readers can hold the lock on the same key at the same time, while a writer is exclusive.
    /// * Readers are preferred: a writer may wait indefinitely under a continuous stream of readers, so consider setting a lock timeout on the writer side.
    /// * The lock is stored as a Redis sorted set and its scripts write after reading the server time, so Redis 5.0 or later is required.
    /// * Do not use the same key for a mutex and a read-write lock because their Redis types are different.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rlock::sync_lock::RLock;
    ///
    /// let rlock = RLock::new("redis://127.0.0.1:6379/0").unwrap();
    ///
    /// let lock = rlock.acquire_read("rlock:example").unwrap();
    ///
    /// // shared section
    ///
    /// // drop(lock);
    /// ```
    #[inline]
    pub fn acquire_read(&self, key: impl AsRef<str>) -> Result<RReadLockGuard, AcquireError> {
        self.acquire_read_with_options(key, AcquireOptions::default())
    }

    /// Acquires a read (shared) lock with a key and options.
    /// A lock timeout covers retries and Redis I/O, and a zero timeout sends no command.
    ///
    /// See [`RLock::acquire_read`].
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
    ///     .acquire_read_with_options(
    ///         "rlock:example",
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
    pub fn acquire_read_with_options(
        &self,
        key: impl AsRef<str>,
        options: AcquireOptions,
    ) -> Result<RReadLockGuard, AcquireError> {
        let inner = self
            .acquire_internal(LockTarget::Read(Arc::new(key.as_ref().to_owned())), options, false)?
            .expect("retrying acquisition returns a guard");

        Ok(RReadLockGuard {
            inner,
        })
    }

    /// Acquires a write (exclusive) lock with a key.
    ///
    /// # Behavior
    ///
    /// * A writer excludes other writers and all readers on the same key.
    /// * Readers are preferred: a writer may wait indefinitely under a continuous stream of readers, so consider setting a lock timeout.
    /// * The lock is stored as a Redis sorted set and its scripts write after reading the server time, so Redis 5.0 or later is required.
    /// * Do not use the same key for a mutex and a read-write lock because their Redis types are different.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rlock::sync_lock::RLock;
    ///
    /// let rlock = RLock::new("redis://127.0.0.1:6379/0").unwrap();
    ///
    /// let lock = rlock.acquire_write("rlock:example").unwrap();
    ///
    /// // critical section
    ///
    /// // drop(lock);
    /// ```
    #[inline]
    pub fn acquire_write(&self, key: impl AsRef<str>) -> Result<RWriteLockGuard, AcquireError> {
        self.acquire_write_with_options(key, AcquireOptions::default())
    }

    /// Acquires a write (exclusive) lock with a key and options.
    /// A lock timeout covers retries and Redis I/O, and a zero timeout sends no command.
    ///
    /// See [`RLock::acquire_write`].
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
    ///     .acquire_write_with_options(
    ///         "rlock:example",
    ///         AcquireOptions::builder()
    ///             .lock_timeout(Some(Duration::from_secs(3)))
    ///             .build()
    ///             .unwrap(),
    ///     )
    ///     .unwrap();
    ///
    /// // critical section
    ///
    /// // drop(lock);
    /// ```
    pub fn acquire_write_with_options(
        &self,
        key: impl AsRef<str>,
        options: AcquireOptions,
    ) -> Result<RWriteLockGuard, AcquireError> {
        let inner = self
            .acquire_internal(LockTarget::Write(Arc::new(key.as_ref().to_owned())), options, false)?
            .expect("retrying acquisition returns a guard");

        Ok(RWriteLockGuard {
            inner,
        })
    }

    /// Attempts to acquire a read (shared) lock once with a key.
    /// Contention returns `Ok(None)`.
    #[inline]
    pub fn try_acquire_read(
        &self,
        key: impl AsRef<str>,
    ) -> Result<Option<RReadLockGuard>, AcquireError> {
        self.try_acquire_read_with_options(key, AcquireOptions::default())
    }

    /// Attempts to acquire a read (shared) lock once with a key and options.
    /// Contention returns `Ok(None)`, while Redis failures and timeouts return an error.
    pub fn try_acquire_read_with_options(
        &self,
        key: impl AsRef<str>,
        options: AcquireOptions,
    ) -> Result<Option<RReadLockGuard>, AcquireError> {
        self.acquire_internal(LockTarget::Read(Arc::new(key.as_ref().to_owned())), options, true)
            .map(|guard| {
                guard.map(|inner| RReadLockGuard {
                    inner,
                })
            })
    }

    /// Attempts to acquire a write (exclusive) lock once with a key.
    /// Contention returns `Ok(None)`.
    #[inline]
    pub fn try_acquire_write(
        &self,
        key: impl AsRef<str>,
    ) -> Result<Option<RWriteLockGuard>, AcquireError> {
        self.try_acquire_write_with_options(key, AcquireOptions::default())
    }

    /// Attempts to acquire a write (exclusive) lock once with a key and options.
    /// Contention returns `Ok(None)`, while Redis failures and timeouts return an error.
    pub fn try_acquire_write_with_options(
        &self,
        key: impl AsRef<str>,
        options: AcquireOptions,
    ) -> Result<Option<RWriteLockGuard>, AcquireError> {
        self.acquire_internal(LockTarget::Write(Arc::new(key.as_ref().to_owned())), options, true)
            .map(|guard| {
                guard.map(|inner| RWriteLockGuard {
                    inner,
                })
            })
    }
}
