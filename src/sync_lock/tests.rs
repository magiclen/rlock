use std::{
    sync::{Arc, LazyLock, Mutex},
    thread,
    time::Duration,
};

use super::*;
use crate::{AcquireError, AcquireOptions};

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
    static mut COUNTER: u32 = 0;

    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    fn counter_increase(rlock: RLock) {
        let lock = rlock.acquire_mutex("test:sync_mutex_critical_section").unwrap();

        // ----- critical section -----

        unsafe { COUNTER += 1 };

        // ----------------------------

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

    assert_eq!(100, unsafe { COUNTER });

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
    static mut COUNTER: u32 = 0;

    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    fn counter_increase(rlock: RLock) {
        let lock = rlock.acquire_write("test:sync_write_lock_critical_section").unwrap();

        // ----- critical section -----

        unsafe { COUNTER += 1 };

        // ----------------------------

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

    assert_eq!(100, unsafe { COUNTER });

    rlock.shutdown();
}

#[test]
#[tracing_test::traced_test]
fn test_rw_lock_exclusion() {
    static mut COUNTER: u32 = 0;

    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    fn writer(rlock: RLock) {
        let lock = rlock.acquire_write("test:sync_rw_lock_exclusion").unwrap();

        // ----- critical section -----

        // the counter is odd only inside a held write lock
        unsafe { COUNTER += 1 };

        thread::sleep(Duration::from_millis(1));

        unsafe { COUNTER += 1 };

        // ----------------------------

        drop(lock);
    }

    fn reader(rlock: RLock) {
        let lock = rlock.acquire_read("test:sync_rw_lock_exclusion").unwrap();

        // ----- shared section -----

        // a reader observing an odd counter proves that the write exclusion is broken
        assert_eq!(0, unsafe { COUNTER } % 2);

        // --------------------------

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

    assert_eq!(100, unsafe { COUNTER });

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
    static mut COUNTER: u32 = 0;

    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    fn counter_increase(rlock: RLock, i: u32) {
        let lock = rlock
            .acquire_multi_key_write(Arc::new(vec![
                String::from("test:sync_multi_key_write_lock_critical_section"),
                format!("test:sync_multi_key_write_lock_critical_section_{i}"),
            ]))
            .unwrap();

        // ----- critical section -----

        unsafe { COUNTER += 1 };

        // ----------------------------

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

    assert_eq!(100, unsafe { COUNTER });

    rlock.shutdown();
}

#[test]
#[tracing_test::traced_test]
fn test_multi_key_mutex_critical_section() {
    static mut COUNTER: u32 = 0;

    let rlock = RLock::new(REDIS_URL.as_str()).unwrap();

    fn counter_increase(rlock: RLock, i: u32) {
        let lock = rlock
            .acquire_multi_key_mutex(Arc::new(vec![
                String::from("test:sync_multi_key_mutex_critical_section"),
                format!("test:sync_multi_key_mutex_critical_section_{i}"),
            ]))
            .unwrap();

        // ----- critical section -----

        unsafe { COUNTER += 1 };

        // ----------------------------

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

    assert_eq!(100, unsafe { COUNTER });

    rlock.shutdown();
}
