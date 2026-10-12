use std::{
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};

use tokio::task::JoinSet;

use super::*;
use crate::{
    AcquireError, AcquireOptions, LeaseState, LockKeys, RenewalMode,
    test_proxy::{TestProxy, assert_key_released},
    test_support::TestState,
};

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
    .unwrap()
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
    .unwrap()
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
    static COUNTER: TestState = TestState::new();

    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    async fn counter_increase(rlock: RLock) {
        let lock = rlock.acquire_mutex("test:mutex_critical_section").await.unwrap();

        let holder = COUNTER.writer();

        // ----- critical section -----

        COUNTER.increment();

        // ----------------------------

        drop(holder);
        drop(lock);
    }

    let mut tasks = JoinSet::new();

    for _ in 0..1000 {
        tasks.spawn(counter_increase(rlock.clone()));
    }

    tasks.join_all().await;

    assert_eq!(1000, COUNTER.value());

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
    static COUNTER: TestState = TestState::new();

    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    async fn counter_increase(rlock: RLock) {
        let lock = rlock.acquire_write("test:write_lock_critical_section").await.unwrap();

        let holder = COUNTER.writer();

        // ----- critical section -----

        COUNTER.increment();

        // ----------------------------

        drop(holder);
        drop(lock);
    }

    let mut tasks = JoinSet::new();

    for _ in 0..1000 {
        tasks.spawn(counter_increase(rlock.clone()));
    }

    tasks.join_all().await;

    assert_eq!(1000, COUNTER.value());

    rlock.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[tracing_test::traced_test]
