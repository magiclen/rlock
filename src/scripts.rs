use std::sync::LazyLock;

use redis::Script;

pub(crate) static SCRIPT_ACQUIRE_MULTI_KEY_LOCK: LazyLock<Script> = LazyLock::new(|| {
    // KEYS    = the lock keys to set
    // ARGV[1] = token (the UUID of the lock owner)
    // ARGV[2] = new TTL (milliseconds)
    Script::new(
        r#"
            local any_exists = redis.call("EXISTS", unpack(KEYS))

            if any_exists > 0 then
                return 0
            end

            local uuid = ARGV[1]
            local ttl = ARGV[2]

            for i = 1, #KEYS do
                redis.call("SET", KEYS[i], uuid, "PX", ttl)
            end

            return 1
        "#,
    )
});

pub(crate) static SCRIPT_RENEW_LOCK: LazyLock<Script> = LazyLock::new(|| {
    // KEYS[1] = the lock key to renew
    // ARGV[1] = token (the UUID of the lock owner)
    // ARGV[2] = new TTL (milliseconds)
    Script::new(
        r#"
            if redis.call("GET", KEYS[1]) == ARGV[1] then
                return redis.call("PEXPIRE", KEYS[1], ARGV[2])
            else
                return 0
            end
        "#,
    )
});

pub(crate) static SCRIPT_RENEW_MULTI_KEY_LOCK: LazyLock<Script> = LazyLock::new(|| {
    // KEYS[1] = the lock key to renew
    // ARGV[1] = token (the UUID of the lock owner)
    // ARGV[2] = new TTL (milliseconds)
    Script::new(
        r#"
            local uuid = ARGV[1]

            for i = 1, #KEYS do
                if redis.call("GET", KEYS[i]) ~= uuid then
                    return 0
                end
            end

            local ttl = ARGV[2]

            for i = 1, #KEYS do
                redis.call("PEXPIRE", KEYS[i], ttl)
            end

            return 1
        "#,
    )
});

pub(crate) static SCRIPT_RELEASE_LOCK: LazyLock<Script> = LazyLock::new(|| {
    // KEYS[1] = the lock key to release
    // ARGV[1] = token (the UUID of the lock owner)
    Script::new(
        r#"
            if redis.call("GET", KEYS[1]) == ARGV[1] then
                return redis.call("DEL", KEYS[1])
            else
                return 0
            end
        "#,
    )
});

pub(crate) static SCRIPT_RELEASE_MULTI_KEY_LOCK: LazyLock<Script> = LazyLock::new(|| {
    // KEYS[1] = the lock key to release
    // ARGV[1] = token (the UUID of the lock owner)
    Script::new(
        r#"
            local uuid = ARGV[1]

            for i = 1, #KEYS do
                if redis.call("GET", KEYS[i]) ~= uuid then
                    return 0
                end
            end

            return redis.call("DEL", unpack(KEYS))
        "#,
    )
});

// A read-write lock is stored as a hash at the lock key.
// The field "w" holds the writer UUID, and each field "r:<uuid>" holds the expiry timestamp (milliseconds) of a reader.
// The PTTL of the hash is kept not shorter than the furthest holder expiry, and it is only extended after a PTTL comparison because "PEXPIRE ... GT" requires Redis 7.
// These scripts write after reading the server time, which relies on effect replication, so Redis 5.0 or later is required.

pub(crate) static SCRIPT_ACQUIRE_READ_LOCK: LazyLock<Script> = LazyLock::new(|| {
    // KEYS[1] = the lock key (a hash) to acquire
    // ARGV[1] = token (the UUID of the reader)
    // ARGV[2] = new TTL (milliseconds)
    Script::new(
        r#"
            if redis.call("HEXISTS", KEYS[1], "w") == 1 then
                return 0
            end

            local time = redis.call("TIME")
            local now = tonumber(time[1]) * 1000 + math.floor(tonumber(time[2]) / 1000)

            local ttl = tonumber(ARGV[2])

            redis.call("HSET", KEYS[1], "r:" .. ARGV[1], now + ttl)

            local pttl = redis.call("PTTL", KEYS[1])

            if pttl < ttl then
                redis.call("PEXPIRE", KEYS[1], ttl)
            end

            return 1
        "#,
    )
});

