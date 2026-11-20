//! The `moldette` command, which checks a configuration and prints its lineage.

mod check;
mod lineage;

use std::env;
use std::ffi::OsString;
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "\
Usage: moldette <command> <config>

Commands:
  check <config>  Report what the configuration gets wrong, then print its
                  digest and lineage as JSON

A configuration file whose name ends in `.json` is read as JSON, any other as
YAML. Only built-in formats and ops are available.";

const USAGE_ERROR: u8 = 2;

fn main() -> ExitCode {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    match args.as_slice() {
        [command, config] if command == "check" => check::run(Path::new(config)),
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
