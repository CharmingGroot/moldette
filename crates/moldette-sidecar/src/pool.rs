use std::mem;
use std::num::NonZeroUsize;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;

use moldette::Pipeline;
use moldette_spec::json::JsonResponse;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::mpsc::UnboundedSender;

const BATCH_RECORDS: usize = 256;
const BATCH_BYTES: usize = 1 << 20;

/// Records of one request, processed together by one worker thread.
pub(crate) struct Batch {
    sequence: usize,
    first_record: usize,
    text: String,
    ends: Vec<usize>,
}

pub(crate) struct Job {
    pub(crate) batch: Batch,
    pub(crate) permit: OwnedSemaphorePermit,
    pub(crate) results: UnboundedSender<Message>,
}

pub(crate) enum Message {
    Processed(Processed),
    /// The request body ended after `batches` batches, or failed at `error`.
    End {
        batches: usize,
        error: Option<RecordError>,
    },
}

pub(crate) struct Processed {
    pub(crate) sequence: usize,
    pub(crate) output: Vec<u8>,
    pub(crate) records: usize,
    pub(crate) error: Option<RecordError>,
    /// Held until the output is written, which bounds the batches of a
    /// request in memory.
    _permit: OwnedSemaphorePermit,
}

pub(crate) struct RecordError {
    pub(crate) record: usize,
    pub(crate) message: String,
}

/// Worker threads shared by all requests; the async runtime only moves bytes.
pub(crate) struct Pool {
    sender: Sender<Job>,
}

impl Pool {
    pub(crate) fn new(pipeline: Pipeline, threads: NonZeroUsize) -> Self {
        let (sender, receiver) = channel::<Job>();
        let receiver = Arc::new(Mutex::new(receiver));
        for _ in 0..threads.get() {
            let receiver = Arc::clone(&receiver);
            let pipeline = pipeline.clone();
            thread::spawn(move || work(&pipeline, &receiver));
        }
        Self { sender }
    }

    pub(crate) fn submit(&self, job: Job) -> bool {
        self.sender.send(job).is_ok()
    }
}

fn work(pipeline: &Pipeline, receiver: &Mutex<Receiver<Job>>) {
    loop {
        let job = match receiver.lock() {
            Ok(receiver) => receiver.recv(),
            Err(_) => return,
        };
        let Ok(Job {
            batch,
            permit,
            results,
        }) = job
        else {
            return;
        };
        let (output, error) = process(pipeline, &batch);
        let _ = results.send(Message::Processed(Processed {
            sequence: batch.sequence,
            output,
            records: batch.ends.len(),
            error,
            _permit: permit,
        }));
    }
}

fn process(pipeline: &Pipeline, batch: &Batch) -> (Vec<u8>, Option<RecordError>) {
    let mut output = Vec::with_capacity(batch.text.len() * 2);
    let mut start = 0;
    for (index, &end) in batch.ends.iter().enumerate() {
        let response = pipeline.process(&batch.text[start..end]);
        start = end;
        if let Err(err) = serde_json::to_writer(&mut output, &JsonResponse::new(&response)) {
            return (
                output,
                Some(RecordError {
                    record: batch.first_record + index,
                    message: err.to_string(),
                }),
            );
        }
        output.push(b'\n');
    }
    (output, None)
}

/// Collects framed records of one request into batches.
pub(crate) struct Batcher {
    sequence: usize,
    next_record: usize,
    text: String,
    ends: Vec<usize>,
    full: Vec<Batch>,
}

impl Batcher {
    pub(crate) fn new() -> Self {
        Self {
            sequence: 0,
            next_record: 1,
            text: String::new(),
            ends: Vec::new(),
            full: Vec::new(),
        }
    }

    pub(crate) fn add(&mut self, record: &str) {
        self.text.push_str(record);
        self.ends.push(self.text.len());
        if self.ends.len() >= BATCH_RECORDS || self.text.len() >= BATCH_BYTES {
            self.close();
        }
    }

    /// Closes the batch being filled, if it holds a record.
    pub(crate) fn close(&mut self) {
        if self.ends.is_empty() {
            return;
        }
        let batch = Batch {
            sequence: self.sequence,
            first_record: self.next_record,
            text: mem::take(&mut self.text),
            ends: mem::take(&mut self.ends),
        };
        self.sequence += 1;
        self.next_record += batch.ends.len();
        self.full.push(batch);
    }

    pub(crate) fn take_full(&mut self) -> Vec<Batch> {
        mem::take(&mut self.full)
    }

    /// Batches made so far, the one being filled excluded.
    pub(crate) fn batches(&self) -> usize {
        self.sequence
    }

    /// The number of the next record, counting from 1.
    pub(crate) fn next_record(&self) -> usize {
        self.next_record + self.ends.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sizes(records: &[&str]) -> Vec<(usize, usize)> {
        let mut batcher = Batcher::new();
        for record in records {
            batcher.add(record);
        }
        batcher.close();
        batcher
            .take_full()
            .iter()
            .map(|batch| (batch.first_record, batch.ends.len()))
            .collect()
    }

    #[test]
    fn batches_close_at_the_record_limit() {
        let records = vec!["{}"; BATCH_RECORDS * 2 + 1];
        assert_eq!(
            sizes(&records),
            [
                (1, BATCH_RECORDS),
                (BATCH_RECORDS + 1, BATCH_RECORDS),
                (BATCH_RECORDS * 2 + 1, 1)
            ]
        );
    }

    #[test]
    fn batches_close_at_the_byte_limit() {
        let long = "x".repeat(BATCH_BYTES / 2);
        assert_eq!(sizes(&["{}", &long, &long, "{}"]), [(1, 3), (4, 1)]);
    }

    #[test]
    fn the_next_record_counts_the_batch_being_filled() {
        let mut batcher = Batcher::new();
        batcher.add("a");
        batcher.add("b");
        assert_eq!((batcher.batches(), batcher.next_record()), (0, 3));
        batcher.close();
        assert_eq!((batcher.batches(), batcher.next_record()), (1, 3));
    }
}
