use once_cell::sync::Lazy;
use redis::Script;

pub(crate) static SCRIPT_ACQUIRE_MULTI_KEY_LOCK: Lazy<Script> = Lazy::new(|| {
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

pub(crate) static SCRIPT_RENEW_LOCK: Lazy<Script> = Lazy::new(|| {
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

pub(crate) static SCRIPT_RENEW_MULTI_KEY_LOCK: Lazy<Script> = Lazy::new(|| {
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

pub(crate) static SCRIPT_RELEASE_LOCK: Lazy<Script> = Lazy::new(|| {
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

pub(crate) static SCRIPT_RELEASE_MULTI_KEY_LOCK: Lazy<Script> = Lazy::new(|| {
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
