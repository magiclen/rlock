#[cfg(debug_assertions)]
use std::collections::HashSet;
use std::sync::Arc;

use super::{RLock, guard::GuardData};
use crate::{
    AcquireError, AcquireOptions, LeaseState, ReleaseError, release_request::ReleaseRequestKey,
};

/// A guard object representing an acquired Redis lock (multi-key).
///
/// # Behavior
///
/// * Dropping the guard automatically releases the lock unless the buffer of the lock release manager is full (in that case, the lock is released after its TTL expires in Redis)
/// * If the renewal interval is not set to `None`, a background thread (aka the renewal thread) is spawned to extend the lock's TTL. The thread is terminated when the lock is released.
#[derive(Debug)]
#[must_use = "Keep this guard alive while using the protected resource."]
pub struct RMultiKeyLockGuard {
    inner: GuardData,
}

impl RMultiKeyLockGuard {
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
    ///     .acquire_multi_key_mutex(Arc::new(vec![
    ///         String::from("rlock:example"),
    ///         String::from("rlock:example2"),
    ///     ]))
    ///     .unwrap();
    ///
    /// // critical section
    ///
    /// lock.release().unwrap();
    /// ```
    #[inline]
    pub fn release(&mut self) -> Result<(), ReleaseError> {
        self.inner.release()
    }

    /// Determines whether the lock is released.
    #[inline]
    pub const fn is_released(&self) -> bool {
        self.inner.is_released
    }

    /// Returns the local lease state; Active does not guarantee that a later external write is safe.
    #[inline]
    pub fn lease_state(&self) -> LeaseState {
        self.inner.lease.state()
    }
}

impl RLock {
    /// Acquires a mutex lock with multiple keys.
    ///
    /// The key list must not be empty and each key must be unique.
    /// Debug builds assert these programmer invariants.
    /// Release builds skip these checks for performance because callers are expected to pass keys chosen by the program rather than direct user input.
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
    ///     .acquire_multi_key_mutex(Arc::new(vec![
    ///         String::from("rlock:example"),
    ///         String::from("rlock:example2"),
    ///     ]))
    ///     .unwrap();
    ///
    /// // critical section
    ///
    /// // drop(lock);
    /// ```
    #[inline]
    pub fn acquire_multi_key_mutex(
        &self,
        keys: impl Into<Arc<Vec<String>>>,
    ) -> Result<RMultiKeyLockGuard, AcquireError> {
        self.acquire_multi_key_mutex_with_options(keys, AcquireOptions::default())
    }

    /// Acquires a mutex lock with multiple keys and options.
    ///
    /// The key list must not be empty and each key must be unique.
    /// Debug builds assert these programmer invariants.
    /// Release builds skip these checks for performance because callers are expected to pass keys chosen by the program rather than direct user input.
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
    ///     .acquire_multi_key_mutex_with_options(
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
    /// // critical section
    ///
    /// // drop(lock);
    /// ```
    pub fn acquire_multi_key_mutex_with_options(
        &self,
        keys: impl Into<Arc<Vec<String>>>,
        options: AcquireOptions,
    ) -> Result<RMultiKeyLockGuard, AcquireError> {
        let keys = keys.into();
        #[cfg(debug_assertions)]
        {
            debug_assert!(!keys.is_empty(), "multi-key lock requires at least one key");
            let mut unique = HashSet::with_capacity(keys.len());
            for key in keys.iter() {
                debug_assert!(unique.insert(key.as_str()), "multi-key lock requires unique keys");
            }
        }
        self.acquire_internal(ReleaseRequestKey::Multiple(keys), options, false)?
            .map(|inner| RMultiKeyLockGuard {
                inner,
            })
            .ok_or(AcquireError::LockTimeout)
    }

    /// Attempts to acquire a lock once; contention returns None and Redis IO can still wait.
    #[inline]
    pub fn try_acquire_multi_key_mutex(
        &self,
        keys: impl Into<Arc<Vec<String>>>,
    ) -> Result<Option<RMultiKeyLockGuard>, AcquireError> {
        self.try_acquire_multi_key_mutex_with_options(keys, AcquireOptions::default())
    }

    /// Attempts to acquire a lock once using the given TTL, renewal mode, and acquisition deadline.
    pub fn try_acquire_multi_key_mutex_with_options(
        &self,
        keys: impl Into<Arc<Vec<String>>>,
        options: AcquireOptions,
    ) -> Result<Option<RMultiKeyLockGuard>, AcquireError> {
        let keys = keys.into();
        #[cfg(debug_assertions)]
        {
            debug_assert!(!keys.is_empty(), "multi-key lock requires at least one key");
            let mut unique = HashSet::with_capacity(keys.len());
            for key in keys.iter() {
                debug_assert!(unique.insert(key.as_str()), "multi-key lock requires unique keys");
            }
        }
        self.acquire_internal(ReleaseRequestKey::Multiple(keys), options, true).map(|guard| {
            guard.map(|inner| RMultiKeyLockGuard {
                inner,
            })
        })
    }
}
