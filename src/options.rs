use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    time::Duration,
};

const DEFAULT_RETRY_INTERVAL: Duration = Duration::from_millis(100);
const DEFAULT_LOCK_TIMEOUT: Option<Duration> = None;
const DEFAULT_TTL: Duration = Duration::from_secs(10);
const DEFAULT_RENEW_INTERVAL: Option<Option<Duration>> = Some(None);
pub(crate) const MAX_TTL_MILLIS: u64 = (1 << 53) - 1;

/// Controls automatic lock renewal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenewalMode {
    /// Do not renew the lease.
    Disabled,
    /// Renew at half of the effective TTL.
    Auto,
    /// Renew using the given interval.
    Interval(Duration),
}

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
#[non_exhaustive]
pub enum AcquireOptionsBuildError {
    /// The TTL is shorter than Redis can represent in milliseconds.
    TtlTooShort,
    /// The TTL is too large to represent exactly in Lua.
    TtlTooLong,
    /// The retry interval is zero.
    RetryIntervalIsZero,
    /// The renewal interval is shorter than Redis can represent in milliseconds.
    RenewIntervalTooShort,
    /// The renewal interval is not shorter than the effective TTL.
    RenewIntervalNotLessThanTtl,
}

impl Display for AcquireOptionsBuildError {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::TtlTooShort => f.write_str("ttl must be at least 1 millisecond"),
            Self::TtlTooLong => f.write_str("ttl exceeds the exact integer range of Lua"),
            Self::RetryIntervalIsZero => f.write_str("retry_interval must not be zero"),
            Self::RenewIntervalTooShort => {
                f.write_str("renew_interval must be at least 1 millisecond")
            },
            Self::RenewIntervalNotLessThanTtl => {
                f.write_str("renew_interval must be shorter than the effective ttl")
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

    /// Maximum duration for acquisition, including retries and Redis IO.
    ///
    /// If `None`, acquisition has no deadline; a zero duration returns a timeout without sending a command.
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

    /// Returns the automatic renewal mode.
    #[inline]
    pub const fn renewal_mode(&self) -> RenewalMode {
        match self.renew_interval {
            None => RenewalMode::Disabled,
            Some(None) => RenewalMode::Auto,
            Some(Some(interval)) => RenewalMode::Interval(interval),
        }
    }

    #[inline]
    pub(crate) fn effective_ttl(&self) -> Duration {
        Duration::from_millis(self.ttl.as_millis() as u64)
    }

    #[inline]
    pub(crate) fn effective_renew_interval(&self) -> Option<Duration> {
        self.renew_interval.map(|interval| interval.unwrap_or_else(|| self.effective_ttl() / 2))
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

    /// Maximum duration for acquisition, including retries and Redis IO.
    ///
    /// If `None`, acquisition has no deadline; a zero duration returns a timeout without sending a command.
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

    /// Sets the automatic renewal mode.
    #[inline]
    pub const fn renewal_mode(mut self, mode: RenewalMode) -> Self {
        self.renew_interval = match mode {
            RenewalMode::Disabled => None,
            RenewalMode::Auto => Some(None),
            RenewalMode::Interval(interval) => Some(Some(interval)),
        };

        self
    }

    /// Builds validated acquisition options.
    pub fn build(self) -> Result<AcquireOptions, AcquireOptionsBuildError> {
        if self.ttl.as_millis() == 0 {
            return Err(AcquireOptionsBuildError::TtlTooShort);
        }

        if self.ttl.as_millis() > u128::from(MAX_TTL_MILLIS) {
            return Err(AcquireOptionsBuildError::TtlTooLong);
        }

        let effective_ttl = Duration::from_millis(self.ttl.as_millis() as u64);

        if self.retry_interval.is_zero() {
            return Err(AcquireOptionsBuildError::RetryIntervalIsZero);
        }

        if let Some(renew_interval) = self.renew_interval {
            let renew_interval = renew_interval.unwrap_or_else(|| effective_ttl / 2);

            if renew_interval.as_millis() == 0 {
                return Err(AcquireOptionsBuildError::RenewIntervalTooShort);
            }

            if renew_interval >= effective_ttl {
                return Err(AcquireOptionsBuildError::RenewIntervalNotLessThanTtl);
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

    #[test]
    fn test_renewal_modes() {
        for (mode, interval) in [
            (RenewalMode::Disabled, None),
            (RenewalMode::Auto, Some(None)),
            (RenewalMode::Interval(Duration::from_secs(1)), Some(Some(Duration::from_secs(1)))),
        ] {
            let options = AcquireOptions::builder().renewal_mode(mode).build().unwrap();
            assert_eq!(mode, options.renewal_mode());
            assert_eq!(interval, options.renew_interval());
        }
    }

    #[test]
    fn test_lease_limits() {
        assert_eq!(
            Err(AcquireOptionsBuildError::TtlTooLong),
            AcquireOptions::builder().ttl(Duration::MAX).build()
        );
        assert_eq!(
            Err(AcquireOptionsBuildError::RenewIntervalNotLessThanTtl),
            AcquireOptions::builder()
                .ttl(Duration::from_micros(2900))
                .renewal_mode(RenewalMode::Interval(Duration::from_micros(2500)))
                .build()
        );
        assert!(
            AcquireOptions::builder().ttl(Duration::from_millis(MAX_TTL_MILLIS)).build().is_ok()
        );
        assert!(
            AcquireOptions::builder()
                .ttl(Duration::from_millis(1))
                .renewal_mode(RenewalMode::Disabled)
                .build()
                .is_ok()
        );
    }
}
