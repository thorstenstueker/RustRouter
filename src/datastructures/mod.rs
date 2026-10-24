//! Generic data structures (port of `app.freerouting.datastructures`).

pub mod shape_tree;

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The stop request state of a routing thread (`StoppableThread` of the original):
/// a stop of everything or of the autorouter only.
#[derive(Clone, Debug, Default)]
pub struct StopFlag(Arc<AtomicU8>);

const STOP_NONE: u8 = 0;
const STOP_AUTOROUTER_ONLY: u8 = 1;
const STOP_ALL: u8 = 2;

impl StopFlag {
    pub fn new() -> StopFlag {
        StopFlag::default()
    }

    /// `requestStop`
    pub fn request_stop(&self) {
        self.0.store(STOP_ALL, Ordering::SeqCst);
    }

    /// `isStopRequested`: true only for a stop of everything.
    pub fn is_stop_requested(&self) -> bool {
        self.0.load(Ordering::SeqCst) == STOP_ALL
    }

    /// `requestStopAutoRouter`
    pub fn request_stop_autorouter(&self) {
        let _ = self.0.compare_exchange(STOP_NONE, STOP_AUTOROUTER_ONLY, Ordering::SeqCst, Ordering::SeqCst);
    }

    /// `isStopAutoRouterRequested`: true for any stop request.
    pub fn is_stop_autorouter_requested(&self) -> bool {
        self.0.load(Ordering::SeqCst) != STOP_NONE
    }

    /// `resetAutoRouterStopRequest`
    pub fn reset_autorouter_stop_request(&self) {
        let _ = self.0.compare_exchange(STOP_AUTOROUTER_ONLY, STOP_NONE, Ordering::SeqCst, Ordering::SeqCst);
    }

    pub fn reset(&self) {
        self.0.store(STOP_NONE, Ordering::SeqCst);
    }
}

/// A time limit in milliseconds (`TimeLimit` of the original).
#[derive(Clone, Copy, Debug)]
pub struct TimeLimit {
    start: Instant,
    limit: Duration,
}

impl TimeLimit {
    pub fn new(milliseconds: u64) -> TimeLimit {
        TimeLimit {
            start: Instant::now(),
            limit: Duration::from_millis(milliseconds),
        }
    }

    pub fn limit_exceeded(&self) -> bool {
        self.start.elapsed() > self.limit
    }

    pub fn restart(&mut self) {
        self.start = Instant::now();
    }
}
