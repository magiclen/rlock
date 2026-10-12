use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    io,
};

use redis::RedisError;

/// Errors for handling lock acquisition failures.
#[derive(Debug)]
#[non_exhaustive]
pub enum AcquireError {
    /// Error that occurred when interacting with Redis.
    RedisError(RedisError),
    /// Error indicating that acquiring a lock has timed out.
    LockTimeout,
    /// Error indicating that the `RLock` manager is shutting down.
    Shutdown,
    /// The lease expired before the acquired lock could be returned.
    LeaseExpired,
    /// A lock worker could not start or return a result.
    WorkerError(io::Error),
}

impl From<RedisError> for AcquireError {
    #[inline]
    fn from(value: RedisError) -> Self {
        Self::RedisError(value)
    }
}

impl Display for AcquireError {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::RedisError(error) => Display::fmt(error, f),
            Self::LockTimeout => {
                f.write_str("attempted to get a lock but the provided timeout was exceeded")
            },
            Self::Shutdown => f.write_str("the RLock manager is shutting down"),
            Self::LeaseExpired => {
                f.write_str("the lock lease expired before acquisition completed")
            },
            Self::WorkerError(error) => Display::fmt(error, f),
        }
    }
}

impl Error for AcquireError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        // the wrapped error is displayed directly, so its source is the next cause
        match self {
            Self::RedisError(error) => error.source(),
            Self::WorkerError(error) => error.source(),
            _ => None,
        }
    }
}

/// Errors for handling lock release failures.
#[derive(Debug)]
#[non_exhaustive]
pub enum ReleaseError {
    /// Error that occurred when interacting with Redis.
    RedisError(RedisError),
    /// A lock worker stopped before returning a release result.
    WorkerError(io::Error),
}

impl From<RedisError> for ReleaseError {
    #[inline]
    fn from(value: RedisError) -> Self {
        Self::RedisError(value)
    }
}

impl Display for ReleaseError {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::RedisError(error) => Display::fmt(error, f),
            Self::WorkerError(error) => Display::fmt(error, f),
        }
    }
}

impl Error for ReleaseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        // the wrapped error is displayed directly, so its source is the next cause
        match self {
            Self::RedisError(error) => error.source(),
            Self::WorkerError(error) => error.source(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn messages(error: &dyn Error) -> Vec<String> {
        let mut messages = vec![error.to_string()];
        let mut source = error.source();

        while let Some(error) = source {
            messages.push(error.to_string());
            source = error.source();
        }

        messages
    }

    #[test]
    fn test_error_chain() {
        let error = AcquireError::WorkerError(io::Error::other("the lock worker stopped"));

        assert_eq!(["the lock worker stopped"], messages(&error).as_slice());

        let error = ReleaseError::WorkerError(io::Error::other("the lock worker stopped"));

        assert_eq!(["the lock worker stopped"], messages(&error).as_slice());
    }
}
