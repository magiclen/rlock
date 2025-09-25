use std::{
    error::Error,
    fmt::{self, Display, Formatter},
};

use redis::RedisError;

/// Errors for handling lock acquisition failures.
#[derive(Debug)]
pub enum AcquireError {
    /// Error that occurred when interacting with Redis.
    RedisError(RedisError),
    /// Error indicating that acquiring a lock has timed out.
    LockTimeout,
    /// Error indicating that the `RLock` manager is shutting down.
    Shutdown,
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
        }
    }
}

impl Error for AcquireError {}

/// Errors for handling lock release failures.
#[derive(Debug)]
pub enum ReleaseError {
    /// Error that occurred when interacting with Redis.
    RedisError(RedisError),
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
        }
    }
}

impl Error for ReleaseError {}
