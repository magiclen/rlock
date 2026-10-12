use std::time::Duration;

use redis::RedisResult;

use super::pool::ConnectionPool;
use crate::lock_request::{LockRequest, LockTarget};

pub(crate) fn acquire_lock(
    pool: &ConnectionPool,
    request: &LockRequest,
    ttl: Duration,
) -> RedisResult<bool> {
    pool.with_connection(|conn| {
        if let LockTarget::Single(key) = &request.target {
            let result: Option<String> = redis::cmd("SET")
                .arg(key.as_str())
                .arg(request.uuid.as_str())
                .arg("NX")
                .arg("PX")
                .arg(ttl.as_millis())
                // we cannot use exec here because we use "NX" in order to know whether the key already exists
                .query(conn)?;

            return Ok(result.is_some());
        }

        request
            .target
            .acquire_script()
            .key(request.target.keys())
            .arg(request.uuid.as_str())
            .arg(ttl.as_millis())
            .invoke(conn)
    })
}

pub(crate) fn renew_lock(
    pool: &ConnectionPool,
    request: &LockRequest,
    ttl: Duration,
) -> RedisResult<bool> {
    pool.with_connection(|conn| {
        request
            .target
            .renew_script()
            .key(request.target.keys())
            .arg(request.uuid.as_str())
            .arg(ttl.as_millis())
            .invoke(conn)
    })
}

pub(crate) fn release_lock(pool: &ConnectionPool, request: &LockRequest) -> RedisResult<bool> {
    pool.with_connection(|conn| {
        request
            .target
            .release_script()
            .key(request.target.keys())
            .arg(request.uuid.as_str())
            .invoke(conn)
    })
}
