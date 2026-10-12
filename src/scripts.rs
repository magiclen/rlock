use std::sync::LazyLock;

use redis::Script;

// ARGV[1] is the owner UUID and ARGV[2] is the TTL in milliseconds.
// Validate every numeric argument before any script can mutate a key.
const TTL_PREFLIGHT: &str = r#"
    local raw_ttl = ARGV[2]

    if not raw_ttl or not string.match(raw_ttl, "^[1-9][0-9]*$") then
        return redis.error_reply("ERR rlock TTL must be a positive integer in milliseconds")
    end

    local ttl = tonumber(raw_ttl)

    if not ttl or ttl > 9007199254740991 then
        return redis.error_reply("ERR rlock TTL exceeds the exact integer range")
    end
"#;

// It uses ttl from TTL_PREFLIGHT and defines now and expiry.
// Server time followed by writes requires effect replication, available since Redis 5.0.
const RW_EXPIRY_PREFLIGHT: &str = r#"
    local time = redis.call("TIME")
    local now = tonumber(time[1]) * 1000 + math.floor(tonumber(time[2]) / 1000)

    if now < 0 or now > 9007199254740991 or ttl > 9007199254740991 - now then
        return redis.error_reply("ERR rlock expiry exceeds the exact integer range")
    end

    local expiry = now + ttl
"#;

// A read-write lock uses one sorted set with positive reader expiries and negative writer expiries.
// Members include the owner UUID so release cannot remove another owner's lease.
const RW_TYPE_PREFLIGHT: &str = r#"
    for i = 1, #KEYS do
        local kind = redis.call("TYPE", KEYS[i]).ok

        if kind ~= "none" and kind ~= "zset" then
            return redis.error_reply("WRONGTYPE rlock read-write locks require sorted set keys")
        end
    end
"#;

fn script_with_ttl_preflight(body: &str) -> Script {
    Script::new(&format!("{TTL_PREFLIGHT}\n{body}"))
}

fn rw_script_with_preflight(body: &str) -> Script {
    Script::new(&format!("{TTL_PREFLIGHT}\n{RW_EXPIRY_PREFLIGHT}\n{RW_TYPE_PREFLIGHT}\n{body}"))
}

pub(crate) static SCRIPT_ACQUIRE_LOCK: LazyLock<Script> = LazyLock::new(|| {
    // KEYS    = the mutex keys to set
    // ARGV[1] = token (the UUID of the lock owner)
    // ARGV[2] = TTL (milliseconds)
    // returns 1 if every key is set, or 0 if any key already exists
    script_with_ttl_preflight(
        r#"
            for i = 1, #KEYS do
                if redis.call("EXISTS", KEYS[i]) ~= 0 then
                    return 0
                end
            end

            for i = 1, #KEYS do
                redis.call("SET", KEYS[i], ARGV[1], "PX", raw_ttl)
            end

            return 1
        "#,
    )
});

pub(crate) static SCRIPT_RENEW_LOCK: LazyLock<Script> = LazyLock::new(|| {
    // KEYS    = the mutex keys to renew
    // ARGV[1] = token (the UUID of the lock owner)
    // ARGV[2] = new TTL (milliseconds)
    // returns 1 if every key is renewed, or 0 if any key is not owned by the token
    script_with_ttl_preflight(
        r#"
            for i = 1, #KEYS do
                if redis.call("GET", KEYS[i]) ~= ARGV[1] then
                    return 0
                end
            end

            for i = 1, #KEYS do
                redis.call("PEXPIRE", KEYS[i], raw_ttl)
            end

            return 1
        "#,
    )
});

pub(crate) static SCRIPT_RELEASE_LOCK: LazyLock<Script> = LazyLock::new(|| {
    // KEYS    = the mutex keys to release
    // ARGV[1] = token (the UUID of the lock owner)
    // every owned key is deleted, and the script returns 1 only if all keys were owned by the token
    Script::new(
        r#"
            local count = 0

            for i = 1, #KEYS do
                local kind = redis.call("TYPE", KEYS[i]).ok

                if kind == "string" and redis.call("GET", KEYS[i]) == ARGV[1] then
                    count = count + redis.call("DEL", KEYS[i])
                end
            end

            if count == #KEYS then
                return 1
            else
                return 0
            end
        "#,
    )
});

