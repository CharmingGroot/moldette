use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use tokio::sync::Semaphore;

use crate::metrics::Metrics;
use crate::pool::Pool;
use crate::targets::Targets;

pub(crate) struct Limits {
    pub(crate) max_body_bytes: u64,
    pub(crate) max_buffer_bytes: usize,
    pub(crate) timeout: Duration,
    pub(crate) max_requests: usize,
}

pub(crate) struct State {
    pub(crate) pool: Pool,
    pub(crate) threads: NonZeroUsize,
    pub(crate) targets: Targets,
    pub(crate) metrics: Metrics,
    pub(crate) limits: Limits,
    pub(crate) requests: Arc<Semaphore>,
    /// Whether `/readyz` answers 200: from the start, or once the
    /// configuration source is first read.
    pub(crate) ready: AtomicBool,
}

impl State {
    pub(crate) fn new(
        threads: NonZeroUsize,
        targets: Targets,
        metrics: Metrics,
        limits: Limits,
        ready: bool,
    ) -> Self {
        Self {
            pool: Pool::new(threads),
            threads,
            targets,
            metrics,
            requests: Arc::new(Semaphore::new(limits.max_requests)),
            limits,
            ready: AtomicBool::new(ready),
        }
    }
}
