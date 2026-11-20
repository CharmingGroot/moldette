use std::io::Write;
use std::process::{Command, Output, Stdio};

pub const CONFIG_FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../moldette-config/tests/fixtures"
);
pub const DATA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../moldette/tests/data");
pub const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

pub fn moldette(args: &[&str]) -> Output {
    moldette_with_input(args, b"")
}

pub fn moldette_with_input(args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_moldette"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("moldette starts");
    child
        .stdin
        .take()
        .expect("piped standard input")
        .write_all(input)
        .expect("input is written");
    child.wait_with_output().expect("moldette finishes")
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("UTF-8 on stderr")
}

mod check;
mod run;