pub(crate) static SCRIPT_ACQUIRE_READ_LOCK: LazyLock<Script> = LazyLock::new(|| {
    // KEYS    = the read-write lock keys (sorted sets) to acquire
    // ARGV[1] = token (the UUID of the reader)
    // ARGV[2] = TTL (milliseconds)
    // expired holders are removed first, and the script returns 1 if the reader is added to every key, or 0 if any key has a live writer
    rw_script_with_preflight(
        r#"
            for i = 1, #KEYS do
                redis.call("ZREMRANGEBYSCORE", KEYS[i], -now, now)

                if redis.call("ZCOUNT", KEYS[i], "-inf", "(0") ~= 0 then
                    return 0
                end
            end

            local member = "r:" .. ARGV[1]

            for i = 1, #KEYS do
                redis.call("ZADD", KEYS[i], expiry, member)

                if redis.call("PTTL", KEYS[i]) < ttl then
                    redis.call("PEXPIRE", KEYS[i], raw_ttl)
                end
            end

            return 1
        "#,
    )
});

pub(crate) static SCRIPT_RENEW_READ_LOCK: LazyLock<Script> = LazyLock::new(|| {
    // KEYS    = the read-write lock keys (sorted sets) to renew
    // ARGV[1] = token (the UUID of the reader)
    // ARGV[2] = new TTL (milliseconds)
    // returns 1 if the reader is renewed in every key, or 0 if the reader is missing or expired in any key
    rw_script_with_preflight(
        r#"
            local member = "r:" .. ARGV[1]

            for i = 1, #KEYS do
                redis.call("ZREMRANGEBYSCORE", KEYS[i], -now, now)

                local score = redis.call("ZSCORE", KEYS[i], member)

                if not score or tonumber(score) <= now then
                    return 0
                end
            end

            for i = 1, #KEYS do
                redis.call("ZADD", KEYS[i], expiry, member)

                if redis.call("PTTL", KEYS[i]) < ttl then
                    redis.call("PEXPIRE", KEYS[i], raw_ttl)
                end
            end

            return 1
        "#,
    )
});

pub(crate) static SCRIPT_RELEASE_READ_LOCK: LazyLock<Script> = LazyLock::new(|| {
    // KEYS    = the read-write lock keys (sorted sets) to release
    // ARGV[1] = token (the UUID of the reader)
    // the reader is removed from every key, and the script returns 1 only if all keys contained it
    Script::new(
        r#"
            local member = "r:" .. ARGV[1]
            local count = 0

            for i = 1, #KEYS do
                if redis.call("TYPE", KEYS[i]).ok == "zset" then
                    count = count + redis.call("ZREM", KEYS[i], member)
                end
            end

            if count == #KEYS then
                return 1
            else
                return 0
            end
        "#,
    )
});

pub(crate) static SCRIPT_ACQUIRE_WRITE_LOCK: LazyLock<Script> = LazyLock::new(|| {
    // KEYS    = the read-write lock keys (sorted sets) to acquire
    // ARGV[1] = token (the UUID of the writer)
    // ARGV[2] = TTL (milliseconds)
    // expired holders are removed first, and the script returns 1 if the writer is added to every key, or 0 if any key has a live holder
    rw_script_with_preflight(
        r#"
            for i = 1, #KEYS do
                redis.call("ZREMRANGEBYSCORE", KEYS[i], -now, now)

                if redis.call("ZCARD", KEYS[i]) ~= 0 then
                    return 0
                end
            end

            local member = "w:" .. ARGV[1]

            for i = 1, #KEYS do
                redis.call("ZADD", KEYS[i], -expiry, member)
                redis.call("PEXPIRE", KEYS[i], raw_ttl)
            end

            return 1
        "#,
    )
});

