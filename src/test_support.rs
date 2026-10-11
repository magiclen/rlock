use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};

pub(crate) struct TestState {
    value:   AtomicU32,
    holders: AtomicI32,
}

impl TestState {
    pub(crate) const fn new() -> Self {
        Self {
            value: AtomicU32::new(0), holders: AtomicI32::new(0)
        }
    }

    pub(crate) fn writer(&self) -> TestHolder<'_> {
        assert_eq!(Ok(0), self.holders.compare_exchange(0, -1, Ordering::SeqCst, Ordering::SeqCst));
        TestHolder {
            state: self, writer: true
        }
    }

    pub(crate) fn reader(&self) -> TestHolder<'_> {
        let mut holders = self.holders.load(Ordering::SeqCst);
        loop {
            assert!(holders >= 0);
            match self.holders.compare_exchange(
                holders,
                holders + 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => break,
                Err(current) => holders = current,
            }
        }
        TestHolder {
            state: self, writer: false
        }
    }

    pub(crate) fn increment(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn value(&self) -> u32 {
        self.value.load(Ordering::Relaxed)
    }
}

pub(crate) struct TestHolder<'a> {
    state:  &'a TestState,
    writer: bool,
}

impl Drop for TestHolder<'_> {
    fn drop(&mut self) {
        if self.writer {
            assert_eq!(-1, self.state.holders.swap(0, Ordering::SeqCst));
        } else {
            assert!(self.state.holders.fetch_sub(1, Ordering::SeqCst) > 0);
        }
    }
}
