use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::time::Instant;

#[derive(Clone)]
pub struct ActivityTracker {
    state: Arc<ActivityState>,
}

struct ActivityState {
    in_flight: AtomicUsize,
    last_completed: Mutex<Instant>,
    idle_timeout: Duration,
}

pub struct CallGuard {
    state: Arc<ActivityState>,
}

impl ActivityTracker {
    pub fn new(idle_timeout: Duration) -> Self {
        Self {
            state: Arc::new(ActivityState {
                in_flight: AtomicUsize::new(0),
                last_completed: Mutex::new(Instant::now()),
                idle_timeout,
            }),
        }
    }

    pub fn enter(&self) -> CallGuard {
        self.state.in_flight.fetch_add(1, Ordering::AcqRel);
        CallGuard {
            state: Arc::clone(&self.state),
        }
    }

    pub fn is_idle(&self) -> bool {
        self.state.in_flight.load(Ordering::Acquire) == 0
            && self.state.last_completed.lock().unwrap().elapsed() >= self.state.idle_timeout
    }
}

impl Drop for CallGuard {
    fn drop(&mut self) {
        *self.state.last_completed.lock().unwrap() = Instant::now();
        self.state.in_flight.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_is_idle_only_after_last_call_and_timeout() {
        let tracker = ActivityTracker::new(Duration::from_millis(10));
        let guard = tracker.enter();
        std::thread::sleep(Duration::from_millis(15));
        assert!(!tracker.is_idle());
        drop(guard);
        assert!(!tracker.is_idle());
        std::thread::sleep(Duration::from_millis(15));
        assert!(tracker.is_idle());
    }
}
