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
    static mut COUNTER: u32 = 0;

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

# #[cfg(not(feature = "async"))]
# fn main() {}
```

## Roadmap / TODO

- [x] Asynchronous mutex lock
- [ ] Synchronous mutex lock
- [ ] Asynchronous read-write lock
- [ ] Synchronous read-write lock
*/

#![cfg_attr(docsrs, feature(doc_auto_cfg))]

#[cfg(feature = "async")]
/// This module provides an async API based on **Tokio**.
pub mod async_lock;
#[cfg(any(feature = "async", feature = "sync"))]
mod errors;
mod functions;
#[cfg(any(feature = "async", feature = "sync"))]
mod options;
#[cfg(any(feature = "async", feature = "sync"))]
mod release_request;
#[cfg(any(feature = "async", feature = "sync"))]
mod scripts;

#[cfg(any(feature = "async", feature = "sync"))]
pub use errors::*;
pub use functions::*;
#[cfg(any(feature = "async", feature = "sync"))]
pub use options::*;
#[cfg(any(feature = "async", feature = "sync"))]
pub use redis::{ConnectionInfo, IntoConnectionInfo};
