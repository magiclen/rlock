use std::sync::Arc;

use super::{RLock, guard::GuardData};
use crate::{
    AcquireError, AcquireOptions, LeaseState, ReleaseError, release_request::ReleaseRequestKey,
};

/// A guard for an acquired Redis multi-key read lease.
/// Dropping the guard stops renewal and queues release, with TTL expiry as a fallback if the queue is unavailable.
/// A guard may outlive its lease, so use `lease_state` or `wait_until_inactive` to observe loss.
#[derive(Debug)]
#[must_use = "Keep this guard alive while using the protected resource."]
pub struct RMultiKeyReadLockGuard {
    data: GuardData,
}

impl RMultiKeyReadLockGuard {
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
    /// Acquires a multi-key read lease with the default options.
    /// The keys must be nonempty and unique, which debug builds assert.
    pub async fn acquire_multi_key_read(
        &self,
        keys: impl Into<Arc<Vec<String>>>,
    ) -> Result<RMultiKeyReadLockGuard, AcquireError> {
        self.acquire_multi_key_read_with_options(keys, AcquireOptions::default()).await
    }

    /// Acquires the lease using the supplied options.
    /// A lock timeout covers the complete wait, including Redis I/O, and a zero timeout sends no command.
    /// Cancelling the wait retains cleanup responsibility for an in-flight acquisition.
    /// The keys must be nonempty and unique, which debug builds assert.
    pub async fn acquire_multi_key_read_with_options(
        &self,
        keys: impl Into<Arc<Vec<String>>>,
        options: AcquireOptions,
    ) -> Result<RMultiKeyReadLockGuard, AcquireError> {
        let keys = keys.into();
        #[cfg(debug_assertions)]
        {
            let mut unique = std::collections::HashSet::with_capacity(keys.len());
            debug_assert!(!keys.is_empty(), "multi-key locks require at least one key");
            for key in keys.iter() {
                debug_assert!(unique.insert(key.as_str()), "multi-key locks require unique keys");
            }
        }
        let result =
            self.acquire_internal(ReleaseRequestKey::MultipleRead(keys), options, false).await?;
        Ok(RMultiKeyReadLockGuard {
            data: result.expect("retrying acquisition returns a guard")
        })
    }

    /// Attempts to acquire a multi-key read lease without retrying contention.
    /// Returns `Ok(None)` when another holder prevents acquisition.
    /// The keys must be nonempty and unique, which debug builds assert.
    pub async fn try_acquire_multi_key_read(
        &self,
        keys: impl Into<Arc<Vec<String>>>,
    ) -> Result<Option<RMultiKeyReadLockGuard>, AcquireError> {
        self.try_acquire_multi_key_read_with_options(keys, AcquireOptions::default()).await
    }

    /// Attempts one acquisition using the supplied options.
    /// A lock timeout covers the complete wait, including Redis I/O, and a zero timeout sends no command.
    /// Cancelling the wait retains cleanup responsibility for an in-flight acquisition.
    /// The keys must be nonempty and unique, which debug builds assert.
    /// Contention returns `Ok(None)`, while Redis failures and timeouts return an error.
    pub async fn try_acquire_multi_key_read_with_options(
        &self,
        keys: impl Into<Arc<Vec<String>>>,
        options: AcquireOptions,
    ) -> Result<Option<RMultiKeyReadLockGuard>, AcquireError> {
        let keys = keys.into();
        #[cfg(debug_assertions)]
        {
            let mut unique = std::collections::HashSet::with_capacity(keys.len());
            debug_assert!(!keys.is_empty(), "multi-key locks require at least one key");
            for key in keys.iter() {
                debug_assert!(unique.insert(key.as_str()), "multi-key locks require unique keys");
            }
        }
        let result =
            self.acquire_internal(ReleaseRequestKey::MultipleRead(keys), options, true).await?;
        Ok(result.map(|data| RMultiKeyReadLockGuard {
            data,
        }))
    }
}

