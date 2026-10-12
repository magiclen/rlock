use std::{
    fmt,
    sync::{Arc, Mutex},
};

use redis::{Client, IntoConnectionInfo, RedisError, aio::ConnectionManager};
use tokio::sync::{mpsc, oneshot, watch};

use super::operations::release_lock;
use crate::{AcquireError, lock_request::LockRequest};

const RECYCLE_CHANNEL_BUFFER: usize = 1024;

#[derive(Debug)]
pub(crate) enum ReleaseCommand {
    Release(LockRequest),
    Barrier(oneshot::Sender<()>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShutdownState {
    Open,
    Closing,
    Completed,
    Failed,
}

pub(crate) type ReleaseSender = Arc<mpsc::Sender<ReleaseCommand>>;

pub(crate) struct RLockInner {
    pub(crate) connection_manager: ConnectionManager,
    pub(crate) admission:          Mutex<Option<ReleaseSender>>,
    pub(crate) shutdown:           Arc<watch::Sender<ShutdownState>>,
}

impl fmt::Debug for RLockInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RLockInner")
            .field("shutdown", &*self.shutdown.borrow())
            .finish_non_exhaustive()
    }
}

/// A manager for creating Redis lock leases using Tokio.
#[derive(Debug, Clone)]
pub struct RLock {
    pub(crate) inner: Arc<RLockInner>,
}

impl RLock {
    /// Connects to Redis and creates a lock manager.
    pub async fn new(redis_url: impl IntoConnectionInfo) -> Result<Self, RedisError> {
        let client = Client::open(redis_url)?;
        Self::with_connection_manager(client.get_connection_manager().await?).await
    }

    /// Creates a lock manager using an existing Redis connection manager.
    pub async fn with_connection_manager(
        connection_manager: ConnectionManager,
    ) -> Result<Self, RedisError> {
        let (request_tx, mut request_rx) = mpsc::channel(RECYCLE_CHANNEL_BUFFER);
        let (shutdown, _) = watch::channel(ShutdownState::Open);
        let shutdown = Arc::new(shutdown);
        let mut release_connection = connection_manager.clone();
        let worker = tokio::spawn(async move {
            while let Some(command) = request_rx.recv().await {
                match command {
                    ReleaseCommand::Release(request) => {
                        if let Err(error) = release_lock(&mut release_connection, &request).await {
                            tracing::error!(uuid = %request.uuid, "failed to release a dropped lock: {error}");
                        }
                    },
                    ReleaseCommand::Barrier(ack) => {
                        let _ = ack.send(());
                    },
                }
            }
        });
        let worker_shutdown = shutdown.clone();
        tokio::spawn(async move {
            if let Err(error) = worker.await {
                tracing::error!("the lock release worker stopped unexpectedly: {error}");
                worker_shutdown.send_if_modified(|state| {
                    if *state == ShutdownState::Completed {
                        return false;
                    }
                    *state = ShutdownState::Failed;
                    true
                });
            }
        });

        Ok(Self {
            inner: Arc::new(RLockInner {
                connection_manager,
                admission: Mutex::new(Some(Arc::new(request_tx))),
                shutdown,
            }),
        })
    }

    pub(crate) fn admit(&self) -> Result<ReleaseSender, AcquireError> {
        let admission = self.inner.admission.lock().unwrap();
        if *self.inner.shutdown.borrow() != ShutdownState::Open {
            return Err(AcquireError::Shutdown);
        }
        admission.as_ref().cloned().ok_or(AcquireError::Shutdown)
    }

    pub(crate) fn is_open(&self) -> bool {
        *self.inner.shutdown.borrow() == ShutdownState::Open
    }

    /// Stops new acquisitions and waits for previously queued releases to finish.
    /// Existing guards can still renew and release their leases after this returns.
    /// Concurrent calls wait for the same shutdown barrier, and cancellation does not stop shutdown.
    ///
    /// # Panics
    ///
    /// Panics if the release worker cannot complete the shutdown barrier.
    pub async fn shutdown(&self) {
        let mut changed = self.inner.shutdown.subscribe();
        {
            let mut admission = self.inner.admission.lock().unwrap();
            if let Some(sender) = admission.take() {
                self.inner.shutdown.send_if_modified(|state| {
                    if *state == ShutdownState::Open {
                        *state = ShutdownState::Closing;
                        true
                    } else {
                        false
                    }
                });
                let completion = self.inner.shutdown.clone();
                tokio::spawn(async move {
                    let (ack_tx, ack_rx) = oneshot::channel();
                    let completed = sender.send(ReleaseCommand::Barrier(ack_tx)).await.is_ok()
                        && ack_rx.await.is_ok();
                    completion.send_if_modified(|state| {
                        if *state == ShutdownState::Failed {
                            return false;
                        }
                        *state = if completed {
                            ShutdownState::Completed
                        } else {
                            ShutdownState::Failed
                        };
                        true
                    });
                });
            }
        }

        loop {
            let state = *changed.borrow_and_update();
            match state {
                ShutdownState::Completed => return,
                ShutdownState::Failed => {
                    panic!("the lock release worker could not complete shutdown")
                },
                ShutdownState::Open | ShutdownState::Closing => {},
            }
            if changed.changed().await.is_err() {
                panic!("the lock shutdown coordinator stopped unexpectedly");
            }
        }
    }
}

pub(crate) async fn wait_for_shutdown(changed: &mut watch::Receiver<ShutdownState>) {
    loop {
        if *changed.borrow_and_update() != ShutdownState::Open {
            return;
        }
        if changed.changed().await.is_err() {
            return;
        }
    }
}
