use std::{
    sync::{Arc, LazyLock, Mutex},
    thread,
    time::{Duration, Instant},
};

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

// use a lock timeout instead of tokio::time::timeout in the async tests to detect a leaked lock which is caused by a missing shutdown
fn acquire_options_with_lock_timeout() -> AcquireOptions {
    AcquireOptions::builder().lock_timeout(Some(Duration::from_secs(3))).build().unwrap()
}

fn test_rlock_new_and_shutdown_1_2() {
    let global_lock = GLOBAL_LOCK_1_2.lock().unwrap();

    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    let lock = rlock
        .acquire_mutex_with_options(
            "test:sync_rlock_new_and_shutdown_1_2",
            acquire_options_with_lock_timeout(),
        )
        .unwrap();

    drop(lock);

    rlock.shutdown(); // if the shutdown method is not called, one of the test case between `test_rlock_new_and_shutdown_1` and `test_rlock_new_and_shutdown_2` will panic because of timeout

    drop(global_lock);
}

#[test]
#[tracing_test::traced_test]
fn test_rlock_new_and_shutdown_1() {
    test_rlock_new_and_shutdown_1_2();
}

#[test]
#[tracing_test::traced_test]
fn test_rlock_new_and_shutdown_2() {
    test_rlock_new_and_shutdown_1_2();
}

fn test_rlock_new_and_shutdown_3_4(n: u32) {
    let global_lock = GLOBAL_LOCK_3_4.lock().unwrap();

    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    let lock = rlock
        .acquire_multi_key_mutex_with_options(
            Arc::new(vec![
                String::from("test:sync_rlock_new_and_shutdown_3_4"),
                format!("test:sync_rlock_new_and_shutdown_{n}"),
            ]),
            acquire_options_with_lock_timeout(),
        )
        .unwrap();

    drop(lock);

    rlock.shutdown(); // if the shutdown method is not called, one of the test case between `test_rlock_new_and_shutdown_3` and `test_rlock_new_and_shutdown_4` will panic because of timeout

    drop(global_lock);
}

#[test]
#[tracing_test::traced_test]
fn test_rlock_new_and_shutdown_3() {
    test_rlock_new_and_shutdown_3_4(3);
}

#[test]
#[tracing_test::traced_test]
fn test_rlock_new_and_shutdown_4() {
    test_rlock_new_and_shutdown_3_4(4);
}

#[test]
#[tracing_test::traced_test]
fn test_acquire_mutex() {
    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    let lock = rlock.acquire_mutex("test:sync_acquire_mutex").unwrap();

    drop(lock);

    rlock.shutdown();
}

#[test]
#[tracing_test::traced_test]
fn test_release_mutex() {
    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    let mut lock = rlock.acquire_mutex("test:sync_release_mutex").unwrap();

    lock.release().unwrap();

    rlock.shutdown();
}

#[test]
#[tracing_test::traced_test]
fn test_acquire_multi_key_mutex() {
    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    let lock = rlock
        .acquire_multi_key_mutex(Arc::new(vec![String::from("test:sync_acquire_multi_key_mutex")]))
        .unwrap();

    drop(lock);

    rlock.shutdown();
}

#[test]
#[tracing_test::traced_test]
fn test_release_multi_key_mutex() {
    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    let mut lock = rlock
        .acquire_multi_key_mutex(Arc::new(vec![String::from("test:sync_release_multi_key_mutex")]))
        .unwrap();

    lock.release().unwrap();

    rlock.shutdown();
}

#[test]
#[tracing_test::traced_test]
fn test_mutex_critical_section() {
    static COUNTER: TestState = TestState::new();

    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    fn counter_increase(rlock: RLock) {
        let lock = rlock.acquire_mutex("test:sync_mutex_critical_section").unwrap();

        let holder = COUNTER.writer();

        // ----- critical section -----

        COUNTER.increment();

        // ----------------------------

        drop(holder);
        drop(lock);
    }

    let mut threads = Vec::with_capacity(100);

    for _ in 0..100 {
        let rlock = rlock.clone();

        threads.push(thread::spawn(move || counter_increase(rlock)));
    }

    for thread in threads {
        thread.join().unwrap();
    }

    assert_eq!(100, COUNTER.value());

    rlock.shutdown();
}

