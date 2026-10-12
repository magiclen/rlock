use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// The local state of a lock lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaseState {
    /// The lease is within its last confirmed, conservative deadline.
    Active,
    /// The lease has expired or can no longer be trusted.
    Lost,
    /// Explicit release completed or the guard was dropped.
    Released,
}

#[derive(Debug)]
struct LeaseData {
    state: LeaseState,
    start: Instant,
    ttl:   Duration,
}

#[derive(Debug)]
pub(crate) struct Lease {
    data:    Mutex<LeaseData>,
    #[cfg(feature = "async")]
    changed: tokio::sync::watch::Sender<()>,
}

impl Lease {
    pub(crate) fn new(start: Instant, ttl: Duration) -> Arc<Self> {
        Arc::new(Self {
            data:                              Mutex::new(LeaseData {
                state: LeaseState::Active,
                start,
                ttl,
            }),
            #[cfg(feature = "async")]
            changed:                           tokio::sync::watch::channel(()).0,
        })
    }

    fn notify(&self) {
        #[cfg(feature = "async")]
        self.changed.send_replace(());
    }

    fn refresh(&self, data: &mut LeaseData) {
        if data.state == LeaseState::Active && data.start.elapsed() >= data.ttl {
            data.state = LeaseState::Lost;
            self.notify();
        }
    }

    pub(crate) fn state(&self) -> LeaseState {
        let mut data = self.data.lock().unwrap();
        self.refresh(&mut data);
        data.state
    }

    pub(crate) fn remaining(&self) -> Duration {
        let mut data = self.data.lock().unwrap();
        self.refresh(&mut data);
        if data.state == LeaseState::Active {
            data.ttl.saturating_sub(data.start.elapsed())
        } else {
            Duration::ZERO
        }
    }

    pub(crate) fn renew(&self, start: Instant, ttl: Duration) -> bool {
        let mut data = self.data.lock().unwrap();
        self.refresh(&mut data);
        if data.state != LeaseState::Active {
            return false;
        }
        if start.elapsed() >= ttl {
            data.state = LeaseState::Lost;
            self.notify();
            return false;
        }
        data.start = start;
        data.ttl = ttl;
        self.notify();
        true
    }

    pub(crate) fn lose(&self) {
        let mut data = self.data.lock().unwrap();
        if data.state == LeaseState::Active {
            data.state = LeaseState::Lost;
            self.notify();
        }
    }

    pub(crate) fn release(&self) {
        self.data.lock().unwrap().state = LeaseState::Released;
        self.notify();
    }

    #[cfg(feature = "async")]
    pub(crate) async fn wait_until_inactive(&self) -> LeaseState {
        let mut changed = self.changed.subscribe();
        loop {
            let state = self.state();
            if state != LeaseState::Active {
                return state;
            }
            tokio::select! {
                _ = changed.changed() => {},
                _ = tokio::time::sleep(self.remaining().min(Duration::from_secs(86_400))) => {},
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lease_transitions() {
        let lease = Lease::new(Instant::now(), Duration::from_secs(1));

        assert_eq!(LeaseState::Active, lease.state());
        assert!(lease.renew(Instant::now(), Duration::from_secs(1)));

        lease.lose();

        assert_eq!(LeaseState::Lost, lease.state());
        assert!(!lease.renew(Instant::now(), Duration::from_secs(1)));

        lease.release();

        assert_eq!(LeaseState::Released, lease.state());
    }

    #[test]
    fn test_expired_lease() {
        let lease = Lease::new(Instant::now() - Duration::from_secs(1), Duration::from_millis(100));

        assert_eq!(LeaseState::Lost, lease.state());
        assert_eq!(Duration::ZERO, lease.remaining());
        assert!(!lease.renew(Instant::now(), Duration::from_secs(1)));
    }
}
