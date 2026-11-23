//! The `moldette` command, which checks a configuration and runs it over
//! records.

mod check;
mod lineage;
mod load;
mod run;

use std::env;
use std::ffi::OsString;
use std::num::NonZeroUsize;
use std::path::Path;
use std::process::ExitCode;
use std::thread;

use moldette::framing::Framer;
use run::Mode;

const USAGE: &str = "\
Usage: moldette check <config>
       moldette run [options] <config> [input]
       moldette explain [options] <config> [input]

Commands:
  check  Report what the configuration gets wrong, then print its digest and
         lineage as JSON
  run    Process each record of the input file, or of standard input, and
         print one JSON response per line; status counts follow on standard
         error
  explain  Same as run, with how each output field was filled added to every
           response

Options:
  --threads <n>           Threads that parse and map records; the default is
                          the number of available cores. Output keeps the
                          input order.
  --records <how>         How the input splits into records: `lines`, one per
                          line (the default); `multiline`, see
                          --record-start; `json`, one per JSON value, or per
                          element of a top-level array
  --record-start <regex>  For `multiline`, the pattern of a line that starts a
                          record; each following line that does not match
                          joins it

A configuration file whose name ends in `.json` is read as JSON, any other as
YAML. Only built-in formats and ops are available. An input file whose name
ends in `.gz` is decompressed. With `lines`, a CSV header row is a record too.";

const USAGE_ERROR: u8 = 2;

fn main() -> ExitCode {
    let mut args: Vec<OsString> = env::args_os().skip(1).collect();
    let (Ok(threads), Ok(records), Ok(record_start)) = (
        take_option(&mut args, "--threads"),
        take_option(&mut args, "--records"),
        take_option(&mut args, "--record-start"),
    ) else {
        return usage_error();
    };
    let options = threads.is_some() || records.is_some() || record_start.is_some();
    match args.as_slice() {
        [command, config] if command == "check" && !options => check::run(Path::new(config)),
        [command, config, input @ ..] if input.len() <= 1 => {
            let (Some(mode), Ok(threads)) = (mode(command), parse_threads(threads)) else {
                return usage_error();
            };
            let framer = match framer(records, record_start) {
                Ok(framer) => framer,
                Err(code) => return code,
            };
            run::run(
                Path::new(config),
                input.first().map(Path::new),
                framer,
                mode,
                threads,
            )
        }
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        [flag] if flag == "--version" || flag == "-V" => {
            println!("moldette {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        _ => usage_error(),
    }
}

fn take_option(args: &mut Vec<OsString>, name: &str) -> Result<Option<String>, ()> {
    let Some(i) = args.iter().position(|arg| arg == name) else {
        return Ok(None);
    };
    let value = args
        .get(i + 1)
        .and_then(|value| value.to_str())
        .map(str::to_owned);
    args.drain(i..(i + 2).min(args.len()));
    value.map(Some).ok_or(())
}

fn parse_threads(threads: Option<String>) -> Result<NonZeroUsize, ()> {
    match threads {
        Some(threads) => threads.parse().map_err(drop),
        None => Ok(available_threads()),
    }
}

fn framer(records: Option<String>, record_start: Option<String>) -> Result<Framer, ExitCode> {
    match (records.as_deref(), record_start) {
        (None | Some("lines"), None) => Ok(Framer::lines()),
        (Some("json"), None) => Ok(Framer::json()),
        (Some("multiline"), Some(start)) => Framer::multiline(&start).map_err(|err| {
            eprintln!("error: --record-start: {err}");
            ExitCode::from(USAGE_ERROR)
        }),
        _ => Err(usage_error()),
    }
}

fn available_threads() -> NonZeroUsize {
    thread::available_parallelism().unwrap_or(NonZeroUsize::MIN)
}

fn mode(command: &OsString) -> Option<Mode> {
    match command.to_str()? {
        "run" => Some(Mode::Run),
        "explain" => Some(Mode::Explain),
        _ => None,
    }
}

fn usage_error() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(USAGE_ERROR)
}
