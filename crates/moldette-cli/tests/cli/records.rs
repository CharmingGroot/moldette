use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process;

use flate2::Compression;
use flate2::write::GzEncoder;

use super::run::responses;
use super::{CONFIG_FIXTURES, DATA, moldette, moldette_with_input, stderr};

fn config() -> String {
    format!("{CONFIG_FIXTURES}/order_api.yaml")
}

fn orders() -> Vec<String> {
    fs::read_to_string(format!("{DATA}/order_api.ndjson"))
        .expect("order API data")
        .lines()
        .take(6)
        .map(str::to_owned)
        .collect()
}

fn ndjson() -> String {
    orders().iter().map(|order| format!("{order}\n")).collect()
}

fn gzip(text: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(text).expect("compressed in memory");
    encoder.finish().expect("compressed in memory")
}

struct TempFile(PathBuf);

impl TempFile {
    fn new(name: &str, contents: &[u8]) -> Self {
        let path = env::temp_dir().join(format!("moldette-cli-{}-{name}", process::id()));
        fs::write(&path, contents).expect("temporary file is written");
        Self(path)
    }

    fn path(&self) -> &str {
        self.0.to_str().expect("UTF-8 temporary path")
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[test]
fn a_json_array_and_concatenated_values_read_like_one_record_per_line() {
    let expected = moldette_with_input(&["run", &config()], ndjson().as_bytes());
    let pretty: Vec<String> = orders()
        .iter()
        .map(|order| order.replace(",\"", ",\n    \""))
        .collect();
    let array = format!("[\n  {}\n]\n", pretty.join(",\n  "));
    let concatenated = orders().concat();
    for input in [array, concatenated] {
        let output =
            moldette_with_input(&["run", "--records", "json", &config()], input.as_bytes());
        assert!(output.status.success(), "{}", stderr(&output));
        assert_eq!(output.stdout, expected.stdout);
        assert_eq!(stderr(&output), stderr(&expected));
    }
}

#[test]
fn multiline_records_join_the_lines_that_do_not_start_one() {
    let expected = moldette_with_input(&["run", &config()], ndjson().as_bytes());
    let input: String = orders()
        .iter()
        .map(|order| format!("{}\n", order.replace(",\"", ",\n  \"")))
        .collect();
    for command in ["run", "explain"] {
        let expected = moldette_with_input(&[command, &config()], ndjson().as_bytes());
        let output = moldette_with_input(
            &[
                command,
                "--records",
                "multiline",
                "--record-start",
                "^\\{",
                &config(),
            ],
            input.as_bytes(),
        );
        assert!(output.status.success(), "{}", stderr(&output));
        assert_eq!(output.stdout, expected.stdout, "{command}");
    }
    let lines = moldette_with_input(&["run", &config()], input.as_bytes());
    assert_ne!(lines.stdout, expected.stdout);
}

#[test]
fn json_input_that_ends_inside_a_value_fails_after_the_records_before_it() {
    let input = format!("[{},{}", orders()[0], &orders()[1][..20]);
    let output = moldette_with_input(&["run", "--records", "json", &config()], input.as_bytes());
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(responses(&output).len(), 1);
    assert_eq!(
        stderr(&output),
        "error: standard input: record 2: the input ends inside a JSON value\n"
    );
}

#[test]
fn gzip_input_files_are_decompressed_member_after_member() {
    let expected = moldette_with_input(&["run", &config()], ndjson().as_bytes());
    let orders = orders();
    let (first, second) = orders.split_at(2);
    let mut compressed = gzip(format!("{}\n", first.join("\n")).as_bytes());
    compressed.extend(gzip(format!("{}\n", second.join("\n")).as_bytes()));
    let file = TempFile::new("orders.ndjson.gz", &compressed);
    let output = moldette(&["run", &config(), file.path()]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(output.stdout, expected.stdout);
}

#[test]
fn a_damaged_gzip_file_fails_at_the_record_it_could_not_read() {
    let compressed = gzip(ndjson().repeat(50).as_bytes());
    let file = TempFile::new("damaged.ndjson.gz", &compressed[..compressed.len() * 2 / 3]);
    let output = moldette(&["run", &config(), file.path()]);
    assert_eq!(output.status.code(), Some(1));
    let read = responses(&output).len();
    assert!(read > 0);
    assert!(
        stderr(&output).starts_with(&format!("error: {}: record {}: ", file.path(), read + 1)),
        "{}",
        stderr(&output)
    );
}

#[test]
fn only_files_ending_in_gz_are_decompressed() {
    let file = TempFile::new("orders.ndjson", &gzip(ndjson().as_bytes()));
    let output = moldette(&["run", &config(), file.path()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(stderr(&output).starts_with(&format!(
        "error: {}: record 1: not valid UTF-8",
        file.path()
    )));
}

#[test]
fn rejects_an_invalid_record_start_pattern() {
    let output = moldette(&[
        "run",
        "--records",
        "multiline",
        "--record-start",
        "(",
        "a.yaml",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        stderr(&output),
        "error: --record-start: argument `record_start` has invalid value `(`\n"
    );
}
