use std::{
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};

use tokio::task::JoinSet;

use super::*;
use crate::{AcquireError, AcquireOptions};

static REDIS_URL: LazyLock<String> = LazyLock::new(|| {
    std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379/0".to_string())
});
static GLOBAL_LOCK_1_2: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
static GLOBAL_LOCK_3_4: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

#[allow(clippy::await_holding_lock)]
async fn test_rlock_new_and_shutdown_1_2() {
    let global_lock = GLOBAL_LOCK_1_2.lock().unwrap();

    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    let lock = tokio::time::timeout(
        Duration::from_secs(3),
        rlock.acquire_mutex("test:rlock_new_and_shutdown_1_2"),
    )
    .await
    .unwrap();

    drop(lock);

    rlock.shutdown().await; // if the shutdown method is not called, one of the test case between `test_rlock_new_and_shutdown_1` and `test_rlock_new_and_shutdown_2` will panic because of timeout

    drop(global_lock);
}

#[tokio::test]
#[tracing_test::traced_test]
async fn test_rlock_new_and_shutdown_1() {
    test_rlock_new_and_shutdown_1_2().await;
}

#[tokio::test]
#[tracing_test::traced_test]
async fn test_rlock_new_and_shutdown_2() {
    test_rlock_new_and_shutdown_1_2().await;
}

#[allow(clippy::await_holding_lock)]
async fn test_rlock_new_and_shutdown_3_4(n: u32) {
    let global_lock = GLOBAL_LOCK_3_4.lock().unwrap();

    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    let lock = tokio::time::timeout(
        Duration::from_secs(3),
        rlock.acquire_multi_key_mutex(Arc::new(vec![
            String::from("test:rlock_new_and_shutdown_3_4"),
            format!("test:rlock_new_and_shutdown_{n}"),
        ])),
    )
    .await
    .unwrap();

    drop(lock);

    rlock.shutdown().await; // if the shutdown method is not called, one of the test case between `test_rlock_new_and_shutdown_3` and `test_rlock_new_and_shutdown_4` will panic because of timeout

    drop(global_lock);
}

#[tokio::test]
#[tracing_test::traced_test]
async fn test_rlock_new_and_shutdown_3() {
    test_rlock_new_and_shutdown_3_4(3).await;
}

#[tokio::test]
#[tracing_test::traced_test]
async fn test_rlock_new_and_shutdown_4() {
    test_rlock_new_and_shutdown_3_4(4).await;
}

#[tokio::test]
#[tracing_test::traced_test]
async fn test_acquire_mutex() {
    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    let lock = rlock.acquire_mutex("test:acquire_mutex").await.unwrap();

    drop(lock);

    rlock.shutdown().await;
}

#[tokio::test]
#[tracing_test::traced_test]
async fn test_release_mutex() {
    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    let mut lock = rlock.acquire_mutex("test:release_mutex").await.unwrap();

    lock.release().await.unwrap();

    rlock.shutdown().await;
}

#[tokio::test]
#[tracing_test::traced_test]
async fn test_acquire_multi_key_mutex() {
    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    let lock = rlock
        .acquire_multi_key_mutex(Arc::new(vec![String::from("test:acquire_multi_key_mutex")]))
        .await
        .unwrap();

    drop(lock);

    rlock.shutdown().await;
}

