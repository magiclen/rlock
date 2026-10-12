use std::sync::Arc;

use super::{RLock, guard::GuardData};
use crate::{AcquireError, AcquireOptions, LeaseState, ReleaseError, lock_request::LockTarget};

/// A guard for an acquired Redis mutex lease.
/// Dropping the guard stops renewal and queues release, with TTL expiry as a fallback if the queue is unavailable.
/// A guard may outlive its lease, so use `lease_state` or `wait_until_inactive` to observe loss.
#[derive(Debug)]
#[must_use = "Keep this guard alive while using the protected resource."]
pub struct RLockGuard {
    data: GuardData,
}

impl RLockGuard {
    /// Releases the lease and waits for the Redis operation to complete.
    /// Starting release stops renewal and marks the lease lost until release completes.
    /// Cancellation or an error leaves the guard lost and allows release to be retried.
    pub async fn release(&mut self) -> Result<(), ReleaseError> {
        self.data.release().await
    }

    /// Returns whether explicit release completed successfully.
    /// This does not indicate whether an unreleased lease is still active.
    pub const fn is_released(&self) -> bool {
        self.data.is_released
    }

    /// Returns the local lease state using its conservative deadline.
    pub fn lease_state(&self) -> LeaseState {
        self.data.lease.state()
    }

    /// Waits until the lease is lost or released, including when renewal is disabled.
    pub async fn wait_until_inactive(&self) -> LeaseState {
        self.data.lease.wait_until_inactive().await
    }
}

impl RLock {
    /// Acquires a mutex lease with the default options.
    pub async fn acquire_mutex(&self, key: impl AsRef<str>) -> Result<RLockGuard, AcquireError> {
        self.acquire_mutex_with_options(key, AcquireOptions::default()).await
    }

    /// Acquires the lease using the supplied options.
    /// A lock timeout covers the complete wait, including Redis I/O, and a zero timeout sends no command.
    /// Cancelling the wait retains cleanup responsibility for an in-flight acquisition.
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

    /// Attempts to acquire a mutex lease without retrying contention.
    /// Returns `Ok(None)` when another holder prevents acquisition.
    pub async fn try_acquire_mutex(
        &self,
        key: impl AsRef<str>,
    ) -> Result<Option<RLockGuard>, AcquireError> {
        self.try_acquire_mutex_with_options(key, AcquireOptions::default()).await
    }

    /// Attempts one acquisition using the supplied options.
    /// A lock timeout covers the complete wait, including Redis I/O, and a zero timeout sends no command.
    /// Cancelling the wait retains cleanup responsibility for an in-flight acquisition.
    /// Contention returns `Ok(None)`, while Redis failures and timeouts return an error.
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
