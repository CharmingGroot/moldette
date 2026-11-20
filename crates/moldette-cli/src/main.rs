//! The `moldette` command, which checks a configuration and runs it over
//! records.

mod check;
mod explain;
mod lineage;
mod load;
mod response;
mod run;

use std::env;
use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;

use run::Mode;

const USAGE: &str = "\
Usage: moldette check <config>
       moldette run <config> [input]
       moldette explain <config> [input]

Commands:
  check  Report what the configuration gets wrong, then print its digest and
         lineage as JSON
  run    Process each line of the input file, or of standard input, and print
         one JSON response per line; status counts follow on standard error
  explain  Same as run, with how each output field was filled added to every
           response

A configuration file whose name ends in `.json` is read as JSON, any other as
YAML. Only built-in formats and ops are available. Every input line is one
record, a CSV header row included.";

const USAGE_ERROR: u8 = 2;

fn main() -> ExitCode {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    match args.as_slice() {
        [command, config] if command == "check" => check::run(Path::new(config)),
        [command, config, input @ ..] if input.len() <= 1 => match mode(command) {
            Some(mode) => run::run(Path::new(config), input.first().map(Path::new), mode),
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
