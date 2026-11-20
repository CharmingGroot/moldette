use std::fs;
use std::process::Output;

use super::{CONFIG_FIXTURES, DATA, FIXTURES, moldette, moldette_with_input, stderr};
use serde_json::{Value, json};

pub(crate) fn responses(output: &Output) -> Vec<Value> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).expect("one JSON response per line"))
        .collect()
}

#[test]
fn prints_one_response_per_input_line_and_status_counts() {
    let config = format!("{CONFIG_FIXTURES}/order_api.yaml");
    let output = moldette(&["run", &config, &format!("{DATA}/order_api.ndjson")]);
    assert!(output.status.success());
    let responses = responses(&output);
    assert_eq!(responses.len(), 9);
    assert_eq!(
        responses[1],
        json!({
            "status": "ok",
            "fields": {
                "id": "A-1002",
                "status": "on hold",
                "customer": "C-101",
                "first_sku": "S-3",
                "first_quantity": 1,
                "total": 40.0,
                "paid": false,
                "created_at": "2026-11-18T18:45:10+09:00",
            },
            "emptied": [],
        })
    );
    assert_eq!(responses[3]["emptied"], json!(["first_quantity", "paid"]));
    assert_eq!(
        responses[4],
        json!({ "status": "missing_required", "fields": ["total"] })
    );
    assert_eq!(responses[6]["status"], "malformed");
    assert!(responses[6]["reason"].is_string());
    assert_eq!(
        stderr(&output),
        "9 records: 4 ok, 3 malformed, 2 missing_required, 0 emptied\n"
    );
}

#[test]
fn reads_standard_input_without_an_input_file() {
    let config = format!("{CONFIG_FIXTURES}/nginx_access.yaml");
    let log = format!("{DATA}/nginx_access.log");
    let from_file = moldette(&["run", &config, &log]);
    let from_stdin = moldette_with_input(&["run", &config], &fs::read(&log).expect("access log"));
    assert!(from_file.status.success() && from_stdin.status.success());
    assert_eq!(from_stdin.stdout, from_file.stdout);
    assert_eq!(responses(&from_stdin).len(), 9);
}

#[test]
fn processes_a_csv_header_row_as_a_record() {
    let config = format!("{CONFIG_FIXTURES}/order_export.yaml");
    let csv = fs::read_to_string(format!("{DATA}/order_export.csv")).expect("order export");
    let with_header = moldette_with_input(&["run", &config], csv.as_bytes());
    let rows = csv.split_once('\n').expect("header row").1;
    let without_header = moldette_with_input(&["run", &config], rows.as_bytes());
    let (with_header, without_header) = (responses(&with_header), responses(&without_header));
    assert_eq!(with_header.len(), without_header.len() + 1);
    assert_eq!(with_header[1..], without_header[..]);
}

#[test]
fn fails_when_the_configuration_has_no_pipeline_sections() {
    let config = format!("{FIXTURES}/parser_only.yaml");
    let output = moldette(&["run", &config]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        stderr(&output),
        format!("error: {config}: configuration has no mapper, resp section\n")
    );
}

#[test]
fn fails_on_a_configuration_error_before_reading_input() {
    let config = format!("{FIXTURES}/errors.yaml");
    let input = "{\"id\":\"A-1\"}\n".repeat(100_000);
    let output = moldette_with_input(&["run", &config], input.as_bytes());
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(stderr(&output).contains("error: "));
}

#[test]
fn stops_at_a_line_that_is_not_utf8() {
    let config = format!("{CONFIG_FIXTURES}/order_api.yaml");
    let output = moldette_with_input(
        &["run", &config],
        b"{\"id\":\"A-1\"}\n\xff\n{\"id\":\"A-3\"}\n",
    );
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(responses(&output).len(), 1);
    assert!(stderr(&output).starts_with("error: standard input: line 2: "));
}

#[test]
fn fails_on_a_missing_input_file() {
    let config = format!("{CONFIG_FIXTURES}/order_api.yaml");
    let input = format!("{DATA}/missing.ndjson");
    let output = moldette(&["run", &config, &input]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).starts_with(&format!("error: {input}: ")));
}
