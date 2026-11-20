use std::fmt;
use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, ErrorKind, Write};
use std::path::Path;
use std::process::ExitCode;

use moldette::{Pipeline, Response};

use crate::{explain, load, response};

pub(crate) enum Mode {
    Run,
    Explain,
}

pub(crate) fn run(config: &Path, input: Option<&Path>, mode: Mode) -> ExitCode {
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
    let (reader, name): (Box<dyn BufRead>, String) = match input {
        Some(path) => match File::open(path) {
            Ok(file) => (Box::new(BufReader::new(file)), path.display().to_string()),
            Err(err) => {
                eprintln!("error: {}: {err}", path.display());
                return ExitCode::FAILURE;
            }
        },
        None => (Box::new(io::stdin().lock()), "standard input".to_owned()),
    };
    let mut output = BufWriter::new(io::stdout().lock());
    let mut counts = Counts::default();
    for (index, line) in reader.lines().enumerate() {
        let line = match line {
            Ok(line) => line,
            Err(err) => {
                let _ = output.flush();
                eprintln!("error: {name}: line {}: {err}", index + 1);
                return ExitCode::FAILURE;
            }
        };
        let printed = match mode {
            Mode::Run => {
                let response = pipeline.process(&line);
                counts.add(&response);
                response::to_json(&response)
            }
            Mode::Explain => {
                let (response, record) = pipeline.explain(&line);
                counts.add(&response);
                let mut printed = response::to_json(&response);
                printed["explain"] = record
                    .as_ref()
                    .map_or(serde_json::Value::Null, explain::to_json);
                printed
            }
        };
        if let Err(err) = write_line(&mut output, &printed) {
            return write_failure(&err);
        }
    }
    if let Err(err) = output.flush() {
        return write_failure(&err);
    }
    eprintln!("{counts}");
    ExitCode::SUCCESS
}

fn write_line(output: &mut impl Write, value: &serde_json::Value) -> io::Result<()> {
    serde_json::to_writer(&mut *output, value)?;
    output.write_all(b"\n")
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
