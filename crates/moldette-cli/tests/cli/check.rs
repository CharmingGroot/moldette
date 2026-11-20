use std::process::Output;

use super::{CONFIG_FIXTURES, FIXTURES, moldette, stderr};
use serde_json::{Value, json};

fn check(path: &str) -> Output {
    moldette(&["check", path])
}

fn stdout_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("JSON on stdout")
}

#[test]
fn prints_digest_and_lineage_of_a_valid_configuration() {
    let output = check(&format!("{CONFIG_FIXTURES}/order_api.yaml"));
    assert!(output.status.success());
    assert_eq!(stderr(&output), "");
    let printed = stdout_json(&output);
    assert!(
        printed["digest"]
            .as_str()
            .is_some_and(|digest| digest.starts_with("sha256:"))
    );
    let lineage = &printed["lineage"];
    assert_eq!(
        (&lineage["schema"], &lineage["version"]),
        (&json!("order"), &json!(1))
    );
    assert_eq!(
        lineage["fields"][2],
        json!({
            "name": "customer",
            "source": "same_name",
            "reads": [{
                "field": "customer",
                "fallback": false,
                "origin": [{"format": "json", "path": "/customer/id"}],
            }],
            "ops": [],
        })
    );
}

#[test]
fn prints_the_same_output_for_yaml_and_json_configurations() {
    let yaml = check(&format!("{CONFIG_FIXTURES}/nginx_access.yaml"));
    let json = check(&format!("{CONFIG_FIXTURES}/nginx_access.json"));
    assert!(yaml.status.success() && json.status.success());
    assert_eq!(yaml.stdout, json.stdout);
}

#[test]
fn reports_warnings_with_their_position_and_succeeds() {
    let path = format!("{FIXTURES}/warnings.yaml");
    let output = check(&path);
    assert!(output.status.success());
    assert_eq!(
        stderr(&output),
        format!(
            "warning: {path}: schemas[0].fields[1]: field `total` has no source in any record at line 6, column 9\n\
             warning: {path}: parser.fields.note: no rule reads field `note` at line 12, column 11\n"
        )
    );
    assert_eq!(
        stdout_json(&output)["lineage"]["fields"][1]["source"],
        "same_name"
    );
}

#[test]
fn reports_every_diagnostic_and_fails_without_output_on_errors() {
    let path = format!("{FIXTURES}/errors.yaml");
    let output = check(&path);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        stderr(&output),
        format!(
            "warning: {path}: parser.fields.note: no rule reads field `note` at line 12, column 11\n\
             error: {path}: mapper.rules[0].from: the parser outputs no field `amount` at line 17, column 29\n"
        )
    );
}

#[test]
fn fails_on_a_configuration_that_does_not_load() {
    let path = format!("{FIXTURES}/unknown_key.yaml");
    let output = check(&path);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        stderr(&output),
        format!("error: {path}: unknown key `parsers` at line 2, column 1\n")
    );
}

#[test]
fn fails_on_a_missing_file() {
    let path = format!("{FIXTURES}/missing.yaml");
    let output = check(&path);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).starts_with(&format!("error: {path}: ")));
}

#[test]
fn exits_with_usage_error_on_unknown_arguments() {
    for args in [
        &[][..],
        &["check"],
        &["check", "a.yaml", "b.yaml"],
        &["run"],
        &["run", "a.yaml", "b.log", "c.log"],
        &["convert", "a.yaml"],
    ] {
        let output = moldette(args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(stderr(&output).starts_with("Usage: moldette"), "{args:?}");
    }
}

#[test]
fn prints_usage_and_version_on_request() {
    let help = moldette(&["--help"]);
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).starts_with("Usage: moldette"));
    let version = moldette(&["--version"]);
    assert_eq!(
        String::from_utf8_lossy(&version.stdout),
        format!("moldette {}\n", env!("CARGO_PKG_VERSION"))
    );
}
