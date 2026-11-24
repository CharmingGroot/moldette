use std::fs;

use super::run::responses;
use super::{CONFIG_FIXTURES, DATA, moldette_with_input, stderr};

fn repeated_orders(times: usize) -> String {
    fs::read_to_string(format!("{DATA}/order_api.ndjson"))
        .expect("order API data")
        .repeat(times)
}

#[test]
fn output_and_counts_do_not_depend_on_the_thread_count() {
    let config = format!("{CONFIG_FIXTURES}/order_api.yaml");
    let input = repeated_orders(400);
    for command in ["run", "explain"] {
        let outputs: Vec<_> = [&["1"][..], &["3"], &[]]
            .iter()
            .map(|threads| {
                let mut args = vec![command];
                if let Some(threads) = threads.first() {
                    args.extend(["--threads", threads]);
                }
                args.push(&config);
                moldette_with_input(&args, input.as_bytes())
            })
            .collect();
        for output in &outputs {
            assert!(output.status.success(), "{command}");
            assert_eq!(output.stdout, outputs[0].stdout, "{command}");
            assert_eq!(stderr(output), stderr(&outputs[0]), "{command}");
        }
        assert_eq!(responses(&outputs[0]).len(), 3600, "{command}");
    }
    assert_eq!(
        stderr(&moldette_with_input(
            &["run", &config, "--threads", "2"],
            input.as_bytes()
        )),
        "3600 records: 1600 ok, 1200 malformed, 800 missing_required, 0 emptied\n"
    );
}

#[test]
fn stops_at_a_line_that_is_not_utf8_after_the_lines_before_it() {
    let config = format!("{CONFIG_FIXTURES}/order_api.yaml");
    let mut input = repeated_orders(250).into_bytes();
    input.extend_from_slice(b"\xff\n");
    input.extend_from_slice(repeated_orders(10).as_bytes());
    let output = moldette_with_input(&["run", "--threads", "4", &config], &input);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(responses(&output).len(), 2250);
    assert!(stderr(&output).starts_with("error: standard input: record 2251: "));
}

#[test]
fn batches_of_long_lines_keep_the_input_order() {
    let config = format!("{CONFIG_FIXTURES}/order_api.yaml");
    let note = "x".repeat(600 * 1024);
    let input: String = (0..6)
        .map(|i| format!("{{\"id\":\"A-{i}\",\"total\":{i},\"created_at\":\"2026-11-18T09:30:00Z\",\"note\":\"{note}\"}}\n"))
        .collect();
    let one = moldette_with_input(&["run", "--threads", "1", &config], input.as_bytes());
    let three = moldette_with_input(&["run", "--threads", "3", &config], input.as_bytes());
    assert_eq!(one.stdout, three.stdout);
    let ids: Vec<_> = responses(&three)
        .iter()
        .map(|response| response["fields"]["id"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(
        ids,
        (0..6).map(|i| Some(format!("A-{i}"))).collect::<Vec<_>>()
    );
}

#[test]
fn lines_ending_in_crlf_read_as_without_it() {
    let config = format!("{CONFIG_FIXTURES}/order_api.yaml");
    let lf = repeated_orders(1);
    let crlf = lf.replace('\n', "\r\n");
    assert_eq!(
        moldette_with_input(&["run", &config], crlf.as_bytes()).stdout,
        moldette_with_input(&["run", &config], lf.as_bytes()).stdout
    );
}

#[test]
fn an_option_may_join_its_value_with_an_equals_sign() {
    let config = format!("{CONFIG_FIXTURES}/order_api.yaml");
    let input = repeated_orders(2);
    let separate = moldette_with_input(&["run", "--threads", "2", &config], input.as_bytes());
    let joined = moldette_with_input(&["run", "--threads=2", &config], input.as_bytes());
    assert!(joined.status.success(), "{}", stderr(&joined));
    assert_eq!(joined.stdout, separate.stdout);
    let empty = moldette_with_input(&["run", "--threads=", &config], input.as_bytes());
    assert_eq!(empty.status.code(), Some(2));
}
