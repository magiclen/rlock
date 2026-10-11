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

#[inline]
pub(crate) async fn acquire_read_lock_async(
    conn: &mut impl aio::ConnectionLike,
    key: &str,
    uuid: &str,
    ttl: Duration,
) -> RedisResult<bool> {
    SCRIPT_ACQUIRE_READ_LOCK.key(key).arg(uuid).arg(ttl.as_millis()).invoke_async(conn).await
}

#[inline]
pub(crate) async fn renew_read_lock_async(
    conn: &mut impl aio::ConnectionLike,
    key: &str,
    uuid: &str,
    ttl: Duration,
) -> RedisResult<bool> {
    SCRIPT_RENEW_READ_LOCK.key(key).arg(uuid).arg(ttl.as_millis()).invoke_async(conn).await
}

#[inline]
pub(crate) async fn release_read_lock_async(
    conn: &mut impl aio::ConnectionLike,
    key: &str,
    uuid: &str,
) -> RedisResult<bool> {
    SCRIPT_RELEASE_READ_LOCK.key(key).arg(uuid).invoke_async(conn).await
}

#[inline]
pub(crate) async fn acquire_write_lock_async(
    conn: &mut impl aio::ConnectionLike,
    key: &str,
    uuid: &str,
    ttl: Duration,
) -> RedisResult<bool> {
    SCRIPT_ACQUIRE_WRITE_LOCK.key(key).arg(uuid).arg(ttl.as_millis()).invoke_async(conn).await
}

#[inline]
pub(crate) async fn renew_write_lock_async(
    conn: &mut impl aio::ConnectionLike,
    key: &str,
    uuid: &str,
    ttl: Duration,
) -> RedisResult<bool> {
    SCRIPT_RENEW_WRITE_LOCK.key(key).arg(uuid).arg(ttl.as_millis()).invoke_async(conn).await
}

#[inline]
pub(crate) async fn release_write_lock_async(
    conn: &mut impl aio::ConnectionLike,
    key: &str,
    uuid: &str,
) -> RedisResult<bool> {
    SCRIPT_RELEASE_WRITE_LOCK.key(key).arg(uuid).invoke_async(conn).await
}

#[inline]
pub(crate) async fn acquire_multi_key_read_lock_async(
    conn: &mut impl aio::ConnectionLike,
    keys: &[String],
    uuid: &str,
    ttl: Duration,
) -> RedisResult<bool> {
    SCRIPT_ACQUIRE_MULTI_KEY_READ_LOCK
        .key(keys)
        .arg(uuid)
        .arg(ttl.as_millis())
        .invoke_async(conn)
        .await
}

#[inline]
pub(crate) async fn renew_multi_key_read_lock_async(
    conn: &mut impl aio::ConnectionLike,
    keys: &[String],
    uuid: &str,
    ttl: Duration,
) -> RedisResult<bool> {
    SCRIPT_RENEW_MULTI_KEY_READ_LOCK
        .key(keys)
        .arg(uuid)
        .arg(ttl.as_millis())
        .invoke_async(conn)
        .await
}

#[inline]
pub(crate) async fn release_multi_key_read_lock_async(
    conn: &mut impl aio::ConnectionLike,
    keys: &[String],
    uuid: &str,
) -> RedisResult<bool> {
    SCRIPT_RELEASE_MULTI_KEY_READ_LOCK.key(keys).arg(uuid).invoke_async(conn).await
}

#[inline]
pub(crate) async fn acquire_multi_key_write_lock_async(
    conn: &mut impl aio::ConnectionLike,
    keys: &[String],
    uuid: &str,
    ttl: Duration,
) -> RedisResult<bool> {
    SCRIPT_ACQUIRE_MULTI_KEY_WRITE_LOCK
        .key(keys)
        .arg(uuid)
        .arg(ttl.as_millis())
        .invoke_async(conn)
        .await
}

