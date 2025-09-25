/// Builds a Redis key by concatenating a prefix and a key with `:` as a separator.
///
/// The resulting format is `<prefix>:<key>`.  
///
/// # Examples
///
/// ```rust
/// let key = rlock::build_redis_key_with_prefix("lock", "123");
///
/// assert_eq!("lock:123", key);
/// ```
#[inline]
pub fn build_redis_key_with_prefix(prefix: impl AsRef<str>, key: impl AsRef<str>) -> String {
    let prefix = prefix.as_ref();
    let key = key.as_ref();

    format!("{prefix}:{key}")
}

/// Builds a Redis key by joining multiple parts with `:` as a separator.
///
/// The resulting format is `<key_part_1>:<key_part_2>:...`.  
///
/// # Examples
///
/// ```rust
/// let key = rlock::build_redis_key_from_parts(["account", "123"]);
///
/// assert_eq!("account:123", key);
/// ```
pub fn build_redis_key_from_parts<S>(key_parts: impl IntoIterator<Item = S>) -> String
where
    S: AsRef<str>, {
    let mut s = String::new();

    for part in key_parts.into_iter() {
        s.push_str(part.as_ref());
        s.push(':');
    }

    let len = s.len();

    if len > 0 {
        unsafe {
            s.as_mut_vec().set_len(len - 1);
        }
    }

    s
}

/// Builds a Redis key by combining a prefix with multiple parts, using `:` as a separator.
///
/// The resulting format is `<prefix>:<key_part_1>:<key_part_2>:...`.  
///
/// # Examples
///
/// ```rust
/// let key = rlock::build_redis_key_from_parts_with_prefix("lock", [
///     "account", "123",
/// ]);
///
/// assert_eq!("lock:account:123", key);
/// ```
pub fn build_redis_key_from_parts_with_prefix<S>(
    prefix: impl AsRef<str>,
    key_parts: impl IntoIterator<Item = S>,
) -> String
where
    S: AsRef<str>, {
    let mut s = format!("{}:", prefix.as_ref());

    for part in key_parts.into_iter() {
        s.push_str(part.as_ref());
        s.push(':');
    }

    let len = s.len();

    unsafe {
        s.as_mut_vec().set_len(len - 1);
    }

    s
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_build_redis_key_with_prefix() {
        assert_eq!("foo:bar:baz", build_redis_key_with_prefix("foo", "bar:baz"))
    }

    #[test]
    fn test_build_redis_key_from_parts() {
        assert_eq!("foo:bar:baz", build_redis_key_from_parts(["foo", "bar", "baz"]))
    }

    #[test]
    fn test_build_redis_key_from_parts_with_prefix() {
        assert_eq!("foo:bar:baz", build_redis_key_from_parts_with_prefix("foo", ["bar", "baz"]))
    }
}
