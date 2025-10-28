use std::time::Duration;

use redis::{RedisResult, aio};

use crate::scripts::*;

#[inline]
pub(crate) async fn acquire_lock_async(
    conn: &mut impl aio::ConnectionLike,
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
        // we cannot use exec_async here because we use "NX" in order to know whether the key already exists
        .query_async(conn)
        .await
}

#[inline]
pub(crate) async fn acquire_multi_key_lock_async(
    conn: &mut impl aio::ConnectionLike,
    keys: &[String],
    uuid: &str,
    ttl: Duration,
) -> RedisResult<bool> {
    SCRIPT_ACQUIRE_MULTI_KEY_LOCK.key(keys).arg(uuid).arg(ttl.as_millis()).invoke_async(conn).await
}

#[inline]
pub(crate) async fn renew_lock_async(
    conn: &mut impl aio::ConnectionLike,
    key: &str,
    uuid: &str,
    ttl: Duration,
) -> RedisResult<bool> {
    SCRIPT_RENEW_LOCK.key(key).arg(uuid).arg(ttl.as_millis()).invoke_async(conn).await
}

#[inline]
pub(crate) async fn renew_multi_key_lock_async(
    conn: &mut impl aio::ConnectionLike,
    keys: &[String],
    uuid: &str,
    ttl: Duration,
) -> RedisResult<bool> {
    SCRIPT_RENEW_MULTI_KEY_LOCK.key(keys).arg(uuid).arg(ttl.as_millis()).invoke_async(conn).await
}

#[inline]
pub(crate) async fn release_lock_async(
    conn: &mut impl aio::ConnectionLike,
    key: &str,
    uuid: &str,
) -> RedisResult<bool> {
    SCRIPT_RELEASE_LOCK.key(key).arg(uuid).invoke_async(conn).await
}

#[inline]
pub(crate) async fn release_multi_key_lock_async(
    conn: &mut impl aio::ConnectionLike,
    keys: &[String],
    uuid: &str,
) -> RedisResult<bool> {
    SCRIPT_RELEASE_MULTI_KEY_LOCK.key(keys).arg(uuid).invoke_async(conn).await
}
