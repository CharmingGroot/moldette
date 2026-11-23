use std::collections::BTreeMap;
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use http_body_util::BodyExt;
use hyper::body::{Body, Bytes, Frame, Incoming};
use serde_json::json;
use tokio::sync::mpsc::{self, Receiver, Sender, UnboundedReceiver, UnboundedSender};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::{Instant, timeout_at};

use crate::pool::{Batcher, Job, Message, RecordError};
use crate::state::State;

/// Batches of one request queued for the response: enough to keep every
/// worker busy while earlier output is written.
const OUTPUT_BATCHES: usize = 4;

/// The streamed body of a `POST /v1/process` response.
pub(crate) struct Output(Receiver<Bytes>);

impl Body for Output {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        self.get_mut()
            .0
            .poll_recv(cx)
            .map(|bytes| bytes.map(|bytes| Ok(Frame::data(bytes))))
    }
}

/// Starts processing `body` and returns the response body that streams the
/// output. `request` holds this request's place among the requests allowed at
/// once until processing ends.
pub(crate) fn start(state: Arc<State>, body: Incoming, request: OwnedSemaphorePermit) -> Output {
    let (sender, receiver) = mpsc::channel(OUTPUT_BATCHES);
    tokio::spawn(async move {
        run(state, body, sender).await;
        drop(request);
    });
    Output(receiver)
}

async fn run(state: Arc<State>, body: Incoming, output: Sender<Bytes>) {
    let deadline = Instant::now() + state.limits.timeout;
    let (results, mut received) = mpsc::unbounded_channel();
    let reader = tokio::spawn(read(Arc::clone(&state), body, results));
    let mut written = 0;
    let finished = timeout_at(
        deadline,
        write_in_order(&mut received, &output, &mut written),
    )
    .await;
    if finished.is_err() {
        reader.abort();
        let message = format!(
            "the request took longer than {} seconds",
            state.limits.timeout.as_secs()
        );
        let _ = output.try_send(error_line(written + 1, &message));
    }
}

async fn read(state: Arc<State>, mut body: Incoming, results: UnboundedSender<Message>) {
    let permits = Arc::new(Semaphore::new(state.threads.get() * 2));
    let mut framer = state.framer.clone();
    let mut batcher = Batcher::new();
    let mut received = 0;
    let error = loop {
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
            if !submit(&state, &permits, &results, batch).await {
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
        if !submit(&state, &permits, &results, batch).await {
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
    results: &UnboundedSender<Message>,
    batch: crate::pool::Batch,
) -> bool {
    let Ok(permit) = Arc::clone(permits).acquire_owned().await else {
        return false;
    };
    state.pool.submit(Job {
        batch,
        permit,
        results: results.clone(),
    })
}

/// Writes processed batches in input order, then the error that ended the
/// input, if any. `written` counts the records written so far.
async fn write_in_order(
    received: &mut UnboundedReceiver<Message>,
    output: &Sender<Bytes>,
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
            if output.send(Bytes::from(processed.output)).await.is_err() {
                return;
            }
            *written += processed.records;
            next += 1;
            if let Some(error) = processed.error {
                let _ = output.send(error_line(error.record, &error.message)).await;
                return;
            }
        }
        if let Some((batches, error)) = &mut end
            && next == *batches
        {
            if let Some(error) = error.take() {
                let _ = output.send(error_line(error.record, &error.message)).await;
            }
            return;
        }
    }
}

fn error_line(record: usize, message: &str) -> Bytes {
    let mut line = json!({ "error": { "record": record, "message": message } }).to_string();
    line.push('\n');
    Bytes::from(line)
}
