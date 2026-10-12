use std::sync::Arc;

use redis::Script;

use crate::scripts::*;

#[derive(Debug, Clone)]
pub(crate) enum LockTarget {
    Single(Arc<String>),
    Multiple(Arc<Vec<String>>),
    Read(Arc<String>),
    Write(Arc<String>),
    MultipleRead(Arc<Vec<String>>),
    MultipleWrite(Arc<Vec<String>>),
}

impl LockTarget {
    /// Returns the Redis keys of the lock.
    pub(crate) fn keys(&self) -> &[String] {
        match self {
            Self::Single(key) | Self::Read(key) | Self::Write(key) => {
                std::slice::from_ref(key.as_ref())
            },
            Self::Multiple(keys) | Self::MultipleRead(keys) | Self::MultipleWrite(keys) => keys,
        }
    }

    /// Returns the acquisition script.
    /// A single-key mutex is usually acquired by `SET NX PX` instead, but this script also works for it.
    pub(crate) fn acquire_script(&self) -> &'static Script {
        match self {
            Self::Single(_) | Self::Multiple(_) => &SCRIPT_ACQUIRE_LOCK,
            Self::Read(_) | Self::MultipleRead(_) => &SCRIPT_ACQUIRE_READ_LOCK,
            Self::Write(_) | Self::MultipleWrite(_) => &SCRIPT_ACQUIRE_WRITE_LOCK,
        }
    }

    /// Returns the renewal script.
    pub(crate) fn renew_script(&self) -> &'static Script {
        match self {
            Self::Single(_) | Self::Multiple(_) => &SCRIPT_RENEW_LOCK,
            Self::Read(_) | Self::MultipleRead(_) => &SCRIPT_RENEW_READ_LOCK,
            Self::Write(_) | Self::MultipleWrite(_) => &SCRIPT_RENEW_WRITE_LOCK,
        }
    }

    /// Returns the release script.
    pub(crate) fn release_script(&self) -> &'static Script {
        match self {
            Self::Single(_) | Self::Multiple(_) => &SCRIPT_RELEASE_LOCK,
            Self::Read(_) | Self::MultipleRead(_) => &SCRIPT_RELEASE_READ_LOCK,
            Self::Write(_) | Self::MultipleWrite(_) => &SCRIPT_RELEASE_WRITE_LOCK,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LockRequest {
    pub(crate) target: LockTarget,
    pub(crate) uuid:   Arc<String>,
}
