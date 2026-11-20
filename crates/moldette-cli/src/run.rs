use std::collections::BTreeMap;
use std::fmt;
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, ErrorKind, Write};
use std::num::NonZeroUsize;
use std::path::Path;
use std::process::ExitCode;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;

use moldette::{Explain, Pipeline, Response};

use crate::load;
use crate::response::Printed;

const BATCH_LINES: usize = 256;
const BATCH_BYTES: usize = 1 << 20;

pub(crate) enum Mode {
    Run,
    Explain,
}

struct Batch {
    sequence: usize,
    first_line: usize,
    text: Vec<u8>,
}

struct Processed {
    sequence: usize,
    output: Vec<u8>,
    counts: Counts,
    error: Option<InputError>,
}

struct InputError {
    line: usize,
    error: io::Error,
}

pub(crate) fn run(
    config: &Path,
    input: Option<&Path>,
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
    let (reader, name): (Box<dyn BufRead + Send>, String) = match input {
        Some(path) => match File::open(path) {
            Ok(file) => (Box::new(BufReader::new(file)), path.display().to_string()),
            Err(err) => {
                eprintln!("error: {}: {err}", path.display());
                return ExitCode::FAILURE;
            }
        },
        None => (
            Box::new(BufReader::new(io::stdin())),
            "standard input".to_owned(),
        ),
    };
    let mut output = BufWriter::new(io::stdout().lock());
    let (written, read) = thread::scope(|scope| {
        let (batch_sender, batch_receiver) = sync_channel(threads.get() * 2);
        let (processed_sender, processed_receiver) = sync_channel(threads.get() * 2);
        let batch_receiver = Arc::new(Mutex::new(batch_receiver));
        let reader = scope.spawn(move || read_batches(reader, &batch_sender));
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
        Some(InputError { line, error }) => {
            eprintln!("error: {name}: line {line}: {error}");
            ExitCode::FAILURE
        }
    }
}

fn read_batches(
    mut reader: Box<dyn BufRead + Send>,
    sender: &SyncSender<Batch>,
) -> Option<InputError> {
    let mut sequence = 0;
    let mut first_line = 1;
    loop {
        let mut text = Vec::new();
        let mut lines = 0;
        let mut error = None;
        let mut end = false;
        while lines < BATCH_LINES && text.len() < BATCH_BYTES {
            match reader.read_until(b'\n', &mut text) {
                Ok(0) => {
                    end = true;
                    break;
                }
                Ok(_) => lines += 1,
                Err(err) => {
                    error = Some(InputError {
                        line: first_line + lines,
                        error: err,
                    });
                    break;
                }
            }
        }
        if lines > 0 {
            let batch = Batch {
                sequence,
                first_line,
                text,
            };
            if sender.send(batch).is_err() {
                return None;
            }
        }
        if error.is_some() || end {
            return error;
        }
        sequence += 1;
        first_line += lines;
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
    let text = batch.text.strip_suffix(b"\n").unwrap_or(&batch.text);
    for (index, line) in text.split(|&byte| byte == b'\n').enumerate() {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Ok(line) = std::str::from_utf8(line) else {
            error = Some(InputError {
                line: batch.first_line + index,
                error: io::Error::new(ErrorKind::InvalidData, "stream did not contain valid UTF-8"),
            });
            break;
        };
        let written = match mode {
            Mode::Run => {
                let response = pipeline.process(line);
                counts.add(&response);
                write_line(&mut output, &response, None)
            }
            Mode::Explain => {
                let (response, record) = pipeline.explain(line);
                counts.add(&response);
                write_line(&mut output, &response, Some(record.as_ref()))
            }
        };
        if let Err(err) = written {
            error = Some(InputError {
                line: batch.first_line + index,
                error: io::Error::other(err),
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
    serde_json::to_writer(&mut *output, &Printed { response, explain })?;
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
