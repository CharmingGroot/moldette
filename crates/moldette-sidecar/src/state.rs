use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

use moldette::Pipeline;
use moldette::framing::Framer;
use tokio::sync::Semaphore;

use crate::pool::Pool;

pub(crate) struct Limits {
    pub(crate) max_body_bytes: u64,
    pub(crate) timeout: Duration,
    pub(crate) max_requests: usize,
}

pub(crate) struct State {
    pub(crate) pool: Pool,
    pub(crate) threads: NonZeroUsize,
    pub(crate) framer: Framer,
    pub(crate) limits: Limits,
    pub(crate) requests: Arc<Semaphore>,
}

impl State {
    pub(crate) fn new(
        pipeline: Pipeline,
        threads: NonZeroUsize,
        framer: Framer,
        limits: Limits,
    ) -> Self {
        Self {
            pool: Pool::new(pipeline, threads),
            threads,
            framer,
            requests: Arc::new(Semaphore::new(limits.max_requests)),
            limits,
        }
    }
}
