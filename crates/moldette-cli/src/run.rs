use std::collections::BTreeMap;
use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, ErrorKind, Read, Write};
use std::mem;
use std::num::NonZeroUsize;
use std::path::Path;
use std::process::ExitCode;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;

use flate2::bufread::MultiGzDecoder;
use moldette::framing::Framer;
use moldette::{Explain, Pipeline, Response};
use moldette_spec::json::JsonResponse;

use crate::load;

const BATCH_RECORDS: usize = 256;
const BATCH_BYTES: usize = 1 << 20;
const READ_BYTES: usize = 64 << 10;

pub(crate) enum Mode {
    Run,
    Explain,
}

struct Batch {
    sequence: usize,
    first_record: usize,
    text: String,
    ends: Vec<usize>,
}

struct Processed {
    sequence: usize,
    output: Vec<u8>,
    counts: Counts,
    error: Option<InputError>,
}

struct InputError {
    record: usize,
    message: String,
}

pub(crate) fn run(
    config: &Path,
    input: Option<&Path>,
    framer: Framer,
    mode: Mode,
    threads: NonZeroUsize,
) -> ExitCode {
    let specs = match load::specs(config) {
        Ok(specs) => specs,
        Err(code) => return code,
    };
    let pipeline = match Pipeline::from_specs(&specs) {
        Ok(pipeline) => pipeline,
        Err(err) => {
            eprintln!("error: {}: {err}", config.display());
            return ExitCode::FAILURE;
        }
    };
    let (reader, name): (Box<dyn Read + Send>, String) = match input {
        Some(path) => match File::open(path) {
            Ok(file) if path.extension().is_some_and(|extension| extension == "gz") => (
                Box::new(MultiGzDecoder::new(BufReader::new(file))),
                path.display().to_string(),
            ),
            Ok(file) => (Box::new(file), path.display().to_string()),
            Err(err) => {
                eprintln!("error: {}: {err}", path.display());
                return ExitCode::FAILURE;
            }
        },
        None => (Box::new(io::stdin()), "standard input".to_owned()),
    };
    let mut output = BufWriter::new(io::stdout().lock());
    let (written, read) = thread::scope(|scope| {
        let (batch_sender, batch_receiver) = sync_channel(threads.get() * 2);
        let (processed_sender, processed_receiver) = sync_channel(threads.get() * 2);
        let batch_receiver = Arc::new(Mutex::new(batch_receiver));
        let reader = scope.spawn(move || read_batches(reader, framer, &batch_sender));
        for _ in 0..threads.get() {
            let batch_receiver = Arc::clone(&batch_receiver);
            let processed_sender = processed_sender.clone();
            let (pipeline, mode) = (pipeline.clone(), &mode);
            scope.spawn(move || {
                process_batches(&pipeline, mode, &batch_receiver, &processed_sender);
            });
        }
        drop((batch_receiver, processed_sender));
        let written = write_in_order(&mut output, processed_receiver);
        (written, reader.join())
    });
    let (counts, processing_error) = match written {
        Ok(written) => written,
        Err(err) => return write_failure(&err),
    };
    let error = match read {
        Ok(read_error) => processing_error.or(read_error),
        Err(_) => {
            eprintln!("error: {name}: the reading thread stopped");
            return ExitCode::FAILURE;
        }
    };
    match error {
        None => {
            eprintln!("{counts}");
            ExitCode::SUCCESS
        }
        Some(InputError { record, message }) => {
            eprintln!("error: {name}: record {record}: {message}");
            ExitCode::FAILURE
        }
    }
}

fn read_batches(
    mut reader: Box<dyn Read + Send>,
    mut framer: Framer,
    sender: &SyncSender<Batch>,
) -> Option<InputError> {
    let mut batcher = Batcher::new(sender);
    let mut chunk = vec![0; READ_BYTES];
    loop {
        let read = match reader.read(&mut chunk) {
            Ok(read) => read,
            Err(err) if err.kind() == ErrorKind::Interrupted => continue,
            Err(err) => return batcher.fail(None, err.to_string()),
        };
        let framed = if read == 0 {
            framer.finish(&mut |record| batcher.add(record))
        } else {
            framer.push(&chunk[..read], &mut |record| batcher.add(record))
        };
        if batcher.closed {
            return None;
        }
        if let Err(err) = framed {
            return batcher.fail(Some(err.record()), err.kind().to_string());
        }
        if read == 0 {
            batcher.send();
            return None;
        }
    }
}

struct Batcher<'a> {
    sender: &'a SyncSender<Batch>,
    sequence: usize,
    next_record: usize,
    text: String,
    ends: Vec<usize>,
    closed: bool,
}

impl<'a> Batcher<'a> {
    fn new(sender: &'a SyncSender<Batch>) -> Self {
        Self {
            sender,
            sequence: 0,
            next_record: 1,
            text: String::new(),
            ends: Vec::new(),
            closed: false,
        }
    }

