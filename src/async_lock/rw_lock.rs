use std::sync::Arc;

use super::{RLock, guard::GuardData};
use crate::{AcquireError, AcquireOptions, LeaseState, ReleaseError, lock_request::LockTarget};

/// A guard for an acquired Redis read lease.
/// Dropping the guard stops renewal and queues release, with TTL expiry as a fallback if the queue is unavailable.
/// A guard may outlive its lease, so use `lease_state` or `wait_until_inactive` to observe loss.
#[derive(Debug)]
#[must_use = "Keep this guard alive while using the protected resource."]
pub struct RReadLockGuard {
    data: GuardData,
}

impl RReadLockGuard {
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
    /// Acquires a read lease with the default options.
    pub async fn acquire_read(&self, key: impl AsRef<str>) -> Result<RReadLockGuard, AcquireError> {
        self.acquire_read_with_options(key, AcquireOptions::default()).await
    }

    /// Acquires the lease using the supplied options.
    /// A lock timeout covers the complete wait, including Redis I/O, and a zero timeout sends no command.
    /// Cancelling the wait retains cleanup responsibility for an in-flight acquisition.
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

    /// Attempts to acquire a read lease without retrying contention.
    /// Returns `Ok(None)` when another holder prevents acquisition.
    pub async fn try_acquire_read(
        &self,
        key: impl AsRef<str>,
    ) -> Result<Option<RReadLockGuard>, AcquireError> {
        self.try_acquire_read_with_options(key, AcquireOptions::default()).await
    }

    /// Attempts one acquisition using the supplied options.
    /// A lock timeout covers the complete wait, including Redis I/O, and a zero timeout sends no command.
    /// Cancelling the wait retains cleanup responsibility for an in-flight acquisition.
    /// Contention returns `Ok(None)`, while Redis failures and timeouts return an error.
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

/// A guard for an acquired Redis write lease.
/// Dropping the guard stops renewal and queues release, with TTL expiry as a fallback if the queue is unavailable.
/// A guard may outlive its lease, so use `lease_state` or `wait_until_inactive` to observe loss.
#[derive(Debug)]
#[must_use = "Keep this guard alive while using the protected resource."]
pub struct RWriteLockGuard {
    data: GuardData,
}

impl RWriteLockGuard {
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
    /// Acquires a write lease with the default options.
    /// Continuous readers may delay writers indefinitely, so consider a lock timeout.
    pub async fn acquire_write(
        &self,
        key: impl AsRef<str>,
    ) -> Result<RWriteLockGuard, AcquireError> {
        self.acquire_write_with_options(key, AcquireOptions::default()).await
    }

    /// Acquires the lease using the supplied options.
    /// A lock timeout covers the complete wait, including Redis I/O, and a zero timeout sends no command.
    /// Cancelling the wait retains cleanup responsibility for an in-flight acquisition.
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

    /// Attempts to acquire a write lease without retrying contention.
    /// Returns `Ok(None)` when another holder prevents acquisition.
    pub async fn try_acquire_write(
        &self,
        key: impl AsRef<str>,
    ) -> Result<Option<RWriteLockGuard>, AcquireError> {
        self.try_acquire_write_with_options(key, AcquireOptions::default()).await
    }

    /// Attempts one acquisition using the supplied options.
    /// A lock timeout covers the complete wait, including Redis I/O, and a zero timeout sends no command.
    /// Cancelling the wait retains cleanup responsibility for an in-flight acquisition.
    /// Contention returns `Ok(None)`, while Redis failures and timeouts return an error.
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
