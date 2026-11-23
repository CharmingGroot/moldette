use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::sync::Notify;

/// Bytes one request holds in memory: records handed to the workers and
/// responses not yet taken by the connection. The body is read only while the
/// request holds less than its limit, so a client that does not read the
/// response stops being read from.
pub(crate) struct Budget {
    limit: usize,
    held: AtomicUsize,
    freed: Notify,
}

impl Budget {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            limit,
            held: AtomicUsize::new(0),
            freed: Notify::new(),
        }
    }

    pub(crate) fn hold(&self, bytes: usize) {
        self.held.fetch_add(bytes, Ordering::AcqRel);
    }

    pub(crate) fn release(&self, bytes: usize) {
        self.held.fetch_sub(bytes, Ordering::AcqRel);
        self.freed.notify_one();
    }

    /// Waits until the request holds less than its limit.
    pub(crate) async fn below_limit(&self) {
        while self.held.load(Ordering::Acquire) >= self.limit {
            self.freed.notified().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::time::timeout;

    use super::*;

    #[tokio::test]
    async fn waits_until_enough_is_released() {
        let budget = Arc::new(Budget::new(10));
        budget.hold(4);
        budget.below_limit().await;
        budget.hold(8);
        let waiting = tokio::spawn({
            let budget = Arc::clone(&budget);
            async move { budget.below_limit().await }
        });
        budget.release(2);
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished(), "a request at its limit waits");
        budget.release(1);
        timeout(Duration::from_secs(5), waiting)
            .await
            .expect("released below the limit")
            .expect("task ends");
    }
}