async fn test_rw_lock_exclusion() {
    static COUNTER: TestState = TestState::new();

    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    async fn writer(rlock: RLock) {
        let lock = rlock.acquire_write("test:rw_lock_exclusion").await.unwrap();

        let holder = COUNTER.writer();

        // ----- critical section -----

        // the counter is odd only inside a held write lock
        COUNTER.increment();

        tokio::time::sleep(Duration::from_millis(1)).await;

        COUNTER.increment();

        // ----------------------------

        drop(holder);
        drop(lock);
    }

    async fn reader(rlock: RLock) {
        let lock = rlock.acquire_read("test:rw_lock_exclusion").await.unwrap();

        let holder = COUNTER.reader();

        // ----- shared section -----

        // a reader observing an odd counter proves that the write exclusion is broken
        assert_eq!(0, COUNTER.value() % 2);

        // --------------------------

        drop(holder);
        drop(lock);
    }

    let mut tasks = JoinSet::new();

    for _ in 0..100 {
        tasks.spawn(writer(rlock.clone()));
        tasks.spawn(reader(rlock.clone()));
    }

    tasks.join_all().await;

    assert_eq!(200, COUNTER.value());

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
    static COUNTER: TestState = TestState::new();

    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    async fn counter_increase(rlock: RLock, i: u32) {
        let lock = rlock
            .acquire_multi_key_write(Arc::new(vec![
                String::from("test:multi_key_write_lock_critical_section"),
                format!("test:multi_key_write_lock_critical_section_{i}"),
            ]))
            .await
            .unwrap();

        let holder = COUNTER.writer();

        // ----- critical section -----

        COUNTER.increment();

        // ----------------------------

        drop(holder);
        drop(lock);
    }

    let mut tasks = JoinSet::new();

    for i in 0..1000 {
        tasks.spawn(counter_increase(rlock.clone(), i));
    }

    tasks.join_all().await;

    assert_eq!(1000, COUNTER.value());

    rlock.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[tracing_test::traced_test]
async fn test_multi_key_mutex_critical_section() {
    static COUNTER: TestState = TestState::new();

    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    async fn counter_increase(rlock: RLock, i: u32) {
        let lock = rlock
            .acquire_multi_key_mutex(Arc::new(vec![
                String::from("test:multi_key_mutex_critical_section"),
                format!("test:multi_key_mutex_critical_section_{i}"),
            ]))
            .await
            .unwrap();

        let holder = COUNTER.writer();

        // ----- critical section -----

        COUNTER.increment();

        // ----------------------------

        drop(holder);
        drop(lock);
    }

    let mut tasks = JoinSet::new();

    for i in 0..1000 {
        tasks.spawn(counter_increase(rlock.clone(), i));
    }

    tasks.join_all().await;

    assert_eq!(1000, COUNTER.value());

    rlock.shutdown().await;
}

#[tokio::test]
#[tracing_test::traced_test]
async fn test_lease_renewal_and_shutdown_with_live_guard() {
    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    let mut guard = rlock
        .acquire_mutex_with_options(
            "test:async_live_shutdown",
            AcquireOptions::builder().ttl(Duration::from_millis(300)).build().unwrap(),
        )
        .await
        .unwrap();

    rlock.shutdown().await;

    tokio::time::sleep(Duration::from_millis(800)).await;

    assert_eq!(LeaseState::Active, guard.lease_state());
    assert!(matches!(rlock.acquire_mutex("test:async_closed").await, Err(AcquireError::Shutdown)));

    guard.release().await.unwrap();

    assert_eq!(LeaseState::Released, guard.wait_until_inactive().await);
    assert!(guard.is_released());

    rlock.shutdown().await;
}

#[tokio::test]
#[tracing_test::traced_test]
async fn test_disabled_renewal_expires() {
    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    let mut guard = rlock
        .acquire_read_with_options(
            "test:async_no_renewal",
            AcquireOptions::builder()
                .ttl(Duration::from_millis(150))
                .renewal_mode(RenewalMode::Disabled)
                .build()
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        LeaseState::Lost,
        tokio::time::timeout(Duration::from_secs(2), guard.wait_until_inactive()).await.unwrap()
    );
    assert!(!guard.is_released());

    guard.release().await.unwrap();

    rlock.shutdown().await;
}

#[tokio::test]
#[tracing_test::traced_test]
async fn test_try_acquisition_and_reusable_keys() {
    let rlock = RLock::new(REDIS_URL.as_str()).await.unwrap();

    let mut mutex = rlock.try_acquire_mutex("test:async_try_mutex").await.unwrap().unwrap();

    assert!(rlock.try_acquire_mutex("test:async_try_mutex").await.unwrap().is_none());

    mutex.release().await.unwrap();

    let keys = LockKeys::new(["test:async_try:a", "test:async_try:b"]).unwrap();

    let mut read = rlock.try_acquire_multi_key_read(&keys).await.unwrap().unwrap();
    let mut other = rlock.try_acquire_read("test:async_try:a").await.unwrap().unwrap();

    assert!(rlock.try_acquire_multi_key_write(&keys).await.unwrap().is_none());

    other.release().await.unwrap();
    read.release().await.unwrap();

    let mut write = rlock.try_acquire_multi_key_write(&keys).await.unwrap().unwrap();

    assert!(rlock.try_acquire_write("test:async_try:a").await.unwrap().is_none());
    assert!(rlock.try_acquire_multi_key_read(&keys).await.unwrap().is_none());

    write.release().await.unwrap();

    let mut multi = rlock.try_acquire_multi_key_mutex(&keys).await.unwrap().unwrap();

    assert!(rlock.try_acquire_mutex("test:async_try:a").await.unwrap().is_none());

    multi.release().await.unwrap();

    let options = AcquireOptions::builder().lock_timeout(Some(Duration::ZERO)).build().unwrap();

    assert!(matches!(
        rlock.try_acquire_mutex_with_options("test:async_zero", options).await,
        Err(AcquireError::LockTimeout)
    ));

    rlock.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[tracing_test::traced_test]
async fn test_canceled_acquisition_is_cleaned_up() {
    let proxy = TestProxy::new();
    let rlock = RLock::new(proxy.connection_info.clone()).await.unwrap();
    let key = "test:async_canceled_acquisition";

    proxy.arm("SET", key);

    let manager = rlock.clone();
    let task = tokio::spawn(async move { manager.acquire_mutex(key).await });

    proxy.wait_blocked().await;
    task.abort();

    assert!(task.await.unwrap_err().is_cancelled());

    proxy.resume();

    tokio::task::spawn_blocking(move || assert_key_released(key)).await.unwrap();

    rlock.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[tracing_test::traced_test]
async fn test_deadline_cleans_up_late_success() {
    let proxy = TestProxy::new();
    let rlock = RLock::new(proxy.connection_info.clone()).await.unwrap();
    let key = "test:async_late_deadline";

    proxy.arm("SET", key);

    let manager = rlock.clone();
    let task = tokio::spawn(async move {
        manager
            .acquire_mutex_with_options(
                key,
                AcquireOptions::builder()
                    .lock_timeout(Some(Duration::from_millis(200)))
                    .build()
                    .unwrap(),
            )
            .await
    });

    proxy.wait_blocked().await;

    assert!(matches!(task.await.unwrap(), Err(AcquireError::LockTimeout)));

    proxy.resume();

    tokio::task::spawn_blocking(move || assert_key_released(key)).await.unwrap();

    rlock.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[tracing_test::traced_test]
async fn test_late_lease_does_not_remove_new_owner() {
    let proxy = TestProxy::new();
    let rlock = RLock::new(proxy.connection_info.clone()).await.unwrap();
    let key = "test:async_late_lease";

    proxy.arm("SET", key);

    let manager = rlock.clone();
    let task = tokio::spawn(async move {
        manager
            .acquire_mutex_with_options(
                key,
                AcquireOptions::builder()
                    .ttl(Duration::from_millis(150))
                    .renewal_mode(RenewalMode::Disabled)
                    .build()
                    .unwrap(),
            )
            .await
    });

    proxy.wait_blocked().await;

    tokio::time::sleep(Duration::from_millis(250)).await;

    let mut conn = redis::Client::open(REDIS_URL.as_str()).unwrap().get_connection().unwrap();

    assert_eq!(
        Some(String::from("OK")),
        redis::cmd("SET")
            .arg(key)
            .arg("new-owner")
            .arg("NX")
            .arg("PX")
            .arg(2000)
            .query::<Option<String>>(&mut conn)
            .unwrap()
    );

    proxy.resume();

    assert!(matches!(task.await.unwrap(), Err(AcquireError::LeaseExpired)));
    assert_eq!("new-owner", redis::cmd("GET").arg(key).query::<String>(&mut conn).unwrap());

    redis::cmd("DEL").arg(key).exec(&mut conn).unwrap();

    rlock.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[tracing_test::traced_test]
async fn test_canceled_release_is_inactive_and_can_retry() {
    let proxy = TestProxy::new();
    let rlock = RLock::new(proxy.connection_info.clone()).await.unwrap();
    let key = "test:async_canceled_release";

    let mut guard = rlock
        .acquire_mutex_with_options(
            key,
            AcquireOptions::builder().renewal_mode(RenewalMode::Disabled).build().unwrap(),
        )
        .await
        .unwrap();

    proxy.arm("EVALSHA", key);

    {
        let releasing = guard.release();
        tokio::pin!(releasing);

        tokio::select! {
            _ = proxy.wait_blocked() => {},
            result = &mut releasing => panic!("release completed before its reply was resumed: {result:?}"),
        }
    }

    assert!(proxy.is_blocked());
    assert_eq!(LeaseState::Lost, guard.lease_state());
    assert!(!guard.is_released());

    proxy.resume();

    guard.release().await.unwrap();

    assert_eq!(LeaseState::Released, guard.lease_state());

    rlock.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[tracing_test::traced_test]
async fn test_canceled_and_concurrent_shutdown_share_barrier() {
    let proxy = TestProxy::new();
    let rlock = RLock::new(proxy.connection_info.clone()).await.unwrap();
    let key = "test:async_shutdown_barrier";

    let guard = rlock.acquire_mutex(key).await.unwrap();

    proxy.arm("EVALSHA", key);

    drop(guard);

    proxy.wait_blocked().await;

    assert!(tokio::time::timeout(Duration::from_millis(50), rlock.shutdown()).await.is_err());
    assert!(tokio::time::timeout(Duration::from_millis(50), rlock.shutdown()).await.is_err());

    proxy.resume();

    tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(rlock.shutdown(), rlock.shutdown());
    })
    .await
    .unwrap();
}

#[cfg(feature = "sync")]
#[tokio::test]
#[tracing_test::traced_test]
async fn test_sync_async_interoperability() {
    let asynchronous = RLock::new(REDIS_URL.as_str()).await.unwrap();
    let synchronous = crate::sync_lock::RLock::new(REDIS_URL.as_str()).unwrap();

    let key = "test:sync_async_mutex";

    let mut mutex = synchronous.acquire_mutex(key).unwrap();

    assert!(asynchronous.try_acquire_mutex(key).await.unwrap().is_none());

    mutex.release().unwrap();

    let mut mutex = asynchronous.try_acquire_mutex(key).await.unwrap().unwrap();

    assert!(synchronous.try_acquire_mutex(key).unwrap().is_none());

    mutex.release().await.unwrap();

    let key = "test:sync_async_rw";

    let mut reader = synchronous.acquire_read(key).unwrap();
    let mut other = asynchronous.try_acquire_read(key).await.unwrap().unwrap();

    assert!(asynchronous.try_acquire_write(key).await.unwrap().is_none());

    reader.release().unwrap();

    assert!(synchronous.try_acquire_write(key).unwrap().is_none());

    other.release().await.unwrap();

    let mut writer = synchronous.try_acquire_write(key).unwrap().unwrap();

    assert!(asynchronous.try_acquire_read(key).await.unwrap().is_none());

    writer.release().unwrap();

    synchronous.shutdown();
    asynchronous.shutdown().await;
}
