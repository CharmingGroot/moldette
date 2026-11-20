//! The `moldette` command, which checks a configuration and runs it over
//! records.

mod check;
mod explain;
mod lineage;
mod load;
mod response;
mod run;
mod value;

use std::env;
use std::ffi::OsString;
use std::num::NonZeroUsize;
use std::path::Path;
use std::process::ExitCode;
use std::thread;

use run::Mode;

const USAGE: &str = "\
Usage: moldette check <config>
       moldette run [--threads <n>] <config> [input]
       moldette explain [--threads <n>] <config> [input]

Commands:
  check  Report what the configuration gets wrong, then print its digest and
         lineage as JSON
  run    Process each line of the input file, or of standard input, and print
         one JSON response per line; status counts follow on standard error
  explain  Same as run, with how each output field was filled added to every
           response

Options:
  --threads <n>  Threads that parse and map records; the default is the number
                 of available cores. Output keeps the input order.

A configuration file whose name ends in `.json` is read as JSON, any other as
YAML. Only built-in formats and ops are available. Every input line is one
record, a CSV header row included.";

const USAGE_ERROR: u8 = 2;

fn main() -> ExitCode {
    let mut args: Vec<OsString> = env::args_os().skip(1).collect();
    let threads = match take_threads(&mut args) {
        Ok(threads) => threads,
        Err(()) => return usage_error(),
    };
    match args.as_slice() {
        [command, config] if command == "check" && threads.is_none() => {
            check::run(Path::new(config))
        }
        [command, config, input @ ..] if input.len() <= 1 => match mode(command) {
            Some(mode) => run::run(
                Path::new(config),
                input.first().map(Path::new),
                mode,
                threads.unwrap_or_else(available_threads),
            ),
            None => usage_error(),
        },
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

fn take_threads(args: &mut Vec<OsString>) -> Result<Option<NonZeroUsize>, ()> {
    let Some(i) = args.iter().position(|arg| arg == "--threads") else {
        return Ok(None);
    };
    let value = args
        .get(i + 1)
        .and_then(|value| value.to_str()?.parse().ok());
    args.drain(i..(i + 2).min(args.len()));
    value.map(Some).ok_or(())
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
