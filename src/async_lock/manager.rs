use std::{
    fmt::{self, Formatter},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use educe::Educe;
use redis::{aio::ConnectionManager, Client, IntoConnectionInfo, RedisError};
use tokio::{
    sync::{mpsc, Mutex},
    task::JoinHandle,
};

use super::operations::*;
use crate::release_request::{ReleaseRequest, ReleaseRequestKey};

const RECYCLE_CHANNEL_BUFFER: usize = 1024;

#[inline]
fn fmt_connection_manager(_s: &ConnectionManager, f: &mut Formatter<'_>) -> fmt::Result {
    f.write_str("ConnectionManager")
}

#[derive(Educe)]
#[educe(Debug)]
pub(crate) struct RLockInner {
    #[educe(Debug(method(fmt_connection_manager)))]
    pub(crate) connection_manager: ConnectionManager,
    lock_release_manager:          Mutex<Option<JoinHandle<()>>>,
    pub(crate) is_shutdown:        AtomicBool,
}

/// A manager for creating locks in Redis.
#[derive(Debug, Clone)]
pub struct RLock {
    pub(crate) request_tx: Arc<mpsc::Sender<Option<ReleaseRequest>>>,
    pub(crate) inner:      Arc<RLockInner>,
}

impl RLock {
    /// Initializes a Redis connection manager.
    ///
    /// # Behavior
    ///
    /// 1. Parsing the input Redis URL (`redis_url`) and building a Redis connection manager.
    /// 2. Spawning a background task (aka the lock release manager) responsible for handling asynchronous lock release requests.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use rlock::async_lock::RLock;
    ///
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn main() {
    /// let rlock = RLock::new("redis://127.0.0.1:6379/0").await.unwrap();
    /// # }
    /// ```
    #[inline]
    pub async fn new(redis_url: impl IntoConnectionInfo) -> Result<Self, RedisError> {
        let client = Client::open(redis_url)?;
        let connection_manager = client.get_connection_manager().await?;

        Self::with_connection_manager(connection_manager).await
    }

    /// See [`RLock::new`].
    pub async fn with_connection_manager(
        connection_manager: ConnectionManager,
    ) -> Result<Self, RedisError> {
        let (request_tx, mut request_rx) =
            mpsc::channel::<Option<ReleaseRequest>>(RECYCLE_CHANNEL_BUFFER);
        let mut connection_manager_for_lock_release = connection_manager.clone();

        // background task for releasing locks asynchronously
        let lock_release_manager = tokio::spawn(async move {
            tracing::trace!("spawned a task in order to release locks in Redis");

            while let Some(Some(req)) = request_rx.recv().await {
                if let Err(error) = match &req.key {
                    ReleaseRequestKey::Single(key) => {
                        release_lock_async(
                            &mut connection_manager_for_lock_release,
                            key.as_str(),
                            req.uuid.as_str(),
                        )
                        .await
                    },
                    ReleaseRequestKey::Multiple(key) => {
                        release_multi_key_lock_async(
                            &mut connection_manager_for_lock_release,
                            key,
                            req.uuid.as_str(),
                        )
                        .await
                    },
                } {
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
                connection_manager,
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
    /// use rlock::async_lock::RLock;
    ///
    /// # #[tokio::main(flavor = "current_thread")]
    /// # async fn main() {
    /// let rlock = RLock::new("redis://127.0.0.1:6379/0").await.unwrap();
    ///
    /// // do something
    ///
    /// rlock.shutdown().await;
    /// # }
    /// ```
    pub async fn shutdown(&self) {
        if self.inner.is_shutdown.swap(true, Ordering::Relaxed) {
            return;
        }

        if let Some(lock_release_manager) = self.inner.lock_release_manager.lock().await.take() {
            // stop the recv loop
            self.request_tx.send(None).await.unwrap();

            lock_release_manager.await.unwrap();
        } else {
            unreachable!("a lock manager must be taken");
        }
    }
}
