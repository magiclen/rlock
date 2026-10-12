use std::{
    fmt, future, io,
    sync::Arc,
    time::{Duration, Instant},
};

use redis::aio::ConnectionManager;
use tokio::sync::{mpsc::error::TrySendError, oneshot};
use uuid::Uuid;

use super::{
    RLock,
    manager::{ReleaseCommand, ReleaseSender, wait_for_shutdown},
    operations::{acquire_lock, release_lock, renew_lock},
};
use crate::{
    AcquireError, AcquireOptions, LeaseState, ReleaseError,
    lease::Lease,
    lock_keys::debug_assert_lock_keys,
    lock_request::{LockRequest, LockTarget},
};

pub(crate) struct GuardData {
    connection:             ConnectionManager,
    request:                LockRequest,
    pub(crate) lease:       Arc<Lease>,
    sender:                 Option<ReleaseSender>,
    stop_tx:                Option<oneshot::Sender<()>>,
    pub(crate) is_released: bool,
}

impl fmt::Debug for GuardData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GuardData")
            .field("request", &self.request)
            .field("lease", &self.lease)
            .field("is_released", &self.is_released)
            .finish_non_exhaustive()
    }
}

impl GuardData {
    fn new(
        connection: ConnectionManager,
        request: LockRequest,
        lease: Arc<Lease>,
        sender: ReleaseSender,
        attempt_start: Instant,
        options: &AcquireOptions,
    ) -> Self {
        let stop_tx = options.effective_renew_interval().map(|interval| {
            let (stop_tx, mut stop_rx) = oneshot::channel();
            let mut connection = connection.clone();
            let request = request.clone();
            let lease = lease.clone();
            let ttl = options.effective_ttl();
            let retry_interval = options.retry_interval();
            tokio::spawn(async move {
                tracing::trace!(uuid = %request.uuid, "spawned a task in order to automatically renew the lock");

                let mut wait_start = attempt_start;
                let mut wait = interval;
                loop {
                    tokio::select! {
                        biased;
                        _ = &mut stop_rx => {
                            tracing::trace!(uuid = %request.uuid, "the renewal task is stopped because the lock is being released");
                            break;
                        },
                        _ = lease.wait_until_inactive() => {
                            tracing::trace!(uuid = %request.uuid, "the renewal task is stopped because the lease is no longer active");
                            break;
                        },
                        _ = wait_for_deadline(wait_start, Some(wait)) => {},
                    }
                    if lease.state() != LeaseState::Active {
                        tracing::trace!(uuid = %request.uuid, "the renewal task is stopped because the lease is no longer active");
                        break;
                    }
                    let start = Instant::now();
                    match renew_lock(&mut connection, &request, ttl).await {
                        Ok(true) if lease.renew(start, ttl) => {
                            tracing::trace!(uuid = %request.uuid, "renewed the lock");
                            wait_start = start;
                            wait = interval;
                        },
                        Ok(_) => {
                            tracing::trace!(uuid = %request.uuid, "the renewal task is stopped because the lease is lost");
                            lease.lose();
                            break;
                        },
                        Err(error) => {
                            // the lease stays valid until its deadline, so retry before the remaining time runs out
                            tracing::warn!(uuid = %request.uuid, "failed to renew the lock, retrying: {error}");
                            wait_start = Instant::now();
                            wait = retry_interval.min(lease.remaining() / 2);
                        },
                    }
                }
            });
            stop_tx
        });

        Self {
            connection,
            request,
            lease,
            sender: Some(sender),
            stop_tx,
            is_released: false,
        }
    }

    fn stop_renewal(&mut self) {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
        }
    }

    pub(crate) async fn release(&mut self) -> Result<(), ReleaseError> {
        if self.is_released {
            return Ok(());
        }
        self.lease.lose();
        self.stop_renewal();

        let mut connection = self.connection.clone();
        let request = self.request.clone();
        let (result_tx, result_rx) = oneshot::channel();
        tokio::spawn(async move {
            let _ = result_tx.send(release_lock(&mut connection, &request).await);
        });
        result_rx.await.map_err(|_| {
            ReleaseError::WorkerError(io::Error::other(
                "the lock release task stopped before returning a result",
            ))
        })??;
        self.is_released = true;
        self.lease.release();
        self.sender.take();
        Ok(())
    }
}

impl Drop for GuardData {
    fn drop(&mut self) {
        if self.is_released {
            return;
        }
        self.lease.release();
        self.stop_renewal();
        if let Some(sender) = self.sender.take()
            && let Err(error) = sender.try_send(ReleaseCommand::Release(self.request.clone()))
        {
            match error {
                TrySendError::Full(_) => {
                    tracing::warn!(uuid = %self.request.uuid, "the lock release queue is full; the lease will expire by its TTL")
                },
                TrySendError::Closed(_) => {
                    tracing::warn!(uuid = %self.request.uuid, "the lock release worker is unavailable; the lease will expire by its TTL")
                },
            }
        }
    }
}

fn deadline_elapsed(start: Instant, timeout: Option<Duration>) -> bool {
    timeout.is_some_and(|timeout| start.elapsed() >= timeout)
}

async fn wait_for_deadline(start: Instant, timeout: Option<Duration>) {
    let Some(timeout) = timeout else {
        future::pending::<()>().await;
        return;
    };
    loop {
        let remaining = timeout.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            return;
        }
        // Short timer segments also support durations beyond Tokio's timer range.
        tokio::time::sleep(remaining.min(Duration::from_secs(86_400))).await;
    }
}

async fn clean_abandoned(connection: &mut ConnectionManager, request: &LockRequest) {
    if let Err(error) = release_lock(connection, request).await {
        tracing::error!(uuid = %request.uuid, "failed to clean up an abandoned acquisition: {error}");
    }
}

