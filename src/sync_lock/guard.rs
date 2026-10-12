use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, RecvTimeoutError, Sender},
    },
    thread,
    time::Instant,
};

use uuid::Uuid;

use super::{
    RLock,
    manager::ReleaseCommand,
    operations::{acquire_lock, release_lock, renew_lock},
};
use crate::{
    AcquireError, AcquireOptions, LeaseState, ReleaseError,
    lease::Lease,
    lock_keys::debug_assert_lock_keys,
    lock_request::{LockRequest, LockTarget},
};

#[derive(Debug)]
pub(crate) struct GuardData {
    rlock:                  RLock,
    request:                LockRequest,
    release_tx:             Option<Arc<mpsc::SyncSender<ReleaseCommand>>>,
    pub(crate) is_released: bool,
    pub(crate) lease:       Arc<Lease>,
    stop_tx:                Option<Sender<()>>,
}

struct Acquisition {
    guard:    GuardData,
    accepted: Sender<()>,
}

impl GuardData {
    fn new(
        rlock: RLock,
        request: LockRequest,
        release_tx: Arc<mpsc::SyncSender<ReleaseCommand>>,
        lease: Arc<Lease>,
        start: Instant,
        options: &AcquireOptions,
    ) -> Result<Self, AcquireError> {
        let mut guard = Self {
            rlock,
            request,
            release_tx: Some(release_tx),
            is_released: false,
            lease,
            stop_tx: None,
        };
        if let Some(interval) = options.effective_renew_interval() {
            let (stop_tx, stop_rx) = mpsc::channel();
            let pool = guard.rlock.inner.pool.clone();
            let request = guard.request.clone();
            let lease = guard.lease.clone();
            let ttl = options.effective_ttl();
            let renewal = thread::Builder::new().name("rlock-renew".into()).spawn(move || {
                let mut last_start = start;
                loop {
                    let wait = interval.saturating_sub(last_start.elapsed()).min(lease.remaining());
                    match stop_rx.recv_timeout(wait) {
                        Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                        Err(RecvTimeoutError::Timeout) => {},
                    }
                    if lease.state() != LeaseState::Active {
                        break;
                    }
                    let attempt = Instant::now();
                    match renew_lock(&pool, &request, ttl) {
                        Ok(true) if lease.renew(attempt, ttl) => last_start = attempt,
                        Ok(_) => {
                            lease.lose();
                            break;
                        },
                        Err(error) => {
                            lease.lose();
                            tracing::error!(uuid = %request.uuid, "cannot renew the lock: {error}");
                            break;
                        },
                    }
                }
            });
            guard.stop_tx = Some(stop_tx);
            renewal.map_err(AcquireError::WorkerError)?;
        }
        Ok(guard)
    }

    pub(crate) fn release(&mut self) -> Result<(), ReleaseError> {
        if self.is_released {
            return Ok(());
        }
        self.lease.lose();
        self.stop_tx.take();
        release_lock(&self.rlock.inner.pool, &self.request)?;
        self.is_released = true;
        self.lease.release();
        self.release_tx.take();
        Ok(())
    }
}

impl Drop for GuardData {
    fn drop(&mut self) {
        self.lease.release();
        self.stop_tx.take();
        if !self.is_released
            && let Some(sender) = self.release_tx.take()
            && let Err(error) = sender.try_send(ReleaseCommand::Release(self.request.clone()))
        {
            tracing::warn!(uuid = %self.request.uuid, "cannot queue lock release; the lease will expire: {error}");
        }
    }
}