pub(crate) static SCRIPT_RENEW_READ_LOCK: LazyLock<Script> = LazyLock::new(|| {
    // KEYS[1] = the lock key (a hash) to renew
    // ARGV[1] = token (the UUID of the reader)
    // ARGV[2] = new TTL (milliseconds)
    Script::new(
        r#"
            local field = "r:" .. ARGV[1]

            if redis.call("HEXISTS", KEYS[1], field) == 0 then
                return 0
            end

            local time = redis.call("TIME")
            local now = tonumber(time[1]) * 1000 + math.floor(tonumber(time[2]) / 1000)

            local ttl = tonumber(ARGV[2])

            redis.call("HSET", KEYS[1], field, now + ttl)

            local pttl = redis.call("PTTL", KEYS[1])

            if pttl < ttl then
                redis.call("PEXPIRE", KEYS[1], ttl)
            end

            return 1
        "#,
    )
});

pub(crate) static SCRIPT_RELEASE_READ_LOCK: LazyLock<Script> = LazyLock::new(|| {
    // KEYS[1] = the lock key (a hash) to release
    // ARGV[1] = token (the UUID of the reader)
    // the field name contains the UUID, so an unconditional HDEL cannot delete the entry of another reader, and an empty hash is automatically removed by Redis
    Script::new(
        r#"
            return redis.call("HDEL", KEYS[1], "r:" .. ARGV[1])
        "#,
    )
});

pub(crate) static SCRIPT_ACQUIRE_WRITE_LOCK: LazyLock<Script> = LazyLock::new(|| {
    // KEYS[1] = the lock key (a hash) to acquire
    // ARGV[1] = token (the UUID of the writer)
    // ARGV[2] = new TTL (milliseconds)
    Script::new(
        r#"
            if redis.call("HEXISTS", KEYS[1], "w") == 1 then
                return 0
            end

            local time = redis.call("TIME")
            local now = tonumber(time[1]) * 1000 + math.floor(tonumber(time[2]) / 1000)

            -- the "w" field does not exist here, so all remaining fields are reader fields
            local fields = redis.call("HGETALL", KEYS[1])

            for i = 1, #fields, 2 do
                if tonumber(fields[i + 1]) > now then
                    -- a live reader holds the lock, and the expired fields which have already been deleted are a harmless side effect
                    return 0
                else
                    redis.call("HDEL", KEYS[1], fields[i])
                end
            end

            -- the writer is now the only live holder, so overwriting a longer leftover TTL is correct
            redis.call("HSET", KEYS[1], "w", ARGV[1])
            redis.call("PEXPIRE", KEYS[1], ARGV[2])

            return 1
        "#,
    )
});

pub(crate) static SCRIPT_RENEW_WRITE_LOCK: LazyLock<Script> = LazyLock::new(|| {
    // KEYS[1] = the lock key (a hash) to renew
    // ARGV[1] = token (the UUID of the writer)
    // ARGV[2] = new TTL (milliseconds)
    Script::new(
        r#"
            if redis.call("HGET", KEYS[1], "w") == ARGV[1] then
                return redis.call("PEXPIRE", KEYS[1], ARGV[2])
            else
                return 0
            end
        "#,
    )
});

pub(crate) static SCRIPT_RELEASE_WRITE_LOCK: LazyLock<Script> = LazyLock::new(|| {
    // KEYS[1] = the lock key (a hash) to release
    // ARGV[1] = token (the UUID of the writer)
    // only the "w" field is deleted instead of the whole key, so the remaining PTTL still garbage-collects the expired reader fields
    Script::new(
        r#"
            if redis.call("HGET", KEYS[1], "w") == ARGV[1] then
                return redis.call("HDEL", KEYS[1], "w")
            else
                return 0
            end
        "#,
    )
});

pub(crate) static SCRIPT_ACQUIRE_MULTI_KEY_READ_LOCK: LazyLock<Script> = LazyLock::new(|| {
    // KEYS    = the lock keys (hashes) to acquire
    // ARGV[1] = token (the UUID of the reader)
    // ARGV[2] = new TTL (milliseconds)
    Script::new(
        r#"
            for i = 1, #KEYS do
                if redis.call("HEXISTS", KEYS[i], "w") == 1 then
                    return 0
                end
            end

            local time = redis.call("TIME")
            local now = tonumber(time[1]) * 1000 + math.floor(tonumber(time[2]) / 1000)

            local field = "r:" .. ARGV[1]
            local ttl = tonumber(ARGV[2])

            for i = 1, #KEYS do
                redis.call("HSET", KEYS[i], field, now + ttl)

                local pttl = redis.call("PTTL", KEYS[i])

                if pttl < ttl then
                    redis.call("PEXPIRE", KEYS[i], ttl)
                end
            end

            return 1
        "#,
    )
});

