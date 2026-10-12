use std::sync::Arc;

use super::{RLock, guard::GuardData};
use crate::{AcquireError, AcquireOptions, LeaseState, ReleaseError, lock_request::LockTarget};

/// A guard object representing an acquired Redis mutex lock.
///
/// # Behavior
///
/// * Dropping the guard releases the lock in the background; if the queue of the lock release manager is full or unavailable, the lock is released after its TTL expires in Redis.
/// * Unless renewal is disabled, a background task (aka the renewal task) extends the lock's TTL until the lock is released.
/// * The guard can outlive its lease, so use `lease_state` or `wait_until_inactive` to observe a lost lease.
#[derive(Debug)]
#[must_use = "Keep this guard alive while using the protected resource."]
pub struct RLockGuard {
    data: GuardData,
}

impl RLockGuard {
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
    /// let mut lock = rlock.acquire_mutex("rlock:example").await.unwrap();
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
    /// Acquires a mutex lock with a key.
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
    /// let lock = rlock.acquire_mutex("rlock:example").await.unwrap();
    ///
    /// // critical section
    ///
    /// // drop(lock);
    /// # }
    /// ```
    pub async fn acquire_mutex(&self, key: impl AsRef<str>) -> Result<RLockGuard, AcquireError> {
        self.acquire_mutex_with_options(key, AcquireOptions::default()).await
    }

    /// Acquires a mutex lock with a key and options.
    /// A lock timeout covers retries and Redis I/O, and a zero timeout sends no command.
    /// Cancelling the wait leaves the cleanup of an in-flight acquisition to a background task.
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
    ///     .acquire_mutex_with_options(
    ///         "rlock:example",
    ///         AcquireOptions::builder()
    ///             .ttl(Duration::from_secs(3))
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
    pub async fn acquire_mutex_with_options(
        &self,
        key: impl AsRef<str>,
        options: AcquireOptions,
    ) -> Result<RLockGuard, AcquireError> {
        let result = self
            .acquire_internal(LockTarget::Single(Arc::new(key.as_ref().to_owned())), options, false)
            .await?;
        Ok(RLockGuard {
            data: result.expect("retrying acquisition returns a guard")
        })
    }

    /// Attempts to acquire a mutex lock once with a key.
    /// Contention returns `Ok(None)`.
    pub async fn try_acquire_mutex(
        &self,
        key: impl AsRef<str>,
    ) -> Result<Option<RLockGuard>, AcquireError> {
        self.try_acquire_mutex_with_options(key, AcquireOptions::default()).await
    }

    /// Attempts to acquire a mutex lock once with a key and options.
    /// Contention returns `Ok(None)`, while Redis failures and timeouts return an error.
    /// Cancelling the wait leaves the cleanup of an in-flight acquisition to a background task.
    pub async fn try_acquire_mutex_with_options(
        &self,
        key: impl AsRef<str>,
        options: AcquireOptions,
    ) -> Result<Option<RLockGuard>, AcquireError> {
        let result = self
            .acquire_internal(LockTarget::Single(Arc::new(key.as_ref().to_owned())), options, true)
            .await?;
        Ok(result.map(|data| RLockGuard {
            data,
        }))
    }
}
