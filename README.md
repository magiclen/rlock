RLock (Redis Lock)
====================

[![CI](https://github.com/magiclen/rlock/actions/workflows/ci.yml/badge.svg)](https://github.com/magiclen/rlock/actions/workflows/ci.yml)

RLock provides Redis-backed mutex and read-write lock leases, with async and sync APIs and atomic acquisition across multiple keys.
It requires Rust 1.89 or later and Redis 5.0 or later.

## Installation

The default `async` feature uses Tokio:

```toml
[dependencies]
rlock = "0.1"
tokio = { version = "1.19", features = ["macros", "rt"] }
```

For the sync API only:

```toml
[dependencies]
rlock = { version = "0.1", default-features = false, features = ["sync"] }
```

To use both APIs, keep the default features and enable `sync`:

```toml
[dependencies]
rlock = { version = "0.1", features = ["sync"] }
tokio = { version = "1.19", features = ["macros", "rt"] }
```

## Async example

Keep the guard alive while using the protected resource and explicitly release it when completion matters:

```rust
use std::time::Duration;

use rlock::{AcquireOptions, RenewalMode, async_lock::RLock};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rlock = RLock::new("redis://127.0.0.1:6379/0").await?;
    let options = AcquireOptions::builder()
        .ttl(Duration::from_secs(3))
        .lock_timeout(Some(Duration::from_secs(2)))
        .renewal_mode(RenewalMode::Auto)
        .build()?;

    let mut guard = rlock
        .acquire_mutex_with_options("locks:reports:daily", options)
        .await?;

    println!("The daily report lease is {:?}.", guard.lease_state());

    guard.release().await?;
    rlock.shutdown().await;
    Ok(())
}
```

`AcquireOptions::default()` uses a 10-second TTL, a 100-millisecond retry interval, automatic renewal, and no acquisition timeout.
`RenewalMode::Auto` renews at half the effective TTL; `RenewalMode::Interval(duration)` selects an interval, and `RenewalMode::Disabled` turns renewal off.
TTL values are rounded down to whole milliseconds, and an enabled renewal interval must be shorter than the effective TTL.
The builder validates these options before use.
The existing `renew_interval` getter and builder method remain available.

## Sync and multiple keys

`LockKeys` validates that a key list is nonempty and contains no duplicates.
Pass it by reference to reuse the same set for multiple acquisitions.
This example attempts the entire set once and distinguishes contention from an error:

```rust
use std::time::Duration;

use rlock::{AcquireOptions, LockKeys, sync_lock::RLock};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rlock = RLock::new("redis://127.0.0.1:6379/0")?;
    let keys = LockKeys::new(["locks:accounts:123", "locks:accounts:456"])?;
    let options = AcquireOptions::builder()
        .lock_timeout(Some(Duration::from_secs(1)))
        .build()?;

    match rlock.try_acquire_multi_key_mutex_with_options(&keys, options)? {
        Some(mut guard) => {
            println!("Acquired both account leases.");
            guard.release()?;
        }
        None => println!("At least one account is busy."),
    }

    rlock.shutdown();
    Ok(())
}
```

Both backends provide `acquire_mutex`, `acquire_read`, and `acquire_write`, plus their `acquire_multi_key_*` counterparts.
Each has `_with_options`, `try_acquire_*`, and `try_acquire_*_with_options` variants.
For a single read-write resource, use `acquire_read("locks:catalog")` for shared access and `acquire_write("locks:catalog")` for exclusive access.
Readers are preferred: continuous readers can starve a writer, so set an acquisition timeout when an indefinite wait is unsuitable.

Mutex keys contain Redis strings; read-write keys contain sorted sets, with all holders stored at the supplied key.
Do not use mutex and read-write operations on the same key.
Successful multi-key acquisition covers the whole set; contention does not leave a partial acquisition.
Expiry cleanup uses an ordered index instead of scanning all live readers.
The index can use more memory than the previous hash, and removing many expired holders at once can still increase Lua latency.

## Lease state and release

A guard's lifetime can exceed its Redis lease.
`lease_state()` reports the local state:

- `LeaseState::Active`: within the last confirmed, conservative lease deadline.
- `LeaseState::Lost`: the deadline passed, renewal failed, or release began.
- `LeaseState::Released`: explicit release completed or the guard was dropped.

Async guards also provide `wait_until_inactive().await`, which observes loss or release even when automatic renewal is disabled.
`is_released()` only records successful completion of explicit release; `false` does not mean the lease is active.

Starting `release()` marks the lease lost and stops automatic renewal.
If release fails, or an async release wait is cancelled, the guard remains lost and release may be retried.
Cancelling that wait does not cancel the Redis release operation already in progress.
Dropping a guard stops renewal and attempts to queue release; a full or unavailable queue, or a failed release, leaves TTL expiry as the fallback.

`Active` is a conservative local observation, not a guarantee that a later external write will be safe.
Redis failover, clock changes, network failures, or a process pause can invalidate exclusivity.
These leases do not provide Rust memory synchronization and must not be used to justify unsynchronized access to local `unsafe` data.
For external resources that require stale writers to be rejected, use resource-side fencing or conditional version checks; RLock does not supply fencing tokens.

## Timeouts and cancellation

`lock_timeout` covers the complete acquisition wait, including retries and Redis I/O.
`Some(Duration::ZERO)` immediately returns `AcquireError::LockTimeout` without sending an acquisition command.
A `try_acquire_*` call makes one acquisition attempt: contention returns `Ok(None)`, success returns `Ok(Some(guard))`, and errors remain errors.
It still waits for Redis I/O and honors the supplied acquisition timeout.

Cancelling or timing out an async acquisition leaves responsibility for an in-flight command with its supervisor.
The supervisor waits for the original request to finish before cleaning up an unclaimed successful acquisition.
A network error can leave the server-side outcome unknown, and stopping the Tokio runtime can interrupt cleanup; TTL expiry remains the fallback in these cases.

Sync acquisitions use an additional worker only when `lock_timeout` is `Some`.
The deadline bounds the caller's wait and does not guarantee that background I/O or cleanup ends immediately.
Without an acquisition timeout, the sync API performs Redis I/O inline.
Shutdown wakes retry waits, but it cannot interrupt sync Redis I/O already in progress.

## Shutdown

`shutdown()` seals the manager against new acquisitions and waits for releases already queued before its barrier.
Concurrent calls wait for the same barrier, and cancelling an async shutdown wait does not cancel shutdown itself.
Existing guards can continue renewing and explicitly releasing their leases after shutdown returns.
The release worker finishes once no guard or pending cleanup still needs it and queued releases have drained.
In-flight acquisition commands finish independently and clean up results that are no longer accepted.
Shutdown therefore does not wait for every live guard or every in-flight Redis command to end.

## Breaking changes and migration

**Read-write lock storage changed from a Redis hash to a sorted set at the same key.**
Old and new versions cannot coordinate read-write ownership on that key.
Do not run them concurrently against the same protected resource, and do not switch to another namespace to bypass this incompatibility: separate keys would allow both versions to acquire the resource independently.

1. Stop new acquisitions in every old-version client.
2. Drain the protected work, explicitly release all old guards, and stop all old renewal tasks or threads.
3. Confirm that the old read-write keys have disappeared through release or expiry.
4. Switch all clients for those resources to the new version before resuming acquisitions.

Waiting only the default 10 seconds is insufficient because old guards can renew their leases.
The library does not automatically delete hashes encountered at read-write lock keys.

The public error enums have gained variants and are now `#[non_exhaustive]`, so a `match` on them needs a wildcard arm.
The Tokio requirement is now 1.19 or later because shutdown coordination uses `watch::Sender::send_if_modified`.
Review the timeout and shutdown contracts above when upgrading from earlier behavior.

## Crates.io

https://crates.io/crates/rlock

## Documentation

https://docs.rs/rlock

## License

[MIT](LICENSE)
