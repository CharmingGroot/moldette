use std::fs;

use serde_json::json;

use super::run::responses;
use super::{CONFIG_FIXTURES, DATA, moldette, moldette_with_input, stderr};

#[test]
fn adds_an_explain_record_to_each_response_of_run() {
    let config = format!("{CONFIG_FIXTURES}/order_api.yaml");
    let input = format!("{DATA}/order_api.ndjson");
    let run = moldette(&["run", &config, &input]);
    let explain = moldette(&["explain", &config, &input]);
    assert!(explain.status.success());
    assert_eq!(stderr(&explain), stderr(&run));
    let explained: Vec<_> = responses(&explain)
        .into_iter()
        .map(|mut response| {
            let object = response.as_object_mut().expect("response object");
            assert!(object.remove("explain").is_some());
            response
        })
        .collect();
    assert_eq!(explained, responses(&run));
}

#[test]
fn records_the_text_read_each_op_output_and_the_converted_value() {
    let config = format!("{CONFIG_FIXTURES}/order_export.yaml");
    let csv = fs::read_to_string(format!("{DATA}/order_export.csv")).expect("order export");
    let first_row = csv.lines().nth(1).expect("first row");
    let output = moldette_with_input(&["explain", &config], first_row.as_bytes());
    let explain = &responses(&output)[0]["explain"];
    assert_eq!(
        (&explain["schema"], &explain["version"]),
        (&json!("order_line"), &json!(1))
    );
    assert_eq!(
        explain["fields"][1],
        json!({
            "name": "placed_at",
            "reads": [{
                "source": {"field": "placed_at"},
                "text": "2026-11-18 09:30:00",
                "steps": [{"op": "timestamp", "output": "2026-11-18T09:30:00Z"}],
                "value": "2026-11-18T09:30:00Z",
            }],
        })
    );
}

#[test]
fn explains_nothing_for_a_malformed_record() {
    let config = format!("{CONFIG_FIXTURES}/order_api.yaml");
    let output = moldette_with_input(&["explain", &config], b"{\"id\":\"A-1007\",\"total\":\n");
    let response = &responses(&output)[0];
    assert_eq!(response["status"], "malformed");
    assert_eq!(response["explain"], json!(null));
}
