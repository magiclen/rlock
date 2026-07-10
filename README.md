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

```rust
use tokio::task::JoinSet;
use rlock::async_lock::RLock;

static mut COUNTER: u32 = 0;

#[tokio::main]
async fn main() {
    let rlock = RLock::new("redis://127.0.0.1:6379/0").await.unwrap();

    async fn writer(rlock: RLock) {
        let lock = rlock.acquire_write("lock").await.unwrap();

        // ----- critical section -----

        unsafe { COUNTER += 1 };

        // ----------------------------

        drop(lock);
    }

    async fn reader(rlock: RLock) {
        let lock = rlock.acquire_read("lock").await.unwrap();

        // ----- shared section -----

        // multiple readers can hold the lock at the same time
        let _value = unsafe { COUNTER };

        // --------------------------

        drop(lock);
    }

    let mut tasks = JoinSet::new();

    for _ in 0..100 {
        tasks.spawn(writer(rlock.clone()));
        tasks.spawn(reader(rlock.clone()));
    }

    tasks.join_all().await;

    assert_eq!(100, unsafe { COUNTER });

    rlock.shutdown().await;
}
```

```rust
use rlock::sync_lock::RLock;

static mut COUNTER: u32 = 0;

fn main() {
    let rlock = RLock::new("redis://127.0.0.1:6379/0").unwrap();

    fn counter_increase(rlock: RLock) {
        let lock = rlock.acquire_mutex("lock").unwrap();

        // ----- critical section -----

        unsafe { COUNTER += 1 };

        // ----------------------------

        drop(lock);
    }

    let mut threads = Vec::new();

    for _ in 0..100 {
        let rlock = rlock.clone();

        threads.push(std::thread::spawn(move || counter_increase(rlock)));
    }

    for thread in threads {
        thread.join().unwrap();
    }

    assert_eq!(100, unsafe { COUNTER });

    rlock.shutdown();
}
```

```rust
use rlock::sync_lock::RLock;

static mut COUNTER: u32 = 0;

fn main() {
    let rlock = RLock::new("redis://127.0.0.1:6379/0").unwrap();

    fn writer(rlock: RLock) {
        let lock = rlock.acquire_write("lock").unwrap();

        // ----- critical section -----

        unsafe { COUNTER += 1 };

        // ----------------------------

        drop(lock);
    }

    fn reader(rlock: RLock) {
        let lock = rlock.acquire_read("lock").unwrap();

        // ----- shared section -----

        // multiple readers can hold the lock at the same time
        let _value = unsafe { COUNTER };

        // --------------------------

        drop(lock);
    }

    let mut threads = Vec::new();

    for _ in 0..100 {
        let rlock_for_writer = rlock.clone();
        let rlock_for_reader = rlock.clone();

        threads.push(std::thread::spawn(move || writer(rlock_for_writer)));
        threads.push(std::thread::spawn(move || reader(rlock_for_reader)));
    }

    for thread in threads {
        thread.join().unwrap();
    }

    assert_eq!(100, unsafe { COUNTER });

    rlock.shutdown();
}
```

## Crates.io

https://crates.io/crates/rlock

## Documentation

https://docs.rs/rlock

## License

[MIT](LICENSE)