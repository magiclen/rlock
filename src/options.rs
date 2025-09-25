use std::time::Duration;

const DEFAULT_RETRY_INTERVAL: Duration = Duration::from_millis(100);
const DEFAULT_LOCK_TIMEOUT: Option<Duration> = None;
const DEFAULT_TTL: Duration = Duration::from_secs(10);
const DEFAULT_RENEW_INTERVAL: Option<Option<Duration>> = Some(None);

/// Options used when acquiring a lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcquireOptions {
    /// Time interval between each retry attempt when acquiring the lock.
    ///
    /// Default: **100 milliseconds**.
    pub retry_interval: Duration,
    /// Maximum duration to wait for the lock to be acquired.
    ///
    /// If `None`, it always waits for the lock.
    ///
    /// Default: `None`.
    pub lock_timeout:   Option<Duration>,
    /// Time-to-live (TTL) of the lock.
    ///
    /// This defines how long the lock will remain valid before expiring automatically.
    ///
    /// Default: **10 seconds**.
    pub ttl:            Duration,
    /// Time interval for automatic lock TTL renewal.
    ///
    /// * `Some(Some(duration))`: Renew using the given interval `duration`.
    /// * `Some(None)`: Renew at half of the TTL.
    /// * `None`: Disable TTL renewal entirely.
    ///
    /// Default: `Some(None)`.
    pub renew_interval: Option<Option<Duration>>,
}

impl Default for AcquireOptions {
    /// ```rust
    /// use std::time::Duration;
    ///
    /// let options = rlock::AcquireOptions::default();
    ///
    /// // equals to
    ///
    /// let options_2 = rlock::AcquireOptions {
    ///     retry_interval: Duration::from_millis(100),
    ///     lock_timeout:   None,
    ///     ttl:            Duration::from_secs(10),
    ///     renew_interval: Some(None),
    /// };
    ///
    /// # assert_eq!(options, options_2);
    /// ```
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl AcquireOptions {
    /// ```rust
    /// use std::time::Duration;
    ///
    /// let options = rlock::AcquireOptions::new();
    ///
    /// // equals to
    ///
    /// let options_2 = rlock::AcquireOptions {
    ///     retry_interval: Duration::from_millis(100),
    ///     lock_timeout:   None,
    ///     ttl:            Duration::from_secs(10),
    ///     renew_interval: Some(None),
    /// };
    ///
    /// # assert_eq!(options, options_2);
    /// ```
    #[inline]
    pub const fn new() -> Self {
        Self {
            retry_interval: DEFAULT_RETRY_INTERVAL,
            lock_timeout:   DEFAULT_LOCK_TIMEOUT,
            ttl:            DEFAULT_TTL,
            renew_interval: DEFAULT_RENEW_INTERVAL,
        }
    }

    /// Time interval between each retry attempt when acquiring the lock.
    #[inline]
    pub const fn retry_interval(mut self, duration: Duration) -> Self {
        self.retry_interval = duration;

        self
    }

    /// Maximum duration to wait for the lock to be acquired.
    ///
    /// If `None`, it always waits for the lock.
    #[inline]
    pub const fn lock_timeout(mut self, duration: Option<Duration>) -> Self {
        self.lock_timeout = duration;

        self
    }

    /// Time-to-live (TTL) of the lock.
    ///
    /// This defines how long the lock will remain valid before expiring automatically.
    #[inline]
    pub const fn ttl(mut self, duration: Duration) -> Self {
        self.ttl = duration;

        self
    }

    /// Time interval for automatic lock TTL renewal.
    ///
    /// * `Some(Some(duration))`: Renew using the given interval `duration`.
    /// * `Some(None)`: Renew at half of the TTL.
    /// * `None`: Disable TTL renewal entirely.
    #[inline]
    pub const fn renew_interval(mut self, duration: Option<Option<Duration>>) -> Self {
        self.renew_interval = duration;

        self
    }
}
