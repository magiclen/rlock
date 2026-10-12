use std::time::Duration;

use redis::{RedisResult, aio};

use crate::lock_request::{LockRequest, LockTarget};

pub(crate) async fn acquire_lock(
    conn: &mut impl aio::ConnectionLike,
    request: &LockRequest,
    ttl: Duration,
) -> RedisResult<bool> {
    if let LockTarget::Single(key) = &request.target {
        let result: Option<String> = redis::cmd("SET")
            .arg(key.as_str())
            .arg(request.uuid.as_str())
            .arg("NX")
            .arg("PX")
            .arg(ttl.as_millis())
            // we cannot use exec_async here because we use "NX" in order to know whether the key already exists
            .query_async(conn)
            .await?;

        return Ok(result.is_some());
    }

    request
        .target
        .acquire_script()
        .key(request.target.keys())
        .arg(request.uuid.as_str())
        .arg(ttl.as_millis())
        .invoke_async(conn)
        .await
}

pub(crate) async fn renew_lock(
    conn: &mut impl aio::ConnectionLike,
    request: &LockRequest,
    ttl: Duration,
) -> RedisResult<bool> {
    request
        .target
        .renew_script()
        .key(request.target.keys())
        .arg(request.uuid.as_str())
        .arg(ttl.as_millis())
        .invoke_async(conn)
        .await
}

pub(crate) async fn release_lock(
    conn: &mut impl aio::ConnectionLike,
    request: &LockRequest,
) -> RedisResult<bool> {
    request
        .target
        .release_script()
        .key(request.target.keys())
        .arg(request.uuid.as_str())
        .invoke_async(conn)
        .await
}
