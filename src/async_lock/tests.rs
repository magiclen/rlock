use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use once_cell::sync::Lazy;
use tokio::task::JoinSet;

use super::*;

static REDIS_URL: Lazy<String> = Lazy::new(|| {
    std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379/0".to_string())
});
static GLOBAL_LOCK_1_2: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
static GLOBAL_LOCK_3_4: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

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