struct Acquisition {
    guard:    GuardData,
    accepted: oneshot::Sender<()>,
}

enum Handoff {
    Sent(oneshot::Receiver<()>),
    Rejected(AcquireError),
    Abandoned(GuardData),
}

impl RLock {
    pub(crate) async fn acquire_internal(
        &self,
        target: LockTarget,
        options: AcquireOptions,
        once: bool,
    ) -> Result<Option<GuardData>, AcquireError> {
        debug_assert_lock_keys(target.keys());

        let start = Instant::now();
        let timeout = options.lock_timeout();
        if deadline_elapsed(start, timeout) {
            return Err(AcquireError::LockTimeout);
        }
        let sender = self.admit()?;
        let mut changed = self.inner.shutdown.subscribe();
        let request = LockRequest {
            target,
            uuid: Arc::new(Uuid::new_v4().to_string()),
        };
        let (result_tx, result_rx) = oneshot::channel();
        let manager = self.clone();
        tokio::spawn(async move {
            manager.supervise_acquisition(request, options, once, start, sender, result_tx).await;
        });

        let result = tokio::select! {
            biased;
            _ = wait_for_shutdown(&mut changed) => return Err(AcquireError::Shutdown),
            _ = wait_for_deadline(start, timeout) => return Err(AcquireError::LockTimeout),
            result = result_rx => result.map_err(|_| AcquireError::WorkerError(io::Error::other("the acquisition task stopped before returning a result")))?,
        }?;
        if deadline_elapsed(start, timeout) {
            return Err(AcquireError::LockTimeout);
        }
        if !self.is_open() {
            return Err(AcquireError::Shutdown);
        }
        if let Some(acquisition) = &result
            && acquisition.guard.lease.state() != LeaseState::Active
        {
            return Err(AcquireError::LeaseExpired);
        }
        Ok(result.map(|acquisition| {
            let _ = acquisition.accepted.send(());
            acquisition.guard
        }))
    }

    async fn supervise_acquisition(
        &self,
        request: LockRequest,
        options: AcquireOptions,
        once: bool,
        start: Instant,
        sender: ReleaseSender,
        result_tx: oneshot::Sender<Result<Option<Acquisition>, AcquireError>>,
    ) {
        let mut result_tx = Some(result_tx);
        let mut connection = self.inner.connection_manager.clone();
        let mut changed = self.inner.shutdown.subscribe();
        let timeout = options.lock_timeout();
        let ttl = options.effective_ttl();
        loop {
            if result_tx.as_ref().unwrap().is_closed() {
                return;
            }
            if !self.is_open() {
                let _ = result_tx.take().unwrap().send(Err(AcquireError::Shutdown));
                return;
            }
            if deadline_elapsed(start, timeout) {
                let _ = result_tx.take().unwrap().send(Err(AcquireError::LockTimeout));
                return;
            }

            let attempt_start = Instant::now();
            // Keep the command alive until its result is known before attempting cleanup.
            let acquired = match acquire_lock(&mut connection, &request, ttl).await {
                Ok(acquired) => acquired,
                Err(error) => {
                    let _ = result_tx.take().unwrap().send(Err(error.into()));
                    clean_abandoned(&mut connection, &request).await;
                    return;
                },
            };
            if acquired {
                let lease = Lease::new(attempt_start, ttl);
                let handoff = {
                    let admission = self.inner.admission.lock().unwrap();
                    if admission.is_none() || !self.is_open() {
                        Handoff::Rejected(AcquireError::Shutdown)
                    } else if deadline_elapsed(start, timeout) {
                        Handoff::Rejected(AcquireError::LockTimeout)
                    } else if lease.state() != LeaseState::Active {
                        Handoff::Rejected(AcquireError::LeaseExpired)
                    } else {
                        let guard = GuardData::new(
                            connection.clone(),
                            request.clone(),
                            lease,
                            sender.clone(),
                            attempt_start,
                            &options,
                        );
                        let (accepted, accepted_rx) = oneshot::channel();
                        match result_tx.take().unwrap().send(Ok(Some(Acquisition {
                            guard,
                            accepted,
                        }))) {
                            Ok(()) => Handoff::Sent(accepted_rx),
                            Err(Ok(Some(acquisition))) => Handoff::Abandoned(acquisition.guard),
                            _ => unreachable!("the acquisition result contains a guard"),
                        }
                    }
                };
                match handoff {
                    Handoff::Sent(accepted) => {
                        if accepted.await.is_err() {
                            clean_abandoned(&mut connection, &request).await;
                        }
                    },
                    Handoff::Rejected(error) => {
                        let _ = result_tx.take().unwrap().send(Err(error));
                        clean_abandoned(&mut connection, &request).await;
                    },
                    Handoff::Abandoned(mut guard) => {
                        if let Err(error) = guard.release().await {
                            tracing::error!(uuid = %request.uuid, "failed to release an unclaimed guard: {error}");
                        }
                    },
                }
                return;
            }
            if once {
                let _ = result_tx.take().unwrap().send(Ok(None));
                return;
            }

            let retry_start = Instant::now();
            tokio::select! {
                biased;
                _ = result_tx.as_mut().unwrap().closed() => return,
                _ = wait_for_shutdown(&mut changed) => {
                    let _ = result_tx.take().unwrap().send(Err(AcquireError::Shutdown));
                    return;
                },
                _ = wait_for_deadline(start, timeout) => {
                    let _ = result_tx.take().unwrap().send(Err(AcquireError::LockTimeout));
                    return;
                },
                _ = wait_for_deadline(retry_start, Some(options.retry_interval())) => {},
            }
        }
    }
}
