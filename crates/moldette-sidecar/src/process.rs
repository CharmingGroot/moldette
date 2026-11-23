use std::collections::BTreeMap;
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use http_body_util::BodyExt;
use hyper::body::{Body, Bytes, Frame, Incoming};
use serde_json::json;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::{Instant, timeout_at};

use crate::budget::Budget;
use crate::pool::{Batch, Batcher, Job, Message, RecordError};
use crate::state::State;

/// The streamed body of a `POST /v1/process` response.
pub(crate) struct Output {
    receiver: UnboundedReceiver<Bytes>,
    budget: Arc<Budget>,
}

impl Body for Output {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        let output = self.get_mut();
        output.receiver.poll_recv(cx).map(|bytes| {
            bytes.map(|bytes| {
                output.budget.release(bytes.len());
                Ok(Frame::data(bytes))
            })
        })
    }
}

/// Starts processing `body` and returns the response body that streams the
/// output. `request` holds this request's place among the requests allowed at
/// once until processing ends.
pub(crate) fn start(state: Arc<State>, body: Incoming, request: OwnedSemaphorePermit) -> Output {
    let (sender, receiver) = mpsc::unbounded_channel();
    let budget = Arc::new(Budget::new(state.limits.max_buffer_bytes));
    let output = Output {
        receiver,
        budget: Arc::clone(&budget),
    };
    tokio::spawn(async move {
        run(state, body, Sink { sender, budget }).await;
        drop(request);
    });
    output
}

/// Where the response of one request is written, holding each line in the
/// request's budget until the connection takes it.
struct Sink {
    sender: UnboundedSender<Bytes>,
    budget: Arc<Budget>,
}

impl Sink {
    /// Sends output that is already held; false once the response is dropped.
    fn send_held(&self, bytes: Vec<u8>) -> bool {
        self.sender.send(Bytes::from(bytes)).is_ok()
    }

    fn send_error(&self, record: usize, message: &str) {
        let mut line = json!({ "error": { "record": record, "message": message } }).to_string();
        line.push('\n');
        self.budget.hold(line.len());
        self.send_held(line.into_bytes());
    }
}

async fn run(state: Arc<State>, body: Incoming, sink: Sink) {
    let deadline = Instant::now() + state.limits.timeout;
    let (results, mut received) = mpsc::unbounded_channel();
    let reader = tokio::spawn(read(
        Arc::clone(&state),
        body,
        Arc::clone(&sink.budget),
        results,
    ));
    let mut written = 0;
    let finished = timeout_at(deadline, write_in_order(&mut received, &sink, &mut written)).await;
    // The reader may be waiting for budget that a dropped response never frees.
    reader.abort();
    if finished.is_err() {
        let message = format!(
            "the request took longer than {} seconds",
            state.limits.timeout.as_secs()
        );
        sink.send_error(written + 1, &message);
    }
}

async fn read(
    state: Arc<State>,
    mut body: Incoming,
    budget: Arc<Budget>,
    results: UnboundedSender<Message>,
) {
    let permits = Arc::new(Semaphore::new(state.threads.get() * 2));
    let mut framer = state.framer.clone();
    let mut batcher = Batcher::new();
    let mut received = 0;
    let error = loop {
        budget.below_limit().await;
        let (framed, end) = match body.frame().await {
            None => (framer.finish(&mut |record| batcher.add(record)), true),
            Some(Err(err)) => {
                break Some((None, format!("the request body could not be read: {err}")));
            }
            Some(Ok(frame)) => {
                let Ok(data) = frame.into_data() else {
                    continue;
                };
                received += data.len() as u64;
                if received > state.limits.max_body_bytes {
                    let limit = state.limits.max_body_bytes;
                    break Some((
                        None,
                        format!("the request body is larger than {limit} bytes"),
                    ));
                }
                (framer.push(&data, &mut |record| batcher.add(record)), false)
            }
        };
        if let Err(err) = framed {
            break Some((Some(err.record()), err.kind().to_string()));
        }
        // A client may stream records slowly; what arrived is processed now.
        batcher.close();
        for batch in batcher.take_full() {
            if !submit(&state, &permits, &budget, &results, batch).await {
                return;
            }
        }
        if end {
            break None;
        }
    };
    let error = error.map(|(record, message)| RecordError {
        record: record.unwrap_or_else(|| batcher.next_record()),
        message,
    });
    batcher.close();
    for batch in batcher.take_full() {
        if !submit(&state, &permits, &budget, &results, batch).await {
            return;
        }
    }
    let _ = results.send(Message::End {
        batches: batcher.batches(),
        error,
    });
}

async fn submit(
    state: &State,
    permits: &Arc<Semaphore>,
    budget: &Arc<Budget>,
    results: &UnboundedSender<Message>,
    batch: Batch,
) -> bool {
    let Ok(permit) = Arc::clone(permits).acquire_owned().await else {
        return false;
    };
    budget.hold(batch.len());
    state.pool.submit(Job {
        batch,
        permit,
        budget: Arc::clone(budget),
        results: results.clone(),
    })
}

/// Writes processed batches in input order, then the error that ended the
/// input, if any. `written` counts the records written so far.
async fn write_in_order(
    received: &mut UnboundedReceiver<Message>,
    sink: &Sink,
    written: &mut usize,
) {
    let mut pending = BTreeMap::new();
    let mut next = 0;
    let mut end = None;
    while let Some(message) = received.recv().await {
        match message {
            Message::Processed(processed) => {
                pending.insert(processed.sequence, processed);
            }
            Message::End { batches, error } => end = Some((batches, error)),
        }
        while let Some(processed) = pending.remove(&next) {
            if !sink.send_held(processed.output) {
                return;
            }
            *written += processed.records;
            next += 1;
            if let Some(error) = processed.error {
                sink.send_error(error.record, &error.message);
                return;
            }
        }
        if let Some((batches, error)) = &mut end
            && next == *batches
        {
            if let Some(error) = error.take() {
                sink.send_error(error.record, &error.message);
            }
            return;
        }
    }
}
