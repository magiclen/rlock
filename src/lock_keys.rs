use std::{
    collections::HashSet,
    error::Error,
    fmt::{self, Display, Formatter},
    sync::Arc,
};

/// A validated, reusable collection of unique lock keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockKeys(Arc<Vec<String>>);

/// Errors for building a collection of lock keys.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LockKeysBuildError {
    /// The collection is empty.
    Empty,
    /// A key occurs more than once.
    DuplicateKey(String),
}

impl Display for LockKeysBuildError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("at least one lock key is required"),
            Self::DuplicateKey(key) => write!(f, "duplicate lock key: {key}"),
        }
    }
}

impl Error for LockKeysBuildError {}

fn validate(keys: &[String]) -> Result<(), LockKeysBuildError> {
    if keys.is_empty() {
        return Err(LockKeysBuildError::Empty);
    }

    let mut unique = HashSet::with_capacity(keys.len());
    for key in keys {
        if !unique.insert(key.as_str()) {
            return Err(LockKeysBuildError::DuplicateKey(key.clone()));
        }
    }

    Ok(())
}

/// Panics in debug builds if the keys are empty or contain duplicates.
#[inline]
pub(crate) fn debug_assert_lock_keys(keys: &[String]) {
    if cfg!(debug_assertions)
        && let Err(error) = validate(keys)
    {
        panic!("invalid lock keys: {error}");
    }
}

impl LockKeys {
    /// Builds a reusable collection from strings.
    ///
    /// ```rust
    /// let keys = rlock::LockKeys::new(["account:1", "account:2"]).unwrap();
    /// assert_eq!(2, keys.as_slice().len());
    /// ```
    pub fn new<I, S>(keys: I) -> Result<Self, LockKeysBuildError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>, {
        let keys: Vec<String> = keys.into_iter().map(Into::into).collect();

        validate(&keys)?;

        Ok(Self(Arc::new(keys)))
    }

    /// Returns the keys in their original order.
    #[inline]
    pub fn as_slice(&self) -> &[String] {
        &self.0
    }
}

impl From<LockKeys> for Arc<Vec<String>> {
    #[inline]
    fn from(keys: LockKeys) -> Self {
        keys.0
    }
}

impl From<&LockKeys> for Arc<Vec<String>> {
    #[inline]
    fn from(keys: &LockKeys) -> Self {
        keys.0.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reusable_keys() {
        let keys = LockKeys::new(["one", "two"]).unwrap();

        let first: Arc<Vec<String>> = (&keys).into();
        let second: Arc<Vec<String>> = keys.clone().into();

        assert_eq!(["one", "two"], keys.as_slice());
        assert!(Arc::ptr_eq(&first, &second));
    }
}