    fn add(&mut self, record: &str) {
        if self.closed {
            return;
        }
        self.text.push_str(record);
        self.ends.push(self.text.len());
        if self.ends.len() >= BATCH_RECORDS || self.text.len() >= BATCH_BYTES {
            self.send();
        }
    }

    fn send(&mut self) {
        if self.ends.is_empty() || self.closed {
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
        self.closed = self.sender.send(batch).is_err();
    }

    fn fail(&mut self, record: Option<usize>, message: String) -> Option<InputError> {
        self.send();
        Some(InputError {
            record: record.unwrap_or(self.next_record),
            message,
        })
    }
}

fn process_batches(
    pipeline: &Pipeline,
    mode: &Mode,
    receiver: &Mutex<Receiver<Batch>>,
    sender: &SyncSender<Processed>,
) {
    loop {
        let batch = match receiver.lock() {
            Ok(receiver) => receiver.recv(),
            Err(_) => return,
        };
        let Ok(batch) = batch else {
            return;
        };
        if sender.send(process_batch(pipeline, mode, &batch)).is_err() {
            return;
        }
    }
}

fn process_batch(pipeline: &Pipeline, mode: &Mode, batch: &Batch) -> Processed {
    let mut output = Vec::with_capacity(batch.text.len() * 2);
    let mut counts = Counts::default();
    let mut error = None;
    let mut start = 0;
    for (index, &end) in batch.ends.iter().enumerate() {
        let record = &batch.text[start..end];
        start = end;
        let written = match mode {
            Mode::Run => {
                let response = pipeline.process(record);
                counts.add(&response);
                write_line(&mut output, &response, None)
            }
            Mode::Explain => {
                let (response, explain) = pipeline.explain(record);
                counts.add(&response);
                write_line(&mut output, &response, Some(explain.as_ref()))
            }
        };
        if let Err(err) = written {
            error = Some(InputError {
                record: batch.first_record + index,
                message: err.to_string(),
            });
            break;
        }
    }
    Processed {
        sequence: batch.sequence,
        output,
        counts,
        error,
    }
}

fn write_line(
    output: &mut Vec<u8>,
    response: &Response<'_>,
    explain: Option<Option<&Explain<'_>>>,
) -> serde_json::Result<()> {
    let json = match explain {
        Some(explain) => JsonResponse::with_explain(response, explain),
        None => JsonResponse::new(response),
    };
    serde_json::to_writer(&mut *output, &json)?;
    output.push(b'\n');
    Ok(())
}

fn write_in_order(
    output: &mut impl Write,
    receiver: Receiver<Processed>,
) -> io::Result<(Counts, Option<InputError>)> {
    let mut pending = BTreeMap::new();
    let mut next = 0;
    let mut counts = Counts::default();
    for processed in receiver {
        pending.insert(processed.sequence, processed);
        while let Some(mut processed) = pending.remove(&next) {
            output.write_all(&processed.output)?;
            counts.merge(&processed.counts);
            next += 1;
            if let Some(error) = processed.error.take() {
                output.flush()?;
                return Ok((counts, Some(error)));
            }
        }
    }
    output.flush()?;
    Ok((counts, None))
}

fn write_failure(err: &io::Error) -> ExitCode {
    if err.kind() == ErrorKind::BrokenPipe {
        return ExitCode::SUCCESS;
    }
    eprintln!("error: standard output: {err}");
    ExitCode::FAILURE
}

#[derive(Default)]
struct Counts {
    ok: u64,
    malformed: u64,
    missing_required: u64,
    emptied: u64,
}

impl Counts {
    fn add(&mut self, response: &Response<'_>) {
        match response {
            Response::Ok(_) => self.ok += 1,
            Response::Malformed(_) => self.malformed += 1,
            Response::MissingRequired(_) => self.missing_required += 1,
            Response::Emptied(_) => self.emptied += 1,
        }
    }

    fn merge(&mut self, other: &Self) {
        self.ok += other.ok;
        self.malformed += other.malformed;
        self.missing_required += other.missing_required;
        self.emptied += other.emptied;
    }
}

impl fmt::Display for Counts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let records = self.ok + self.malformed + self.missing_required + self.emptied;
        write!(
            f,
            "{records} records: {} ok, {} malformed, {} missing_required, {} emptied",
            self.ok, self.malformed, self.missing_required, self.emptied
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn batch_sizes(records: &[&str]) -> Vec<(usize, usize)> {
        let (sender, receiver) = sync_channel(records.len() + 1);
        let mut batcher = Batcher::new(&sender);
        for record in records {
            batcher.add(record);
        }
        batcher.send();
        drop(sender);
        receiver
            .iter()
            .map(|batch| (batch.first_record, batch.ends.len()))
            .collect()
    }

    #[test]
    fn batches_close_at_the_record_limit() {
        let records = vec!["{}"; BATCH_RECORDS * 2 + 1];
        assert_eq!(
            batch_sizes(&records),
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
        assert_eq!(batch_sizes(&["{}", &long, &long, "{}"]), [(1, 3), (4, 1)]);
    }
}
