use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    thread::{self, JoinHandle},
};

use redis::{Client, IntoConnectionInfo, RedisError};

use super::{
    operations::*,
    pool::{ConnectionPool, PoolOptions},
};
use crate::release_request::{ReleaseRequest, ReleaseRequestKey};

const RECYCLE_CHANNEL_BUFFER: usize = 1024;

#[derive(Debug)]
pub(crate) struct RLockInner {
    pub(crate) pool:        Arc<ConnectionPool>,
    lock_release_manager:   Mutex<Option<JoinHandle<()>>>,
    pub(crate) is_shutdown: AtomicBool,
}

/// A manager for creating locks in Redis.
#[derive(Debug, Clone)]
pub struct RLock {
    pub(crate) request_tx: Arc<SyncSender<Option<ReleaseRequest>>>,
    pub(crate) inner:      Arc<RLockInner>,
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

        let (request_tx, request_rx) =
            mpsc::sync_channel::<Option<ReleaseRequest>>(RECYCLE_CHANNEL_BUFFER);
        let pool_for_lock_release = pool.clone();

        // background thread for releasing locks asynchronously
        let lock_release_manager = thread::spawn(move || {
            tracing::trace!("spawned a thread in order to release locks in Redis");

            while let Ok(Some(req)) = request_rx.recv() {
                if let Err(error) =
                    pool_for_lock_release.checkout().and_then(|mut conn| match &req.key {
                        ReleaseRequestKey::Single(key) => {
                            release_lock_sync(&mut *conn, key.as_str(), req.uuid.as_str())
                        },
                        ReleaseRequestKey::Multiple(key) => {
                            release_multi_key_lock_sync(&mut *conn, key, req.uuid.as_str())
                        },
                        ReleaseRequestKey::Read(key) => {
                            release_read_lock_sync(&mut *conn, key.as_str(), req.uuid.as_str())
                        },
                        ReleaseRequestKey::Write(key) => {
                            release_write_lock_sync(&mut *conn, key.as_str(), req.uuid.as_str())
                        },
                        ReleaseRequestKey::MultipleRead(key) => {
                            release_multi_key_read_lock_sync(&mut *conn, key, req.uuid.as_str())
                        },
                        ReleaseRequestKey::MultipleWrite(key) => {
                            release_multi_key_write_lock_sync(&mut *conn, key, req.uuid.as_str())
                        },
                    })
                {
                    tracing::error!(
                        "an error occured when releasing the lock {uuid}: {error}",
                        uuid = req.uuid
                    );
                }
            }
        });

        Ok(Self {
            request_tx: Arc::new(request_tx),
            inner:      Arc::new(RLockInner {
                pool,
                lock_release_manager: Mutex::new(Some(lock_release_manager)),
                is_shutdown: AtomicBool::new(false),
            }),
        })
    }

    /// Shuts down the RLock manager.
    ///
    /// # Behavior
    ///
    /// 1. Marking the manager as shutdown.
    /// 2. Stopping the lock release manager.
    /// 3. Waiting for the lock release manager to be stopped.
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
        if self.inner.is_shutdown.swap(true, Ordering::Relaxed) {
            return;
        }

        if let Some(lock_release_manager) = self.inner.lock_release_manager.lock().unwrap().take() {
            // stop the recv loop
            self.request_tx.send(None).unwrap();

            lock_release_manager.join().unwrap();
        } else {
            unreachable!("a lock manager must be taken");
        }
    }
}