#[test]
#[tracing_test::traced_test]
fn test_acquire_read() {
    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    let lock = rlock.acquire_read("test:sync_acquire_read").unwrap();

    drop(lock);

    rlock.shutdown();
}

#[test]
#[tracing_test::traced_test]
fn test_release_read() {
    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    let mut lock = rlock.acquire_read("test:sync_release_read").unwrap();

    lock.release().unwrap();

    rlock.shutdown();
}

#[test]
#[tracing_test::traced_test]
fn test_acquire_write() {
    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    let lock = rlock.acquire_write("test:sync_acquire_write").unwrap();

    drop(lock);

    rlock.shutdown();
}

#[test]
#[tracing_test::traced_test]
fn test_release_write() {
    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    let mut lock = rlock.acquire_write("test:sync_release_write").unwrap();

    lock.release().unwrap();

    rlock.shutdown();
}

#[test]
#[tracing_test::traced_test]
fn test_read_locks_are_shared() {
    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    let lock_1 = rlock.acquire_read("test:sync_read_locks_are_shared").unwrap();

    // if read locks were wrongly exclusive, this acquisition would spin on retries and hit the lock timeout
    let lock_2 = rlock
        .acquire_read_with_options(
            "test:sync_read_locks_are_shared",
            acquire_options_with_lock_timeout(),
        )
        .unwrap();

    drop(lock_1);
    drop(lock_2);

    rlock.shutdown();
}

#[test]
#[tracing_test::traced_test]
fn test_write_lock_excluded_by_read_lock() {
    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    let mut read_lock = rlock.acquire_read("test:sync_write_lock_excluded_by_read_lock").unwrap();

    let result = rlock.acquire_write_with_options(
        "test:sync_write_lock_excluded_by_read_lock",
        AcquireOptions::builder().lock_timeout(Some(Duration::from_secs(1))).build().unwrap(),
    );

    assert!(matches!(result, Err(AcquireError::LockTimeout)));

    read_lock.release().unwrap();

    let write_lock = rlock
        .acquire_write_with_options(
            "test:sync_write_lock_excluded_by_read_lock",
            acquire_options_with_lock_timeout(),
        )
        .unwrap();

    drop(write_lock);

    rlock.shutdown();
}

#[test]
#[tracing_test::traced_test]
fn test_read_lock_excluded_by_write_lock() {
    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    let mut write_lock = rlock.acquire_write("test:sync_read_lock_excluded_by_write_lock").unwrap();

    let result = rlock.acquire_read_with_options(
        "test:sync_read_lock_excluded_by_write_lock",
        AcquireOptions::builder().lock_timeout(Some(Duration::from_secs(1))).build().unwrap(),
    );

    assert!(matches!(result, Err(AcquireError::LockTimeout)));

    write_lock.release().unwrap();

    let read_lock = rlock
        .acquire_read_with_options(
            "test:sync_read_lock_excluded_by_write_lock",
            acquire_options_with_lock_timeout(),
        )
        .unwrap();

    drop(read_lock);

    rlock.shutdown();
}

#[test]
#[tracing_test::traced_test]
fn test_write_lock_critical_section() {
    static COUNTER: TestState = TestState::new();

    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    fn counter_increase(rlock: RLock) {
        let lock = rlock.acquire_write("test:sync_write_lock_critical_section").unwrap();

        let holder = COUNTER.writer();

        // ----- critical section -----

        COUNTER.increment();

        // ----------------------------

        drop(holder);
        drop(lock);
    }

    let mut threads = Vec::with_capacity(100);

    for _ in 0..100 {
        let rlock = rlock.clone();

        threads.push(thread::spawn(move || counter_increase(rlock)));
    }

    for thread in threads {
        thread.join().unwrap();
    }

    assert_eq!(100, COUNTER.value());

    rlock.shutdown();
}

