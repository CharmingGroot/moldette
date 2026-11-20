//! The `moldette` command, which checks a configuration and runs it over
//! records.

mod check;
mod lineage;
mod load;
mod response;
mod run;

use std::env;
use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "\
Usage: moldette check <config>
       moldette run <config> [input]

Commands:
  check  Report what the configuration gets wrong, then print its digest and
         lineage as JSON
  run    Process each line of the input file, or of standard input, and print
         one JSON response per line; status counts follow on standard error

A configuration file whose name ends in `.json` is read as JSON, any other as
YAML. Only built-in formats and ops are available. Every input line is one
record, a CSV header row included.";

const USAGE_ERROR: u8 = 2;

fn main() -> ExitCode {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    match args.as_slice() {
        [command, config] if command == "check" => check::run(Path::new(config)),
        [command, config] if command == "run" => run::run(Path::new(config), None),
        [command, config, input] if command == "run" => {
            run::run(Path::new(config), Some(Path::new(input)))
        }
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        [flag] if flag == "--version" || flag == "-V" => {
            println!("moldette {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(USAGE_ERROR)
        }
    }
}
