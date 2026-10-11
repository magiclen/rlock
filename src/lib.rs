/*!
# RLock (Redis Lock)

It is an easy-to-use Redis-backed lock library providing both async and sync Mutex, RwLock, and others.

## Examples

```rust,no_run
# #[cfg(feature = "async")]
use tokio::task::JoinSet;
# #[cfg(feature = "async")]
use rlock::async_lock::RLock;

# #[cfg(feature = "async")]
#[tokio::main]
async fn main() {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    let rlock = RLock::new("redis://127.0.0.1:6379/0").await.unwrap();

    async fn counter_increase(rlock: RLock) {
        let lock = rlock.acquire_mutex("lock").await.unwrap();

        // ----- critical section -----

        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // ----------------------------

        drop(lock);
    }

    let mut tasks = JoinSet::new();

    for _ in 0..1000 {
        tasks.spawn(counter_increase(rlock.clone()));
    }

    tasks.join_all().await;

    assert_eq!(1000, COUNTER.load(std::sync::atomic::Ordering::Relaxed));

    rlock.shutdown().await;
}

# #[cfg(not(feature = "async"))]
# fn main() {}
```

```rust,no_run
# #[cfg(feature = "async")]
use tokio::task::JoinSet;
# #[cfg(feature = "async")]
use rlock::async_lock::RLock;

# #[cfg(feature = "async")]
#[tokio::main]
async fn main() {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    let rlock = RLock::new("redis://127.0.0.1:6379/0").await.unwrap();

    async fn writer(rlock: RLock) {
        let lock = rlock.acquire_write("lock").await.unwrap();

        // ----- critical section -----

        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // ----------------------------

        drop(lock);
    }

    async fn reader(rlock: RLock) {
        let lock = rlock.acquire_read("lock").await.unwrap();

        // ----- shared section -----

        // multiple readers can hold the lock at the same time
        let _value = COUNTER.load(std::sync::atomic::Ordering::Relaxed);

        // --------------------------

        drop(lock);
    }

    let mut tasks = JoinSet::new();

    for _ in 0..100 {
        tasks.spawn(writer(rlock.clone()));
        tasks.spawn(reader(rlock.clone()));
    }

    tasks.join_all().await;

    assert_eq!(100, COUNTER.load(std::sync::atomic::Ordering::Relaxed));

    rlock.shutdown().await;
}

# #[cfg(not(feature = "async"))]
# fn main() {}
```

```rust,no_run
# #[cfg(feature = "sync")]
use rlock::sync_lock::RLock;

# #[cfg(feature = "sync")]
fn main() {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    let rlock = RLock::new("redis://127.0.0.1:6379/0").unwrap();

    fn counter_increase(rlock: RLock) {
        let lock = rlock.acquire_mutex("lock").unwrap();

        // ----- critical section -----

        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

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

    assert_eq!(100, COUNTER.load(std::sync::atomic::Ordering::Relaxed));

    rlock.shutdown();
}

# #[cfg(not(feature = "sync"))]
# fn main() {}
```

```rust,no_run
# #[cfg(feature = "sync")]
use rlock::sync_lock::RLock;

# #[cfg(feature = "sync")]
fn main() {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    let rlock = RLock::new("redis://127.0.0.1:6379/0").unwrap();

    fn writer(rlock: RLock) {
        let lock = rlock.acquire_write("lock").unwrap();

        // ----- critical section -----

        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // ----------------------------

        drop(lock);
    }

    fn reader(rlock: RLock) {
        let lock = rlock.acquire_read("lock").unwrap();

        // ----- shared section -----

        // multiple readers can hold the lock at the same time
        let _value = COUNTER.load(std::sync::atomic::Ordering::Relaxed);

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

    assert_eq!(100, COUNTER.load(std::sync::atomic::Ordering::Relaxed));

    rlock.shutdown();
}

# #[cfg(not(feature = "sync"))]
# fn main() {}
```
*/

#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "async")]
/// This module provides an async API based on **Tokio**.
pub mod async_lock;
#[cfg(any(feature = "async", feature = "sync"))]
mod errors;
mod functions;
#[cfg(any(feature = "async", feature = "sync"))]
mod lease;
#[cfg(any(feature = "async", feature = "sync"))]
mod lock_keys;
#[cfg(any(feature = "async", feature = "sync"))]
mod options;
#[cfg(any(feature = "async", feature = "sync"))]
mod release_request;
#[cfg(any(feature = "async", feature = "sync"))]
mod scripts;
#[cfg(feature = "sync")]
/// This module provides a sync API based on threads with a simple built-in connection pool.
pub mod sync_lock;

#[cfg(any(feature = "async", feature = "sync"))]
pub use errors::*;
pub use functions::*;
#[cfg(any(feature = "async", feature = "sync"))]
pub use lease::LeaseState;
#[cfg(any(feature = "async", feature = "sync"))]
pub use lock_keys::*;
#[cfg(any(feature = "async", feature = "sync"))]
pub use options::*;
#[cfg(any(feature = "async", feature = "sync"))]
pub use redis;

#[cfg(all(test, any(feature = "async", feature = "sync")))]
mod test_proxy;
#[cfg(all(test, any(feature = "async", feature = "sync")))]
mod test_support;