#[test]
#[tracing_test::traced_test]
fn test_rw_lock_exclusion() {
    static COUNTER: TestState = TestState::new();

    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    fn writer(rlock: RLock) {
        let lock = rlock.acquire_write("test:sync_rw_lock_exclusion").unwrap();

        let holder = COUNTER.writer();

        // ----- critical section -----

        // the counter is odd only inside a held write lock
        COUNTER.increment();

        thread::sleep(Duration::from_millis(1));

        COUNTER.increment();

        // ----------------------------

        drop(holder);
        drop(lock);
    }

    fn reader(rlock: RLock) {
        let lock = rlock.acquire_read("test:sync_rw_lock_exclusion").unwrap();

        let holder = COUNTER.reader();

        // ----- shared section -----

        // a reader observing an odd counter proves that the write exclusion is broken
        assert_eq!(0, COUNTER.value() % 2);

        // --------------------------

        drop(holder);
        drop(lock);
    }

    let mut threads = Vec::with_capacity(100);

    for _ in 0..50 {
        let rlock_for_writer = rlock.clone();
        let rlock_for_reader = rlock.clone();

        threads.push(thread::spawn(move || writer(rlock_for_writer)));
        threads.push(thread::spawn(move || reader(rlock_for_reader)));
    }

    for thread in threads {
        thread.join().unwrap();
    }

    assert_eq!(100, COUNTER.value());

    rlock.shutdown();
}

#[test]
#[tracing_test::traced_test]
fn test_acquire_multi_key_read() {
    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    let lock = rlock
        .acquire_multi_key_read(Arc::new(vec![String::from("test:sync_acquire_multi_key_read")]))
        .unwrap();

    drop(lock);

    rlock.shutdown();
}

#[test]
#[tracing_test::traced_test]
fn test_release_multi_key_read() {
    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    let mut lock = rlock
        .acquire_multi_key_read(Arc::new(vec![String::from("test:sync_release_multi_key_read")]))
        .unwrap();

    lock.release().unwrap();

    rlock.shutdown();
}

#[test]
#[tracing_test::traced_test]
fn test_acquire_multi_key_write() {
    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    let lock = rlock
        .acquire_multi_key_write(Arc::new(vec![String::from("test:sync_acquire_multi_key_write")]))
        .unwrap();

    drop(lock);

    rlock.shutdown();
}

#[test]
#[tracing_test::traced_test]
fn test_release_multi_key_write() {
    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    let mut lock = rlock
        .acquire_multi_key_write(Arc::new(vec![String::from("test:sync_release_multi_key_write")]))
        .unwrap();

    lock.release().unwrap();

    rlock.shutdown();
}

#[test]
#[tracing_test::traced_test]
fn test_multi_key_write_lock_critical_section() {
    static COUNTER: TestState = TestState::new();

    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    fn counter_increase(rlock: RLock, i: u32) {
        let lock = rlock
            .acquire_multi_key_write(Arc::new(vec![
                String::from("test:sync_multi_key_write_lock_critical_section"),
                format!("test:sync_multi_key_write_lock_critical_section_{i}"),
            ]))
            .unwrap();

        let holder = COUNTER.writer();

        // ----- critical section -----

        COUNTER.increment();

        // ----------------------------

        drop(holder);
        drop(lock);
    }

    let mut threads = Vec::with_capacity(100);

    for i in 0..100 {
        let rlock = rlock.clone();

        threads.push(thread::spawn(move || counter_increase(rlock, i)));
    }

    for thread in threads {
        thread.join().unwrap();
    }

    assert_eq!(100, COUNTER.value());

    rlock.shutdown();
}

#[test]
#[tracing_test::traced_test]
fn test_multi_key_mutex_critical_section() {
    static COUNTER: TestState = TestState::new();

    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    fn counter_increase(rlock: RLock, i: u32) {
        let lock = rlock
            .acquire_multi_key_mutex(Arc::new(vec![
                String::from("test:sync_multi_key_mutex_critical_section"),
                format!("test:sync_multi_key_mutex_critical_section_{i}"),
            ]))
            .unwrap();

        let holder = COUNTER.writer();

        // ----- critical section -----

        COUNTER.increment();

        // ----------------------------

        drop(holder);
        drop(lock);
    }

    let mut threads = Vec::with_capacity(100);

    for i in 0..100 {
        let rlock = rlock.clone();

        threads.push(thread::spawn(move || counter_increase(rlock, i)));
    }

    for thread in threads {
        thread.join().unwrap();
    }

    assert_eq!(100, COUNTER.value());

    rlock.shutdown();
}

