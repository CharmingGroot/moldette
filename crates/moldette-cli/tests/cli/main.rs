use std::io::{ErrorKind, Write};
use std::process::{Command, Output, Stdio};
use std::thread;

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
    let mut stdin = child.stdin.take().expect("piped standard input");
    let input = input.to_vec();
    let writer = thread::spawn(move || match stdin.write_all(&input) {
        Err(err) if err.kind() != ErrorKind::BrokenPipe => panic!("input is written: {err}"),
        _ => {}
    });
    let output = child.wait_with_output().expect("moldette finishes");
    writer.join().expect("input writer finishes");
    output
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("UTF-8 on stderr")
}

mod check;
mod explain;
mod records;
mod run;
mod threads;