impl RLock {
    pub(crate) fn acquire_internal(
        &self,
        target: LockTarget,
        options: AcquireOptions,
        once: bool,
    ) -> Result<Option<GuardData>, AcquireError> {
        debug_assert_lock_keys(target.keys());

        let start = Instant::now();
        if options.lock_timeout().is_some_and(|timeout| timeout.is_zero()) {
            return Err(AcquireError::LockTimeout);
        }
        let release_tx = self.admit()?;
        let request = LockRequest {
            target,
            uuid: Arc::new(Uuid::new_v4().to_string()),
        };
        let canceled = Arc::new(AtomicBool::new(false));
        let result = if let Some(timeout) = options.lock_timeout() {
            let (result_tx, result_rx) = mpsc::sync_channel(1);
            let worker = self.clone();
            let worker_canceled = canceled.clone();
            thread::Builder::new().name("rlock-acquire".into()).spawn(move || {
                let cleanup = request.clone();
                let result = worker.acquire_loop(request, options, once, start, &worker_canceled, release_tx);
                if let Ok(Some(guard)) = result {
                    let (accepted, accepted_rx) = mpsc::channel();
                    let sent = result_tx.send(Ok(Some(Acquisition { guard, accepted }))).is_ok();
                    if (!sent || accepted_rx.recv().is_err()) && let Err(error) = release_lock(&worker.inner.pool, &cleanup) {
                        tracing::error!(uuid = %cleanup.uuid, "cannot clean up an unclaimed acquisition: {error}");
                    }
                } else {
                    let _ = result_tx.send(result.map(|_| None));
                }
            }).map_err(AcquireError::WorkerError)?;
            match result_rx.recv_timeout(timeout.saturating_sub(start.elapsed())) {
                Ok(result) => {
                    if start.elapsed() >= timeout {
                        self.cancel_acquisition(&canceled);
                        drop(result);
                        return Err(AcquireError::LockTimeout);
                    }
                    if self.is_shutdown() {
                        drop(result);
                        return Err(AcquireError::Shutdown);
                    }
                    if let Ok(Some(acquisition)) = &result
                        && acquisition.guard.lease.state() != LeaseState::Active
                    {
                        drop(result);
                        return Err(AcquireError::LeaseExpired);
                    }
                    result.map(|acquisition| {
                        acquisition.map(|acquisition| {
                            let _ = acquisition.accepted.send(());
                            acquisition.guard
                        })
                    })
                },
                Err(RecvTimeoutError::Timeout) => {
                    self.cancel_acquisition(&canceled);
                    return Err(AcquireError::LockTimeout);
                },
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(AcquireError::WorkerError(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "the lock worker stopped before returning a result",
                    )));
                },
            }
        } else {
            self.acquire_loop(request, options, once, start, &canceled, release_tx)
        };
        if let Ok(Some(guard)) = &result
            && guard.lease.state() != LeaseState::Active
        {
            drop(result);
            return Err(AcquireError::LeaseExpired);
        }
        result
    }

    fn acquire_loop(
        &self,
        request: LockRequest,
        options: AcquireOptions,
        once: bool,
        start: Instant,
        canceled: &AtomicBool,
        release_tx: Arc<mpsc::SyncSender<ReleaseCommand>>,
    ) -> Result<Option<GuardData>, AcquireError> {
        let ttl = options.effective_ttl();
        loop {
            if canceled.load(Ordering::Acquire)
                || options.lock_timeout().is_some_and(|timeout| start.elapsed() >= timeout)
            {
                return Err(AcquireError::LockTimeout);
            }
            if self.is_shutdown() {
                return Err(AcquireError::Shutdown);
            }
            let attempt = Instant::now();
            let acquired = acquire_lock(&self.inner.pool, &request, ttl)?;
            if acquired {
                let lease = Lease::new(attempt, ttl);
                let admission = self.inner.release_tx.lock().unwrap();
                let failure = if admission.is_none() {
                    Some(AcquireError::Shutdown)
                } else if canceled.load(Ordering::Acquire)
                    || options.lock_timeout().is_some_and(|timeout| start.elapsed() >= timeout)
                {
                    Some(AcquireError::LockTimeout)
                } else if lease.state() != LeaseState::Active {
                    Some(AcquireError::LeaseExpired)
                } else {
                    None
                };
                if let Some(error) = failure {
                    drop(admission);
                    if let Err(error) = release_lock(&self.inner.pool, &request) {
                        tracing::error!(uuid = %request.uuid, "cannot clean up a late acquisition: {error}");
                    }
                    return Err(error);
                }
                let guard =
                    GuardData::new(self.clone(), request, release_tx, lease, attempt, &options)?;
                drop(admission);
                return Ok(Some(guard));
            }
            if self.is_shutdown() {
                return Err(AcquireError::Shutdown);
            }
            if options.lock_timeout().is_some_and(|timeout| start.elapsed() >= timeout) {
                return Err(AcquireError::LockTimeout);
            }
            if once {
                return Ok(None);
            }
            let wait = options.lock_timeout().map_or(options.retry_interval(), |timeout| {
                options.retry_interval().min(timeout.saturating_sub(start.elapsed()))
            });
            self.wait_retry(wait, canceled);
        }
    }
}