pub(crate) static SCRIPT_RENEW_MULTI_KEY_READ_LOCK: LazyLock<Script> = LazyLock::new(|| {
    // KEYS    = the lock keys (hashes) to renew
    // ARGV[1] = token (the UUID of the reader)
    // ARGV[2] = new TTL (milliseconds)
    Script::new(
        r#"
            local field = "r:" .. ARGV[1]

            for i = 1, #KEYS do
                if redis.call("HEXISTS", KEYS[i], field) == 0 then
                    return 0
                end
            end

            local time = redis.call("TIME")
            local now = tonumber(time[1]) * 1000 + math.floor(tonumber(time[2]) / 1000)

            local ttl = tonumber(ARGV[2])

            for i = 1, #KEYS do
                redis.call("HSET", KEYS[i], field, now + ttl)

                local pttl = redis.call("PTTL", KEYS[i])

                if pttl < ttl then
                    redis.call("PEXPIRE", KEYS[i], ttl)
                end
            end

            return 1
        "#,
    )
});

pub(crate) static SCRIPT_RELEASE_MULTI_KEY_READ_LOCK: LazyLock<Script> = LazyLock::new(|| {
    // KEYS    = the lock keys (hashes) to release
    // ARGV[1] = token (the UUID of the reader)
    // the field name contains the UUID, so an unconditional HDEL cannot delete the entry of another reader
    Script::new(
        r#"
            local field = "r:" .. ARGV[1]
            local count = 0

            for i = 1, #KEYS do
                count = count + redis.call("HDEL", KEYS[i], field)
            end

            if count == #KEYS then
                return 1
            else
                return 0
            end
        "#,
    )
});

pub(crate) static SCRIPT_ACQUIRE_MULTI_KEY_WRITE_LOCK: LazyLock<Script> = LazyLock::new(|| {
    // KEYS    = the lock keys (hashes) to acquire
    // ARGV[1] = token (the UUID of the writer)
    // ARGV[2] = new TTL (milliseconds)
    Script::new(
        r#"
            for i = 1, #KEYS do
                if redis.call("HEXISTS", KEYS[i], "w") == 1 then
                    return 0
                end
            end

            local time = redis.call("TIME")
            local now = tonumber(time[1]) * 1000 + math.floor(tonumber(time[2]) / 1000)

            for i = 1, #KEYS do
                -- the "w" field does not exist here, so all remaining fields are reader fields
                local fields = redis.call("HGETALL", KEYS[i])

                for j = 1, #fields, 2 do
                    if tonumber(fields[j + 1]) > now then
                        -- a live reader holds the lock, and the expired fields which have already been deleted are a harmless side effect
                        return 0
                    else
                        redis.call("HDEL", KEYS[i], fields[j])
                    end
                end
            end

            local uuid = ARGV[1]
            local ttl = ARGV[2]

            for i = 1, #KEYS do
                redis.call("HSET", KEYS[i], "w", uuid)
                redis.call("PEXPIRE", KEYS[i], ttl)
            end

            return 1
        "#,
    )
});

pub(crate) static SCRIPT_RENEW_MULTI_KEY_WRITE_LOCK: LazyLock<Script> = LazyLock::new(|| {
    // KEYS    = the lock keys (hashes) to renew
    // ARGV[1] = token (the UUID of the writer)
    // ARGV[2] = new TTL (milliseconds)
    Script::new(
        r#"
            local uuid = ARGV[1]

            for i = 1, #KEYS do
                if redis.call("HGET", KEYS[i], "w") ~= uuid then
                    return 0
                end
            end

            local ttl = ARGV[2]

            for i = 1, #KEYS do
                redis.call("PEXPIRE", KEYS[i], ttl)
            end

            return 1
        "#,
    )
});

pub(crate) static SCRIPT_RELEASE_MULTI_KEY_WRITE_LOCK: LazyLock<Script> = LazyLock::new(|| {
    // KEYS    = the lock keys (hashes) to release
    // ARGV[1] = token (the UUID of the writer)
    Script::new(
        r#"
            local uuid = ARGV[1]

            for i = 1, #KEYS do
                if redis.call("HGET", KEYS[i], "w") ~= uuid then
                    return 0
                end
            end

            for i = 1, #KEYS do
                redis.call("HDEL", KEYS[i], "w")
            end

            return 1
        "#,
    )
});
