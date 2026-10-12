use std::sync::Arc;

use super::{RLock, guard::GuardData};
use crate::{AcquireError, AcquireOptions, LeaseState, ReleaseError, lock_request::LockTarget};

/// A guard object representing an acquired Redis read lock.
///
/// # Behavior
///
/// * Multiple read guards can hold the same key at the same time, while a writer is excluded until all live readers are gone.
/// * Dropping the guard releases the lock in the background; if the queue of the lock release manager is full or unavailable, the lock is released after its TTL expires in Redis.
/// * Unless renewal is disabled, a background task (aka the renewal task) extends the lock's TTL until the lock is released.
/// * The guard can outlive its lease, so use `lease_state` or `wait_until_inactive` to observe a lost lease.
#[derive(Debug)]
#[must_use = "Keep this guard alive while using the protected resource."]
pub struct RReadLockGuard {
    data: GuardData,
}

impl RReadLockGuard {
    /// Releases the lock and waits for the Redis operation to complete.
    /// Starting release stops renewal and marks the lease lost until release completes.
    /// Cancellation or an error leaves the lease lost and allows release to be retried.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rlock::async_lock::RLock;
    ///
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn main() {
    /// let rlock = RLock::new("redis://127.0.0.1:6379/0").await.unwrap();
    ///
    /// let mut lock = rlock.acquire_read("rlock:example").await.unwrap();
    ///
    /// // shared section
    ///
    /// lock.release().await.unwrap();
    /// # }
    /// ```
    pub async fn release(&mut self) -> Result<(), ReleaseError> {
        self.data.release().await
    }

    /// Returns whether explicit release completed successfully.
    /// This does not indicate whether an unreleased lease is still active.
    pub const fn is_released(&self) -> bool {
        self.data.is_released
    }

    /// Returns the local lease state using its conservative deadline.
    /// `LeaseState::Active` does not guarantee that a later external write is safe.
    pub fn lease_state(&self) -> LeaseState {
        self.data.lease.state()
    }

    /// Waits until the lease is lost or released, including when renewal is disabled.
    pub async fn wait_until_inactive(&self) -> LeaseState {
        self.data.lease.wait_until_inactive().await
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
    /// use rlock::async_lock::RLock;
    ///
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn main() {
    /// let rlock = RLock::new("redis://127.0.0.1:6379/0").await.unwrap();
    ///
    /// let lock = rlock.acquire_read("rlock:example").await.unwrap();
    ///
    /// // shared section
    ///
    /// // drop(lock);
    /// # }
    /// ```
    pub async fn acquire_read(&self, key: impl AsRef<str>) -> Result<RReadLockGuard, AcquireError> {
        self.acquire_read_with_options(key, AcquireOptions::default()).await
    }

    /// Acquires a read (shared) lock with a key and options.
    /// A lock timeout covers retries and Redis I/O, and a zero timeout sends no command.
    /// Cancelling the wait leaves the cleanup of an in-flight acquisition to a background task.
    ///
    /// See [`RLock::acquire_read`].
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use std::time::Duration;
    ///
    /// use rlock::{AcquireOptions, async_lock::RLock};
    ///
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn main() {
    /// let rlock = RLock::new("redis://127.0.0.1:6379/0").await.unwrap();
    ///
    /// let lock = rlock
    ///     .acquire_read_with_options(
    ///         "rlock:example",
    ///         AcquireOptions::builder()
    ///             .ttl(Duration::from_secs(3))
    ///             .build()
    ///             .unwrap(),
    ///     )
    ///     .await
    ///     .unwrap();
    ///
    /// // shared section
    ///
    /// // drop(lock);
    /// # }
    /// ```
    pub async fn acquire_read_with_options(
        &self,
        key: impl AsRef<str>,
        options: AcquireOptions,
    ) -> Result<RReadLockGuard, AcquireError> {
        let result = self
            .acquire_internal(LockTarget::Read(Arc::new(key.as_ref().to_owned())), options, false)
            .await?;
        Ok(RReadLockGuard {
            data: result.expect("retrying acquisition returns a guard")
        })
    }

    /// Attempts to acquire a read (shared) lock once with a key.
    /// Contention returns `Ok(None)`.
    pub async fn try_acquire_read(
        &self,
        key: impl AsRef<str>,
    ) -> Result<Option<RReadLockGuard>, AcquireError> {
        self.try_acquire_read_with_options(key, AcquireOptions::default()).await
    }

    /// Attempts to acquire a read (shared) lock once with a key and options.
    /// Contention returns `Ok(None)`, while Redis failures and timeouts return an error.
    /// Cancelling the wait leaves the cleanup of an in-flight acquisition to a background task.
    pub async fn try_acquire_read_with_options(
        &self,
        key: impl AsRef<str>,
        options: AcquireOptions,
    ) -> Result<Option<RReadLockGuard>, AcquireError> {
        let result = self
            .acquire_internal(LockTarget::Read(Arc::new(key.as_ref().to_owned())), options, true)
            .await?;
        Ok(result.map(|data| RReadLockGuard {
            data,
        }))
    }
}

/// A guard object representing an acquired Redis write lock.
///
/// # Behavior
///
/// * A write guard is exclusive: it excludes other writers and all readers on the same key.
/// * Dropping the guard releases the lock in the background; if the queue of the lock release manager is full or unavailable, the lock is released after its TTL expires in Redis.
/// * Unless renewal is disabled, a background task (aka the renewal task) extends the lock's TTL until the lock is released.
/// * The guard can outlive its lease, so use `lease_state` or `wait_until_inactive` to observe a lost lease.
#[derive(Debug)]
#[must_use = "Keep this guard alive while using the protected resource."]
pub struct RWriteLockGuard {
    data: GuardData,
}

impl RWriteLockGuard {
    /// Releases the lock and waits for the Redis operation to complete.
    /// Starting release stops renewal and marks the lease lost until release completes.
    /// Cancellation or an error leaves the lease lost and allows release to be retried.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rlock::async_lock::RLock;
    ///
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn main() {
    /// let rlock = RLock::new("redis://127.0.0.1:6379/0").await.unwrap();
    ///
    /// let mut lock = rlock.acquire_write("rlock:example").await.unwrap();
    ///
    /// // critical section
    ///
    /// lock.release().await.unwrap();
    /// # }
    /// ```
    pub async fn release(&mut self) -> Result<(), ReleaseError> {
        self.data.release().await
    }

    /// Returns whether explicit release completed successfully.
    /// This does not indicate whether an unreleased lease is still active.
    pub const fn is_released(&self) -> bool {
        self.data.is_released
    }

    /// Returns the local lease state using its conservative deadline.
    /// `LeaseState::Active` does not guarantee that a later external write is safe.
    pub fn lease_state(&self) -> LeaseState {
        self.data.lease.state()
    }

    /// Waits until the lease is lost or released, including when renewal is disabled.
    pub async fn wait_until_inactive(&self) -> LeaseState {
        self.data.lease.wait_until_inactive().await
    }
}

impl RLock {
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
    /// use rlock::async_lock::RLock;
    ///
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn main() {
    /// let rlock = RLock::new("redis://127.0.0.1:6379/0").await.unwrap();
    ///
    /// let lock = rlock.acquire_write("rlock:example").await.unwrap();
    ///
    /// // critical section
    ///
    /// // drop(lock);
    /// # }
    /// ```
    pub async fn acquire_write(
        &self,
        key: impl AsRef<str>,
    ) -> Result<RWriteLockGuard, AcquireError> {
        self.acquire_write_with_options(key, AcquireOptions::default()).await
    }

    /// Acquires a write (exclusive) lock with a key and options.
    /// A lock timeout covers retries and Redis I/O, and a zero timeout sends no command.
    /// Cancelling the wait leaves the cleanup of an in-flight acquisition to a background task.
    ///
    /// See [`RLock::acquire_write`].
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use std::time::Duration;
    ///
    /// use rlock::{AcquireOptions, async_lock::RLock};
    ///
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn main() {
    /// let rlock = RLock::new("redis://127.0.0.1:6379/0").await.unwrap();
    ///
    /// let lock = rlock
    ///     .acquire_write_with_options(
    ///         "rlock:example",
    ///         AcquireOptions::builder()
    ///             .lock_timeout(Some(Duration::from_secs(3)))
    ///             .build()
    ///             .unwrap(),
    ///     )
    ///     .await
    ///     .unwrap();
    ///
    /// // critical section
    ///
    /// // drop(lock);
    /// # }
    /// ```
    pub async fn acquire_write_with_options(
        &self,
        key: impl AsRef<str>,
        options: AcquireOptions,
    ) -> Result<RWriteLockGuard, AcquireError> {
        let result = self
            .acquire_internal(LockTarget::Write(Arc::new(key.as_ref().to_owned())), options, false)
            .await?;
        Ok(RWriteLockGuard {
            data: result.expect("retrying acquisition returns a guard")
        })
    }

    /// Attempts to acquire a write (exclusive) lock once with a key.
    /// Contention returns `Ok(None)`.
    pub async fn try_acquire_write(
        &self,
        key: impl AsRef<str>,
    ) -> Result<Option<RWriteLockGuard>, AcquireError> {
        self.try_acquire_write_with_options(key, AcquireOptions::default()).await
    }

    /// Attempts to acquire a write (exclusive) lock once with a key and options.
    /// Contention returns `Ok(None)`, while Redis failures and timeouts return an error.
    /// Cancelling the wait leaves the cleanup of an in-flight acquisition to a background task.
    pub async fn try_acquire_write_with_options(
        &self,
        key: impl AsRef<str>,
        options: AcquireOptions,
    ) -> Result<Option<RWriteLockGuard>, AcquireError> {
        let result = self
            .acquire_internal(LockTarget::Write(Arc::new(key.as_ref().to_owned())), options, true)
            .await?;
        Ok(result.map(|data| RWriteLockGuard {
            data,
        }))
    }
}
