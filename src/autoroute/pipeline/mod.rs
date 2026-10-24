//! The batch routing pipeline (port of `app.freerouting.autoroute.pipeline`):
//! fanout, the autorouter pass loop with board history and fallbacks, and the optimizer.

pub mod batch;
pub mod fallback;
pub mod fanout;
pub mod optimizer;
pub mod stats;

pub use batch::{BatchAutorouter, StopReason};

use crate::board::Board;
use crate::datastructures::StopFlag;
use crate::settings::RouterSettings;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The pipeline stage reported to a [`JobObserver`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobStage {
    Routing,
    Optimizing,
}

/// Progress information passed to a [`JobObserver`].
#[derive(Clone, Copy, Debug)]
pub struct JobProgress {
    pub stage: JobStage,
    /// The current router or optimizer pass (1-based, 0 before the first pass).
    pub pass: i32,
    /// True when called at the end of a pass.
    pub pass_finished: bool,
}

/// Called by the pipeline with the current board while routing (e.g. to update a GUI).
/// Observers only read the board, so they do not influence the routing result.
pub type JobObserver = Arc<dyn Fn(&Board, &JobProgress) + Send + Sync>;

/// The state of a routing job relevant for the pipeline (subset of `RoutingJob`).
pub struct RoutingJob {
    pub board: Board,
    pub settings: RouterSettings,
    /// The routing thread's stop state (`job.thread`).
    pub thread: StopFlag,
    pub timeout_at: Option<Instant>,
    /// `job.state == TIMED_OUT`
    pub timed_out: Arc<AtomicBool>,
    pub current_pass: i32,
    pub observer: Option<JobObserver>,
    pub stage: JobStage,
    pub progress_pass: i32,
}

impl RoutingJob {
    pub fn new(board: Board, settings: RouterSettings) -> RoutingJob {
        RoutingJob {
            board,
            settings,
            thread: StopFlag::new(),
            timeout_at: None,
            timed_out: Arc::new(AtomicBool::new(false)),
            current_pass: 0,
            observer: None,
            stage: JobStage::Routing,
            progress_pass: 0,
        }
    }

    pub fn is_timed_out(&self) -> bool {
        self.timed_out.load(Ordering::SeqCst)
    }

    /// Starts the job timeout monitor (`RoutingJobSchedulerActionThread`): when the
    /// timeout is reached, a stop of the job is requested.
    pub fn start_timeout_monitor(&mut self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        self.timeout_at = Some(deadline);
        let thread = self.thread.clone();
        let timed_out = self.timed_out.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(250));
            if thread.is_stop_requested() && !timed_out.load(Ordering::SeqCst) && Instant::now() < deadline {
                // stopped for another reason; keep watching for the timeout state only
            }
            if Instant::now() >= deadline {
                thread.request_stop();
                timed_out.store(true, Ordering::SeqCst);
                return;
            }
        });
    }

    /// Reports the current board to the observer (if any).
    pub fn notify(&self, pass_finished: bool) {
        if let Some(o) = &self.observer {
            o(
                &self.board,
                &JobProgress {
                    stage: self.stage,
                    pass: self.progress_pass,
                    pass_finished,
                },
            );
        }
    }

    pub fn log_info(&self, msg: &str) {
        log::info!("{msg}");
    }

    pub fn log_debug(&self, msg: &str) {
        log::debug!("{msg}");
    }

    pub fn log_warning(&self, msg: &str) {
        log::warn!("{msg}");
    }
}

/// `TextManager.parseTimespanString`: "HH:MM:SS", "MM:SS" or "SS" in seconds.
pub fn parse_timespan_string(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let parts: Vec<&str> = s.split(':').collect();
    let nums: Option<Vec<f64>> = parts.iter().map(|p| p.trim().parse::<f64>().ok()).collect();
    let nums = nums?;
    let secs = match nums.len() {
        3 => nums[0] * 3600.0 + nums[1] * 60.0 + nums[2],
        2 => nums[0] * 60.0 + nums[1],
        1 => nums[0],
        _ => return None,
    };
    Some(secs.floor() as i64)
}

/// `RoutingPipeline.run()`: routing stage (with fallbacks) and optimization stage.
pub fn run_pipeline(job: &mut RoutingJob) {
    if job.settings.autorouter.algorithm != crate::settings::ALGORITHM_CURRENT {
        job.log_warning(&format!(
            "The algorithm '{}' is not supported. The default algorithm '{}' will be used instead.",
            job.settings.autorouter.algorithm,
            crate::settings::ALGORITHM_CURRENT
        ));
        job.settings.autorouter.algorithm = crate::settings::ALGORITHM_CURRENT.to_string();
    }
    let run_optimizer = job.settings.get_run_optimizer();
    let router_enabled = job.settings.get_run_router() && job.settings.autorouter.max_passes >= 0;
    if router_enabled && !job.thread.is_stop_autorouter_requested() {
        let strategies = fallback::parse(&job.settings, |name| {
            log::warn!(
                "Unknown routing fallback strategy '{}' ignored; known strategies: {}.",
                name,
                fallback::known_strategy_names()
            )
        });
        let input = if strategies.is_empty() { None } else { Some(job.board.snapshot_copy()) };
        let mut router = BatchAutorouter::from_job(job);
        router.run_batch_loop(job);
        let mut runner = fallback::RoutingFallbackRunner::new();
        if let Some(input) = input {
            router = runner.run(job, router, &strategies, input);
        }
        if fallback::explores(&job.settings) {
            router = runner.explore(job, router);
        }
        let _ = router;
    } else if job.settings.is_fanout_enabled() && !job.thread.is_stop_autorouter_requested() {
        let original = job.settings.autorouter.max_passes;
        job.settings.autorouter.max_passes = 0;
        let mut router = BatchAutorouter::from_job(job);
        router.run_batch_loop(job);
        job.settings.autorouter.max_passes = original;
    }
    job.board.finish_autoroute();
    job.notify(true);
    if run_optimizer && !job.thread.is_stop_requested() {
        job.stage = JobStage::Optimizing;
        job.progress_pass = 0;
        let mut opt = optimizer::BatchOptimizer::new(job);
        opt.run_batch_loop(job);
    }
}