pub(crate) static SCRIPT_RENEW_WRITE_LOCK: LazyLock<Script> = LazyLock::new(|| {
    // KEYS    = the read-write lock keys (sorted sets) to renew
    // ARGV[1] = token (the UUID of the writer)
    // ARGV[2] = new TTL (milliseconds)
    // returns 1 if the writer is renewed in every key, or 0 if the writer is missing or expired in any key
    rw_script_with_preflight(
        r#"
            local member = "w:" .. ARGV[1]

            for i = 1, #KEYS do
                redis.call("ZREMRANGEBYSCORE", KEYS[i], -now, now)

                local score = redis.call("ZSCORE", KEYS[i], member)

                if not score or tonumber(score) >= -now then
                    return 0
                end
            end

            for i = 1, #KEYS do
                redis.call("ZADD", KEYS[i], -expiry, member)
                redis.call("PEXPIRE", KEYS[i], raw_ttl)
            end

            return 1
        "#,
    )
});

pub(crate) static SCRIPT_RELEASE_WRITE_LOCK: LazyLock<Script> = LazyLock::new(|| {
    // KEYS    = the read-write lock keys (sorted sets) to release
    // ARGV[1] = token (the UUID of the writer)
    // the writer is removed from every key, and the script returns 1 only if all keys contained it
    Script::new(
        r#"
            local member = "w:" .. ARGV[1]
            local count = 0

            for i = 1, #KEYS do
                if redis.call("TYPE", KEYS[i]).ok == "zset" then
                    count = count + redis.call("ZREM", KEYS[i], member)
                end
            end

            if count == #KEYS then
                return 1
            else
                return 0
            end
        "#,
    )
});

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn connection() -> redis::Connection {
        redis::Client::open(
            std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379/0".to_string()),
        )
        .unwrap()
        .get_connection()
        .unwrap()
    }

    #[test]
    fn test_ttl_preflight() {
        let mut conn = connection();

        for (script, name) in [
            (&*SCRIPT_ACQUIRE_LOCK, "mutex"),
            (&*SCRIPT_ACQUIRE_READ_LOCK, "read"),
            (&*SCRIPT_ACQUIRE_WRITE_LOCK, "write"),
        ] {
            let keys = [format!("test:script_ttl:{name}:a"), format!("test:script_ttl:{name}:b")];

            for ttl in ["0", "-1", "1.5", "9007199254740992"] {
                let result: redis::RedisResult<bool> =
                    script.key(&keys).arg("owner").arg(ttl).invoke(&mut conn);

                assert!(result.is_err());
                assert_eq!(0, redis::cmd("EXISTS").arg(&keys).query::<usize>(&mut conn).unwrap());
            }
        }
    }

    #[test]
    fn test_reader_lease_and_cleanup() {
        let mut conn = connection();
        let key = "test:script_reader_cleanup";

        assert!(
            SCRIPT_ACQUIRE_READ_LOCK
                .key(key)
                .arg("short")
                .arg(80)
                .invoke::<bool>(&mut conn)
                .unwrap()
        );
        assert!(
            SCRIPT_ACQUIRE_READ_LOCK
                .key(key)
                .arg("long")
                .arg(2000)
                .invoke::<bool>(&mut conn)
                .unwrap()
        );

        std::thread::sleep(Duration::from_millis(120));

        assert!(
            !SCRIPT_RENEW_READ_LOCK
                .key(key)
                .arg("short")
                .arg(80)
                .invoke::<bool>(&mut conn)
                .unwrap()
        );

        redis::cmd("ZADD").arg(key).arg(1).arg("r:stale").exec(&mut conn).unwrap();

        assert!(
            SCRIPT_RENEW_READ_LOCK
                .key(key)
                .arg("long")
                .arg(2000)
                .invoke::<bool>(&mut conn)
                .unwrap()
        );
        assert_eq!(1, redis::cmd("ZCARD").arg(key).query::<usize>(&mut conn).unwrap());
        assert!(SCRIPT_RELEASE_READ_LOCK.key(key).arg("long").invoke::<bool>(&mut conn).unwrap());
    }

    #[test]
    fn test_multi_key_contention_and_partial_release() {
        let mut conn = connection();
        let keys = ["test:script_multi:a", "test:script_multi:b"];

        assert!(
            SCRIPT_ACQUIRE_WRITE_LOCK
                .key(keys[1])
                .arg("other")
                .arg(2000)
                .invoke::<bool>(&mut conn)
                .unwrap()
        );

        for script in [&*SCRIPT_ACQUIRE_READ_LOCK, &*SCRIPT_ACQUIRE_WRITE_LOCK] {
            assert!(!script.key(&keys).arg("owner").arg(2000).invoke::<bool>(&mut conn).unwrap());
            assert_eq!(0, redis::cmd("EXISTS").arg(keys[0]).query::<usize>(&mut conn).unwrap());
        }

        assert!(
            SCRIPT_RELEASE_WRITE_LOCK.key(keys[1]).arg("other").invoke::<bool>(&mut conn).unwrap()
        );

        for (acquire, renew, release) in [
            (&*SCRIPT_ACQUIRE_READ_LOCK, &*SCRIPT_RENEW_READ_LOCK, &*SCRIPT_RELEASE_READ_LOCK),
            (&*SCRIPT_ACQUIRE_WRITE_LOCK, &*SCRIPT_RENEW_WRITE_LOCK, &*SCRIPT_RELEASE_WRITE_LOCK),
        ] {
            assert!(acquire.key(&keys).arg("owner").arg(2000).invoke::<bool>(&mut conn).unwrap());
            assert!(renew.key(&keys).arg("owner").arg(2000).invoke::<bool>(&mut conn).unwrap());

            redis::cmd("DEL").arg(keys[1]).exec(&mut conn).unwrap();

            assert!(
                SCRIPT_ACQUIRE_WRITE_LOCK
                    .key(keys[1])
                    .arg("other")
                    .arg(2000)
                    .invoke::<bool>(&mut conn)
                    .unwrap()
            );

            let before = redis::cmd("PTTL").arg(keys[0]).query::<i64>(&mut conn).unwrap();

            assert!(!renew.key(&keys).arg("owner").arg(4000).invoke::<bool>(&mut conn).unwrap());
            assert!(redis::cmd("PTTL").arg(keys[0]).query::<i64>(&mut conn).unwrap() <= before);
            assert!(!release.key(&keys).arg("owner").invoke::<bool>(&mut conn).unwrap());
            assert_eq!(0, redis::cmd("EXISTS").arg(keys[0]).query::<usize>(&mut conn).unwrap());
            assert!(
                SCRIPT_RELEASE_WRITE_LOCK
                    .key(keys[1])
                    .arg("other")
                    .invoke::<bool>(&mut conn)
                    .unwrap()
            );
        }
    }

    #[test]
    fn test_mutex_partial_release() {
        let mut conn = connection();
        let keys = ["test:script_mutex_release:a", "test:script_mutex_release:b"];

        assert!(
            SCRIPT_ACQUIRE_LOCK
                .key(&keys)
                .arg("owner")
                .arg(2000)
                .invoke::<bool>(&mut conn)
                .unwrap()
        );

        redis::cmd("SET").arg(keys[1]).arg("other").arg("PX").arg(2000).exec(&mut conn).unwrap();

        assert!(!SCRIPT_RELEASE_LOCK.key(&keys).arg("owner").invoke::<bool>(&mut conn).unwrap());
        assert_eq!(0, redis::cmd("EXISTS").arg(keys[0]).query::<usize>(&mut conn).unwrap());
        assert_eq!("other", redis::cmd("GET").arg(keys[1]).query::<String>(&mut conn).unwrap());

        redis::cmd("DEL").arg(keys[1]).exec(&mut conn).unwrap();
    }

    #[test]
    fn test_type_preflight_before_cleanup() {
        let mut conn = connection();
        let keys = ["test:script_type:a", "test:script_type:b"];

        redis::cmd("ZADD").arg(keys[0]).arg(1).arg("r:stale").exec(&mut conn).unwrap();
        redis::cmd("SET").arg(keys[1]).arg("other").exec(&mut conn).unwrap();

        let result: redis::RedisResult<bool> =
            SCRIPT_ACQUIRE_READ_LOCK.key(&keys).arg("owner").arg(2000).invoke(&mut conn);

        assert!(result.is_err());
        assert_eq!(1, redis::cmd("ZCARD").arg(keys[0]).query::<usize>(&mut conn).unwrap());

        redis::cmd("DEL").arg(&keys).exec(&mut conn).unwrap();
    }
}