#[test]
#[tracing_test::traced_test]
fn test_lease_renewal_and_shutdown_with_live_guard() {
    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    let mut guard = rlock
        .acquire_mutex_with_options(
            "test:sync_live_shutdown",
            AcquireOptions::builder().ttl(Duration::from_millis(300)).build().unwrap(),
        )
        .unwrap();

    rlock.shutdown();

    thread::sleep(Duration::from_millis(800));

    assert_eq!(LeaseState::Active, guard.lease_state());
    assert!(matches!(rlock.acquire_mutex("test:sync_closed"), Err(AcquireError::Shutdown)));

    guard.release().unwrap();

    assert_eq!(LeaseState::Released, guard.lease_state());
    assert!(guard.is_released());

    rlock.shutdown();
}

#[test]
#[tracing_test::traced_test]
fn test_disabled_renewal_expires() {
    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    let mut guard = rlock
        .acquire_read_with_options(
            "test:sync_no_renewal",
            AcquireOptions::builder()
                .ttl(Duration::from_millis(150))
                .renewal_mode(RenewalMode::Disabled)
                .build()
                .unwrap(),
        )
        .unwrap();

    thread::sleep(Duration::from_millis(250));

    assert_eq!(LeaseState::Lost, guard.lease_state());

    guard.release().unwrap();

    rlock.shutdown();
}

#[test]
#[tracing_test::traced_test]
fn test_try_acquisition_and_reusable_keys() {
    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    let mut mutex = rlock.try_acquire_mutex("test:sync_try_mutex").unwrap().unwrap();

    assert!(rlock.try_acquire_mutex("test:sync_try_mutex").unwrap().is_none());

    mutex.release().unwrap();

    let keys = LockKeys::new(["test:sync_try:a", "test:sync_try:b"]).unwrap();

    let mut read = rlock.try_acquire_multi_key_read(&keys).unwrap().unwrap();
    let mut other = rlock.try_acquire_read("test:sync_try:a").unwrap().unwrap();

    assert!(rlock.try_acquire_multi_key_write(&keys).unwrap().is_none());

    other.release().unwrap();
    read.release().unwrap();

    let mut write = rlock.try_acquire_multi_key_write(&keys).unwrap().unwrap();

    assert!(rlock.try_acquire_write("test:sync_try:a").unwrap().is_none());
    assert!(rlock.try_acquire_multi_key_read(&keys).unwrap().is_none());

    write.release().unwrap();

    let mut multi = rlock.try_acquire_multi_key_mutex(&keys).unwrap().unwrap();

    assert!(rlock.try_acquire_mutex("test:sync_try:a").unwrap().is_none());

    multi.release().unwrap();

    let options = AcquireOptions::builder().lock_timeout(Some(Duration::ZERO)).build().unwrap();

    assert!(matches!(
        rlock.try_acquire_mutex_with_options("test:sync_zero", options),
        Err(AcquireError::LockTimeout)
    ));

    rlock.shutdown();
}

#[test]
#[tracing_test::traced_test]
fn test_deadline_cleans_up_late_success() {
    let proxy = TestProxy::new();
    let rlock = RLock::new(proxy.connection_info.clone()).unwrap();
    let key = "test:sync_late_deadline";

    proxy.arm("SET", key);

    let start = Instant::now();
    let result = rlock.acquire_mutex_with_options(
        key,
        AcquireOptions::builder().lock_timeout(Some(Duration::from_millis(200))).build().unwrap(),
    );

    assert!(matches!(result, Err(AcquireError::LockTimeout)));
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(proxy.is_blocked());

    proxy.resume();

    assert_key_released(key);

    rlock.shutdown();
}

#[test]
#[tracing_test::traced_test]
fn test_retry_wait_does_not_exceed_deadline() {
    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    let mut guard = rlock.acquire_mutex("test:sync_retry_deadline").unwrap();

    let start = Instant::now();
    let result = rlock.acquire_mutex_with_options(
        "test:sync_retry_deadline",
        AcquireOptions::builder()
            .retry_interval(Duration::from_secs(2))
            .lock_timeout(Some(Duration::from_millis(100)))
            .build()
            .unwrap(),
    );

    assert!(matches!(result, Err(AcquireError::LockTimeout)));
    assert!(start.elapsed() < Duration::from_secs(1));

    guard.release().unwrap();

    rlock.shutdown();
}