#[inline]
pub(crate) async fn renew_multi_key_write_lock_async(
    conn: &mut impl aio::ConnectionLike,
    keys: &[String],
    uuid: &str,
    ttl: Duration,
) -> RedisResult<bool> {
    SCRIPT_RENEW_MULTI_KEY_WRITE_LOCK
        .key(keys)
        .arg(uuid)
        .arg(ttl.as_millis())
        .invoke_async(conn)
        .await
}

#[inline]
pub(crate) async fn release_multi_key_write_lock_async(
    conn: &mut impl aio::ConnectionLike,
    keys: &[String],
    uuid: &str,
) -> RedisResult<bool> {
    SCRIPT_RELEASE_MULTI_KEY_WRITE_LOCK.key(keys).arg(uuid).invoke_async(conn).await
}

pub(crate) async fn acquire_request(
    conn: &mut impl aio::ConnectionLike,
    request: &crate::release_request::ReleaseRequest,
    ttl: Duration,
) -> RedisResult<bool> {
    use crate::release_request::ReleaseRequestKey;

    let uuid = request.uuid.as_str();
    match &request.key {
        ReleaseRequestKey::Single(key) => {
            acquire_lock_async(conn, key, uuid, ttl).await.map(|value| value.is_some())
        },
        ReleaseRequestKey::Multiple(keys) => {
            acquire_multi_key_lock_async(conn, keys, uuid, ttl).await
        },
        ReleaseRequestKey::Read(key) => acquire_read_lock_async(conn, key, uuid, ttl).await,
        ReleaseRequestKey::Write(key) => acquire_write_lock_async(conn, key, uuid, ttl).await,
        ReleaseRequestKey::MultipleRead(keys) => {
            acquire_multi_key_read_lock_async(conn, keys, uuid, ttl).await
        },
        ReleaseRequestKey::MultipleWrite(keys) => {
            acquire_multi_key_write_lock_async(conn, keys, uuid, ttl).await
        },
    }
}

pub(crate) async fn renew_request(
    conn: &mut impl aio::ConnectionLike,
    request: &crate::release_request::ReleaseRequest,
    ttl: Duration,
) -> RedisResult<bool> {
    use crate::release_request::ReleaseRequestKey;

    let uuid = request.uuid.as_str();
    match &request.key {
        ReleaseRequestKey::Single(key) => renew_lock_async(conn, key, uuid, ttl).await,
        ReleaseRequestKey::Multiple(keys) => {
            renew_multi_key_lock_async(conn, keys, uuid, ttl).await
        },
        ReleaseRequestKey::Read(key) => renew_read_lock_async(conn, key, uuid, ttl).await,
        ReleaseRequestKey::Write(key) => renew_write_lock_async(conn, key, uuid, ttl).await,
        ReleaseRequestKey::MultipleRead(keys) => {
            renew_multi_key_read_lock_async(conn, keys, uuid, ttl).await
        },
        ReleaseRequestKey::MultipleWrite(keys) => {
            renew_multi_key_write_lock_async(conn, keys, uuid, ttl).await
        },
    }
}

pub(crate) async fn release_request(
    conn: &mut impl aio::ConnectionLike,
    request: &crate::release_request::ReleaseRequest,
) -> RedisResult<bool> {
    use crate::release_request::ReleaseRequestKey;

    let uuid = request.uuid.as_str();
    match &request.key {
        ReleaseRequestKey::Single(key) => release_lock_async(conn, key, uuid).await,
        ReleaseRequestKey::Multiple(keys) => release_multi_key_lock_async(conn, keys, uuid).await,
        ReleaseRequestKey::Read(key) => release_read_lock_async(conn, key, uuid).await,
        ReleaseRequestKey::Write(key) => release_write_lock_async(conn, key, uuid).await,
        ReleaseRequestKey::MultipleRead(keys) => {
            release_multi_key_read_lock_async(conn, keys, uuid).await
        },
        ReleaseRequestKey::MultipleWrite(keys) => {
            release_multi_key_write_lock_async(conn, keys, uuid).await
        },
    }
}