#[tokio::test]
#[tracing_test::traced_test]
async fn test_release_multi_key_mutex() {
    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    let mut lock = rlock
        .acquire_multi_key_mutex(Arc::new(vec![String::from("test:release_multi_key_mutex")]))
        .await
        .unwrap();

    lock.release().await.unwrap();

    rlock.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[tracing_test::traced_test]
async fn test_mutex_critical_section() {
    static mut COUNTER: u32 = 0;

    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    async fn counter_increase(rlock: RLock) {
        let lock = rlock.acquire_mutex("test:mutex_critical_section").await.unwrap();

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

#[tokio::test]
#[tracing_test::traced_test]
async fn test_acquire_read() {
    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    let lock = rlock.acquire_read("test:acquire_read").await.unwrap();

    drop(lock);

    rlock.shutdown().await;
}

#[tokio::test]
#[tracing_test::traced_test]
async fn test_release_read() {
    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    let mut lock = rlock.acquire_read("test:release_read").await.unwrap();

    lock.release().await.unwrap();

    rlock.shutdown().await;
}

#[tokio::test]
#[tracing_test::traced_test]
async fn test_acquire_write() {
    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    let lock = rlock.acquire_write("test:acquire_write").await.unwrap();

    drop(lock);

    rlock.shutdown().await;
}

#[tokio::test]
#[tracing_test::traced_test]
async fn test_release_write() {
    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    let mut lock = rlock.acquire_write("test:release_write").await.unwrap();

    lock.release().await.unwrap();

    rlock.shutdown().await;
}

#[tokio::test]
#[tracing_test::traced_test]
async fn test_read_locks_are_shared() {
    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    let lock_1 = rlock.acquire_read("test:read_locks_are_shared").await.unwrap();

    // if read locks were wrongly exclusive, this acquisition would spin on retries and hit the timeout
    let lock_2 = tokio::time::timeout(
        Duration::from_secs(3),
        rlock.acquire_read("test:read_locks_are_shared"),
    )
    .await
    .unwrap()
    .unwrap();

    drop(lock_1);
    drop(lock_2);

    rlock.shutdown().await;
}

#[tokio::test]
#[tracing_test::traced_test]
async fn test_write_lock_excluded_by_read_lock() {
    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    let mut read_lock = rlock.acquire_read("test:write_lock_excluded_by_read_lock").await.unwrap();

    let result = rlock
        .acquire_write_with_options(
            "test:write_lock_excluded_by_read_lock",
            AcquireOptions::builder().lock_timeout(Some(Duration::from_secs(1))).build().unwrap(),
        )
        .await;

    assert!(matches!(result, Err(AcquireError::LockTimeout)));

    read_lock.release().await.unwrap();

    let write_lock = tokio::time::timeout(
        Duration::from_secs(3),
        rlock.acquire_write("test:write_lock_excluded_by_read_lock"),
    )
    .await
    .unwrap()
    .unwrap();

    drop(write_lock);

    rlock.shutdown().await;
}

#[tokio::test]
#[tracing_test::traced_test]
async fn test_read_lock_excluded_by_write_lock() {
    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    let mut write_lock =
        rlock.acquire_write("test:read_lock_excluded_by_write_lock").await.unwrap();

    let result = rlock
        .acquire_read_with_options(
            "test:read_lock_excluded_by_write_lock",
            AcquireOptions::builder().lock_timeout(Some(Duration::from_secs(1))).build().unwrap(),
        )
        .await;

    assert!(matches!(result, Err(AcquireError::LockTimeout)));

    write_lock.release().await.unwrap();

    let read_lock = tokio::time::timeout(
        Duration::from_secs(3),
        rlock.acquire_read("test:read_lock_excluded_by_write_lock"),
    )
    .await
    .unwrap()
    .unwrap();

    drop(read_lock);

    rlock.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[tracing_test::traced_test]
async fn test_write_lock_critical_section() {
    static mut COUNTER: u32 = 0;

    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    async fn counter_increase(rlock: RLock) {
        let lock = rlock.acquire_write("test:write_lock_critical_section").await.unwrap();

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

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[tracing_test::traced_test]
async fn test_rw_lock_exclusion() {
    static mut COUNTER: u32 = 0;

    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    async fn writer(rlock: RLock) {
        let lock = rlock.acquire_write("test:rw_lock_exclusion").await.unwrap();

        // ----- critical section -----

        // the counter is odd only inside a held write lock
        unsafe { COUNTER += 1 };

        tokio::time::sleep(Duration::from_millis(1)).await;

        unsafe { COUNTER += 1 };

        // ----------------------------

        drop(lock);
    }

    async fn reader(rlock: RLock) {
        let lock = rlock.acquire_read("test:rw_lock_exclusion").await.unwrap();

        // ----- shared section -----

        // a reader observing an odd counter proves that the write exclusion is broken
        assert_eq!(0, unsafe { COUNTER } % 2);

        // --------------------------

        drop(lock);
    }

    let mut tasks = JoinSet::new();

    for _ in 0..100 {
        tasks.spawn(writer(rlock.clone()));
        tasks.spawn(reader(rlock.clone()));
    }

    tasks.join_all().await;

    assert_eq!(200, unsafe { COUNTER });

    rlock.shutdown().await;
}

#[tokio::test]
#[tracing_test::traced_test]
async fn test_acquire_multi_key_read() {
    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    let lock = rlock
        .acquire_multi_key_read(Arc::new(vec![String::from("test:acquire_multi_key_read")]))
        .await
        .unwrap();

    drop(lock);

    rlock.shutdown().await;
}

#[tokio::test]
#[tracing_test::traced_test]
async fn test_release_multi_key_read() {
    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    let mut lock = rlock
        .acquire_multi_key_read(Arc::new(vec![String::from("test:release_multi_key_read")]))
        .await
        .unwrap();

    lock.release().await.unwrap();

    rlock.shutdown().await;
}

#[tokio::test]
#[tracing_test::traced_test]
async fn test_acquire_multi_key_write() {
    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    let lock = rlock
        .acquire_multi_key_write(Arc::new(vec![String::from("test:acquire_multi_key_write")]))
        .await
        .unwrap();

    drop(lock);

    rlock.shutdown().await;
}

#[tokio::test]
#[tracing_test::traced_test]
async fn test_release_multi_key_write() {
    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    let mut lock = rlock
        .acquire_multi_key_write(Arc::new(vec![String::from("test:release_multi_key_write")]))
        .await
        .unwrap();

    lock.release().await.unwrap();

    rlock.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[tracing_test::traced_test]
async fn test_multi_key_write_lock_critical_section() {
    static mut COUNTER: u32 = 0;

    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    async fn counter_increase(rlock: RLock, i: u32) {
        let lock = rlock
            .acquire_multi_key_write(Arc::new(vec![
                String::from("test:multi_key_write_lock_critical_section"),
                format!("test:multi_key_write_lock_critical_section_{i}"),
            ]))
            .await
            .unwrap();

        // ----- critical section -----

        unsafe { COUNTER += 1 };

        // ----------------------------

        drop(lock);
    }

    let mut tasks = JoinSet::new();

    for i in 0..1000 {
        tasks.spawn(counter_increase(rlock.clone(), i));
    }

    tasks.join_all().await;

    assert_eq!(1000, unsafe { COUNTER });

    rlock.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[tracing_test::traced_test]
async fn test_multi_key_mutex_critical_section() {
    static mut COUNTER: u32 = 0;

    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    async fn counter_increase(rlock: RLock, i: u32) {
        let lock = rlock
            .acquire_multi_key_mutex(Arc::new(vec![
                String::from("test:multi_key_mutex_critical_section"),
                format!("test:multi_key_mutex_critical_section_{i}"),
            ]))
            .await
            .unwrap();

        // ----- critical section -----

        unsafe { COUNTER += 1 };

        // ----------------------------

        drop(lock);
    }

    let mut tasks = JoinSet::new();

    for i in 0..1000 {
        tasks.spawn(counter_increase(rlock.clone(), i));
    }

    tasks.join_all().await;

    assert_eq!(1000, unsafe { COUNTER });

    rlock.shutdown().await;
}