/// A guard for an acquired Redis multi-key write lease.
/// Dropping the guard stops renewal and queues release, with TTL expiry as a fallback if the queue is unavailable.
/// A guard may outlive its lease, so use `lease_state` or `wait_until_inactive` to observe loss.
#[derive(Debug)]
#[must_use = "Keep this guard alive while using the protected resource."]
pub struct RMultiKeyWriteLockGuard {
    data: GuardData,
}

impl RMultiKeyWriteLockGuard {
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
    /// Acquires a multi-key write lease with the default options.
    /// The keys must be nonempty and unique, which debug builds assert.
    /// Continuous readers may delay writers indefinitely, so consider a lock timeout.
    pub async fn acquire_multi_key_write(
        &self,
        keys: impl Into<Arc<Vec<String>>>,
    ) -> Result<RMultiKeyWriteLockGuard, AcquireError> {
        self.acquire_multi_key_write_with_options(keys, AcquireOptions::default()).await
    }

    /// Acquires the lease using the supplied options.
    /// A lock timeout covers the complete wait, including Redis I/O, and a zero timeout sends no command.
    /// Cancelling the wait retains cleanup responsibility for an in-flight acquisition.
    /// The keys must be nonempty and unique, which debug builds assert.
    pub async fn acquire_multi_key_write_with_options(
        &self,
        keys: impl Into<Arc<Vec<String>>>,
        options: AcquireOptions,
    ) -> Result<RMultiKeyWriteLockGuard, AcquireError> {
        let keys = keys.into();
        #[cfg(debug_assertions)]
        {
            let mut unique = std::collections::HashSet::with_capacity(keys.len());
            debug_assert!(!keys.is_empty(), "multi-key locks require at least one key");
            for key in keys.iter() {
                debug_assert!(unique.insert(key.as_str()), "multi-key locks require unique keys");
            }
        }
        let result =
            self.acquire_internal(ReleaseRequestKey::MultipleWrite(keys), options, false).await?;
        Ok(RMultiKeyWriteLockGuard {
            data: result.expect("retrying acquisition returns a guard")
        })
    }

    /// Attempts to acquire a multi-key write lease without retrying contention.
    /// Returns `Ok(None)` when another holder prevents acquisition.
    /// The keys must be nonempty and unique, which debug builds assert.
    pub async fn try_acquire_multi_key_write(
        &self,
        keys: impl Into<Arc<Vec<String>>>,
    ) -> Result<Option<RMultiKeyWriteLockGuard>, AcquireError> {
        self.try_acquire_multi_key_write_with_options(keys, AcquireOptions::default()).await
    }

    /// Attempts one acquisition using the supplied options.
    /// A lock timeout covers the complete wait, including Redis I/O, and a zero timeout sends no command.
    /// Cancelling the wait retains cleanup responsibility for an in-flight acquisition.
    /// The keys must be nonempty and unique, which debug builds assert.
    /// Contention returns `Ok(None)`, while Redis failures and timeouts return an error.
    pub async fn try_acquire_multi_key_write_with_options(
        &self,
        keys: impl Into<Arc<Vec<String>>>,
        options: AcquireOptions,
    ) -> Result<Option<RMultiKeyWriteLockGuard>, AcquireError> {
        let keys = keys.into();
        #[cfg(debug_assertions)]
        {
            let mut unique = std::collections::HashSet::with_capacity(keys.len());
            debug_assert!(!keys.is_empty(), "multi-key locks require at least one key");
            for key in keys.iter() {
                debug_assert!(unique.insert(key.as_str()), "multi-key locks require unique keys");
            }
        }
        let result =
            self.acquire_internal(ReleaseRequestKey::MultipleWrite(keys), options, true).await?;
        Ok(result.map(|data| RMultiKeyWriteLockGuard {
            data,
        }))
    }
}
