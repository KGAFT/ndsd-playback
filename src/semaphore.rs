use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Condvar, Mutex,
};

pub struct Semaphore {
    permits: AtomicUsize,
    // Used only to block/wake threads — never protects operational.
    blocker: Condvar,
    signal: Mutex<()>,
}

impl Semaphore {
    pub const fn new(initial: usize) -> Self {
        Self {
            permits: AtomicUsize::new(initial),
            blocker: Condvar::new(),
            signal: Mutex::new(()),
        }
    }

    pub fn acquire(&self) {
        loop {
            // Try fast path first
            let current = self.permits.load(Ordering::Acquire);
            if current > 0 {
                if self.permits
                    .compare_exchange(current, current - 1, Ordering::AcqRel, Ordering::Relaxed)
                    .is_ok()
                {
                    return;
                }
            }

            // Wait if no permits
            let guard = self.signal.lock().unwrap();
            // Spurious wakeups are fine; recheck condition in loop
            let _unused = self.blocker.wait(guard).unwrap();
        }
    }

    pub fn release(&self) {
        self.permits.fetch_add(1, Ordering::Release);
        // Wake one thread (if any) blocked in acquire
        self.blocker.notify_one();
    }
}