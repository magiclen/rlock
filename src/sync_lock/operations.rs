use std::time::Duration;

use redis::{ConnectionLike, RedisResult};

use crate::scripts::*;

#[inline]
pub(crate) fn acquire_lock_sync(
    conn: &mut impl ConnectionLike,
    key: &str,
    uuid: &str,
    ttl: Duration,
) -> RedisResult<Option<String>> {
    redis::cmd("SET")
        .arg(key)
        .arg(uuid)
        .arg("NX")
        .arg("PX")
        .arg(ttl.as_millis())
        // we cannot use exec here because we use "NX" in order to know whether the key already exists
        .query(conn)
}

#[inline]
pub(crate) fn acquire_multi_key_lock_sync(
    conn: &mut impl ConnectionLike,
    keys: &[String],
    uuid: &str,
    ttl: Duration,
) -> RedisResult<bool> {
    SCRIPT_ACQUIRE_MULTI_KEY_LOCK.key(keys).arg(uuid).arg(ttl.as_millis()).invoke(conn)
}

#[inline]
pub(crate) fn renew_lock_sync(
    conn: &mut impl ConnectionLike,
    key: &str,
    uuid: &str,
    ttl: Duration,
) -> RedisResult<bool> {
    SCRIPT_RENEW_LOCK.key(key).arg(uuid).arg(ttl.as_millis()).invoke(conn)
}

#[inline]
pub(crate) fn renew_multi_key_lock_sync(
    conn: &mut impl ConnectionLike,
    keys: &[String],
    uuid: &str,
    ttl: Duration,
) -> RedisResult<bool> {
    SCRIPT_RENEW_MULTI_KEY_LOCK.key(keys).arg(uuid).arg(ttl.as_millis()).invoke(conn)
}

#[inline]
pub(crate) fn release_lock_sync(
    conn: &mut impl ConnectionLike,
    key: &str,
    uuid: &str,
) -> RedisResult<bool> {
    SCRIPT_RELEASE_LOCK.key(key).arg(uuid).invoke(conn)
}

#[inline]
pub(crate) fn release_multi_key_lock_sync(
    conn: &mut impl ConnectionLike,
    keys: &[String],
    uuid: &str,
) -> RedisResult<bool> {
    SCRIPT_RELEASE_MULTI_KEY_LOCK.key(keys).arg(uuid).invoke(conn)
}
