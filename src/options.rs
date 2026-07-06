use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    time::Duration,
};

const DEFAULT_RETRY_INTERVAL: Duration = Duration::from_millis(100);
const DEFAULT_LOCK_TIMEOUT: Option<Duration> = None;
const DEFAULT_TTL: Duration = Duration::from_secs(10);
const DEFAULT_RENEW_INTERVAL: Option<Option<Duration>> = Some(None);

/// Options used when acquiring a lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcquireOptions {
    pub(crate) retry_interval: Duration,
    pub(crate) lock_timeout:   Option<Duration>,
    pub(crate) ttl:            Duration,
    pub(crate) renew_interval: Option<Option<Duration>>,
}

/// Errors for building acquisition options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcquireOptionsBuildError {
    /// The TTL is shorter than Redis can represent in milliseconds.
    TtlTooShort,
    /// The retry interval is zero.
    RetryIntervalIsZero,
    /// The renewal interval is shorter than Redis can represent in milliseconds.
    RenewIntervalTooShort,
}

impl Display for AcquireOptionsBuildError {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::TtlTooShort => f.write_str("ttl must be at least 1 millisecond"),
            Self::RetryIntervalIsZero => f.write_str("retry_interval must not be zero"),
            Self::RenewIntervalTooShort => {
                f.write_str("renew_interval must be at least 1 millisecond")
            },
        }
    }
}

impl Error for AcquireOptionsBuildError {}

/// Builder used to create validated acquisition options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcquireOptionsBuilder {
    retry_interval: Duration,
    lock_timeout:   Option<Duration>,
    ttl:            Duration,
    renew_interval: Option<Option<Duration>>,
}

impl Default for AcquireOptions {
    /// ```rust
    /// use std::time::Duration;
    ///
    /// let options = rlock::AcquireOptions::default();
    ///
    /// assert_eq!(Duration::from_millis(100), options.retry_interval());
    /// assert_eq!(None, options.lock_timeout());
    /// assert_eq!(Duration::from_secs(10), options.ttl());
    /// assert_eq!(Some(None), options.renew_interval());
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
    /// assert_eq!(Duration::from_millis(100), options.retry_interval());
    /// assert_eq!(None, options.lock_timeout());
    /// assert_eq!(Duration::from_secs(10), options.ttl());
    /// assert_eq!(Some(None), options.renew_interval());
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

    /// Creates a builder for acquisition options.
    #[inline]
    pub const fn builder() -> AcquireOptionsBuilder {
        AcquireOptionsBuilder::new()
    }

    /// Time interval between each retry attempt when acquiring the lock.
    #[inline]
    pub const fn retry_interval(&self) -> Duration {
        self.retry_interval
    }

    /// Maximum duration to wait for the lock to be acquired.
    ///
    /// If `None`, it always waits for the lock.
    #[inline]
    pub const fn lock_timeout(&self) -> Option<Duration> {
        self.lock_timeout
    }

    /// Time-to-live (TTL) of the lock.
    ///
    /// This defines how long the lock will remain valid before expiring automatically.
    #[inline]
    pub const fn ttl(&self) -> Duration {
        self.ttl
    }

    /// Time interval for automatic lock TTL renewal.
    ///
    /// * `Some(Some(duration))`: Renew using the given interval `duration`.
    /// * `Some(None)`: Renew at half of the TTL.
    /// * `None`: Disable TTL renewal entirely.
    #[inline]
    pub const fn renew_interval(&self) -> Option<Option<Duration>> {
        self.renew_interval
    }
}

impl Default for AcquireOptionsBuilder {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl AcquireOptionsBuilder {
    /// Creates a builder with the default acquisition options.
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

    /// Builds validated acquisition options.
    pub fn build(self) -> Result<AcquireOptions, AcquireOptionsBuildError> {
        if self.ttl.as_millis() == 0 {
            return Err(AcquireOptionsBuildError::TtlTooShort);
        }

        if self.retry_interval.is_zero() {
            return Err(AcquireOptionsBuildError::RetryIntervalIsZero);
        }

        if let Some(renew_interval) = self.renew_interval {
            let renew_interval = renew_interval.unwrap_or_else(|| self.ttl / 2);

            if renew_interval.as_millis() == 0 {
                return Err(AcquireOptionsBuildError::RenewIntervalTooShort);
            }
        }

        Ok(AcquireOptions {
            retry_interval: self.retry_interval,
            lock_timeout:   self.lock_timeout,
            ttl:            self.ttl,
            renew_interval: self.renew_interval,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_default() {
        assert_eq!(AcquireOptions::default(), AcquireOptions::builder().build().unwrap());
    }

    #[test]
    fn test_builder_custom_options() {
        let options = AcquireOptions::builder()
            .retry_interval(Duration::from_millis(50))
            .lock_timeout(Some(Duration::from_secs(1)))
            .ttl(Duration::from_secs(3))
            .renew_interval(Some(Some(Duration::from_secs(1))))
            .build()
            .unwrap();

        assert_eq!(Duration::from_millis(50), options.retry_interval());
        assert_eq!(Some(Duration::from_secs(1)), options.lock_timeout());
        assert_eq!(Duration::from_secs(3), options.ttl());
        assert_eq!(Some(Some(Duration::from_secs(1))), options.renew_interval());
    }
}
