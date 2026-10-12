use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    thread,
    time::Duration,
};

use redis::{Client, IntoConnectionInfo, RedisError};

use super::{
    operations::release_lock,
    pool::{ConnectionPool, PoolOptions},
};
use crate::{AcquireError, lock_request::LockRequest};

const RECYCLE_CHANNEL_BUFFER: usize = 1024;

#[derive(Debug)]
pub(crate) enum ReleaseCommand {
    Release(LockRequest),
    Barrier(mpsc::Sender<()>),
}

#[derive(Debug)]
pub(crate) struct RLockInner {
    pub(crate) pool:       Arc<ConnectionPool>,
    pub(crate) release_tx: Mutex<Option<Arc<SyncSender<ReleaseCommand>>>>,
    is_shutdown:           AtomicBool,
    shutdown_result:       Mutex<Option<bool>>,
    shutdown_done:         Condvar,
    retry_lock:            Mutex<()>,
    retry_wake:            Condvar,
}

/// A manager for creating locks in Redis.
#[derive(Debug, Clone)]
pub struct RLock {
    pub(crate) inner: Arc<RLockInner>,
}

impl RLock {
    /// Initializes a Redis connection pool with the default pool options.
    ///
    /// # Behavior
    ///
    /// 1. Parsing the input Redis URL (`redis_url`) and building a Redis connection pool.
    /// 2. Spawning a background thread (aka the lock release manager) responsible for handling asynchronous lock release requests.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rlock::sync_lock::RLock;
    ///
    /// let rlock = RLock::new("redis://127.0.0.1:6379/0").unwrap();
    /// ```
    #[inline]
    pub fn new(redis_url: impl IntoConnectionInfo) -> Result<Self, RedisError> {
        Self::new_with_pool_options(redis_url, PoolOptions::new())
    }

    /// Initializes a Redis connection pool with the given pool options.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use std::time::Duration;
    ///
    /// use rlock::sync_lock::{PoolOptions, RLock};
    ///
    /// let rlock = RLock::new_with_pool_options(
    ///     "redis://127.0.0.1:6379/0",
    ///     PoolOptions::new()
    ///         .max_idle_connections(4)
    ///         .connection_timeout(Some(Duration::from_secs(3))),
    /// )
    /// .unwrap();
    /// ```
    #[inline]
    pub fn new_with_pool_options(
        redis_url: impl IntoConnectionInfo,
        pool_options: PoolOptions,
    ) -> Result<Self, RedisError> {
        let client = Client::open(redis_url)?;

        Self::with_client(client, pool_options)
    }

    /// See [`RLock::new`].
    pub fn with_client(client: Client, pool_options: PoolOptions) -> Result<Self, RedisError> {
        let pool = Arc::new(ConnectionPool::new(client, pool_options));

        // open one connection immediately so an unreachable Redis server can be detected here, like the async version does
        drop(pool.checkout()?);

        let (request_tx, request_rx) = mpsc::sync_channel(RECYCLE_CHANNEL_BUFFER);
        let pool_for_release = pool.clone();
        thread::Builder::new().name("rlock-release".into()).spawn(move || {
            tracing::trace!("spawned a thread in order to release locks in Redis");

            while let Ok(command) = request_rx.recv() {
                match command {
                    ReleaseCommand::Release(request) => {
                        if let Err(error) = release_lock(&pool_for_release, &request) {
                            tracing::error!(uuid = %request.uuid, "cannot release the lock: {error}");
                        }
                    },
                    ReleaseCommand::Barrier(ack) => { let _ = ack.send(()); },
                }
            }

            tracing::trace!("the lock release thread is stopped");
        }).map_err(RedisError::from)?;
        Ok(Self {
            inner: Arc::new(RLockInner {
                pool,
                release_tx: Mutex::new(Some(Arc::new(request_tx))),
                is_shutdown: AtomicBool::new(false),
                shutdown_result: Mutex::new(None),
                shutdown_done: Condvar::new(),
                retry_lock: Mutex::new(()),
                retry_wake: Condvar::new(),
            }),
        })
    }

    /// Shuts down the RLock manager.
    ///
    /// # Behavior
    ///
    /// Stops new acquisitions and waits for release requests already in the queue.
    /// Existing guards can still renew and release their leases; the release thread ends after its last sender is dropped.
    /// Concurrent calls wait for the same queue barrier.
    /// In-flight Redis commands finish independently, and shutdown cannot interrupt sync Redis I/O already in progress.
    ///
    /// # Panics
    ///
    /// Panics if the lock release manager cannot be stopped normally.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rlock::sync_lock::RLock;
    ///
    /// let rlock = RLock::new("redis://127.0.0.1:6379/0").unwrap();
    ///
    /// // do something
    ///
    /// rlock.shutdown();
    /// ```
    pub fn shutdown(&self) {
        let sender = {
            let mut sender = self.inner.release_tx.lock().unwrap();
            let sender = sender.take();
            self.inner.is_shutdown.store(true, Ordering::Release);
            sender
        };
        {
            let _retry = self.inner.retry_lock.lock().unwrap();
            self.inner.retry_wake.notify_all();
        }
        if let Some(sender) = sender {
            let (ack_tx, ack_rx) = mpsc::channel();
            let complete =
                sender.send(ReleaseCommand::Barrier(ack_tx)).is_ok() && ack_rx.recv().is_ok();
            *self.inner.shutdown_result.lock().unwrap() = Some(complete);
            self.inner.shutdown_done.notify_all();
        }
        let mut result = self.inner.shutdown_result.lock().unwrap();
        while result.is_none() {
            result = self.inner.shutdown_done.wait(result).unwrap();
        }
        assert_eq!(
            Some(true),
            *result,
            "the lock release manager stopped before the shutdown barrier"
        );
    }

    pub(crate) fn admit(&self) -> Result<Arc<SyncSender<ReleaseCommand>>, AcquireError> {
        self.inner.release_tx.lock().unwrap().as_ref().cloned().ok_or(AcquireError::Shutdown)
    }

    pub(crate) fn is_shutdown(&self) -> bool {
        self.inner.is_shutdown.load(Ordering::Acquire)
    }

    pub(crate) fn wait_retry(&self, wait: Duration, canceled: &AtomicBool) {
        let lock = self.inner.retry_lock.lock().unwrap();
        drop(
            self.inner
                .retry_wake
                .wait_timeout_while(lock, wait, |_| {
                    !self.is_shutdown() && !canceled.load(Ordering::Acquire)
                })
                .unwrap(),
        );
    }

    pub(crate) fn cancel_acquisition(&self, canceled: &AtomicBool) {
        let _lock = self.inner.retry_lock.lock().unwrap();
        canceled.store(true, Ordering::Release);
        self.inner.retry_wake.notify_all();
    }
}
