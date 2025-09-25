RLock (Redis Lock)
====================

[![CI](https://github.com/magiclen/rlock/actions/workflows/ci.yml/badge.svg)](https://github.com/magiclen/rlock/actions/workflows/ci.yml)

It is an easy-to-use Redis-backed lock library providing both async and sync Mutex, RwLock, and others.

## Examples

```rust
use tokio::task::JoinSet;
use rlock::async_lock::RLock;

static mut COUNTER: u32 = 0;

#[tokio::main]
async fn main() {
    let rlock = RLock::new("redis://127.0.0.1:6379/0").await.unwrap();

    async fn counter_increase(rlock: RLock) {
        let lock = rlock.acquire_mutex("lock").await.unwrap();

        // ----- critical section -----

        unsafe { COUNTER += 1 };

        // ----------------------------

        drop(lock);
    }

    let mut tasks = JoinSet::new();

    for _ in 0..1000 {
        tasks.spawn(counter_increase(rlock.clone()));
    }

    tasks.join_all().await;

    assert_eq!(1000, unsafe { COUNTER });

    rlock.shutdown().await;
}
```

## Roadmap / TODO

- [x] Asynchronous mutex lock
- [ ] Synchronous mutex lock
- [ ] Asynchronous read-write lock
- [ ] Synchronous read-write lock

## Crates.io

https://crates.io/crates/rlock

## Documentation

https://docs.rs/rlock

## License

[MIT](LICENSE)