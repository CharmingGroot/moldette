mod common;

use common::{
    DEPRECATED_FIELDS, FIXED_ORDERS, NO_RULES, ORDER_SCHEMA, build_error, build_yaml, location,
};
use moldette_config::{Config, DiagnosticKind, LoadErrorKind, Registry, Report, Severity};

type Located<'r> = (DiagnosticKind, Severity, &'r str, Option<(usize, usize)>);

fn diagnostics(report: &Report) -> Vec<Located<'_>> {
    report
        .diagnostics()
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.kind().clone(),
                diagnostic.severity(),
                diagnostic.path(),
                diagnostic.position().map(|p| (p.line(), p.column())),
            )
        })
        .collect()
}

fn check_yaml(text: &str) -> Report {
    Config::from_yaml(text).expect("valid YAML").check()
}

#[test]
fn access_log_fixtures_report_only_the_deprecated_field_they_map() {
    for (config, position) in [
        (
            Config::from_yaml(include_str!("fixtures/nginx_access.yaml")).expect("valid YAML"),
            Some((14, 9)),
        ),
        (
            Config::from_json(include_str!("fixtures/nginx_access.json")).expect("valid JSON"),
            None,
        ),
    ] {
        let expected = [(
            DiagnosticKind::DeprecatedField("referer".into()),
            Severity::Warning,
            "schemas[0].fields[9]",
            position,
        )];
        assert_eq!(diagnostics(&config.check()), expected);
        let specs = config.build(&Registry::new()).expect("specs are built");
        assert_eq!(diagnostics(specs.report()), expected);
    }
}

#[test]
fn reports_deprecated_fields_a_rule_or_the_parser_still_fills() {
    let deprecated = |name: &str, path, position| {
        (
            DiagnosticKind::DeprecatedField(name.into()),
            Severity::Warning,
            path,
            Some(position),
        )
    };
    let note = deprecated("note", "schemas[0].fields[2]", (7, 9));
    let channel = deprecated("channel", "mapper.rules[1].target", (19, 16));
    let specs = build_yaml(DEPRECATED_FIELDS, &Registry::new()).expect("specs are built");
    assert_eq!(diagnostics(specs.report()), [note, channel]);
    assert_eq!(
        specs.report().diagnostics()[1].to_string(),
        "mapper.rules[1].target: deprecated field `channel` is mapped at line 19, column 16"
    );
    let explicit = DEPRECATED_FIELDS.replace(
        "  rules:\n",
        "  explicit_rules: true\n  rules:\n    - {target: id, from: id}\n",
    );
    assert_eq!(
        diagnostics(&check_yaml(&explicit)),
        [
            (
                DiagnosticKind::UnreadField("note".into()),
                Severity::Warning,
                "parser.fields.note",
                Some((14, 26))
            ),
            deprecated("channel", "mapper.rules[2].target", (21, 16)),
        ]
    );

    let without_parser = DEPRECATED_FIELDS.replace(
        "parser:\n  format: kv\n  pair_delimiter: \" \"\n  key_value_delimiter: \"=\"\n  fields: {id: id, note: note, total: total}\n",
        "",
    );
    assert_eq!(
        diagnostics(&check_yaml(&without_parser)),
        [deprecated("channel", "mapper.rules[1].target", (14, 16))]
    );

    let unfilled = DEPRECATED_FIELDS
        .replace("note: note, ", "")
        .replace("    - {target: channel, constant: web}\n", "");
    assert_eq!(check_yaml(&unfilled), Report::default());
}

const UNKNOWN_SOURCES: &str = r#"schemas:
  - name: order
    version: 1
    fields:
      - {name: id, type: string, required: true}
      - {name: amount, type: float}
      - {name: status, type: string}
parser:
  format: delimited
  delimiter: ","
  fields: {order_id: "0", total: "1", state: "2"}
mapper:
  schema: {name: order, version: 1}
  rules:
    - {target: id, from: order_no, fallback: order_id}
    - {target: amount, coalesce: [total, sum], fallback: amont}
    - {target: status, from: state}
"#;

#[test]
fn rejects_sources_the_parser_never_outputs() {
    let unknown = |name: &str| DiagnosticKind::UnknownSourceField(name.into());
    assert_eq!(
        diagnostics(&check_yaml(UNKNOWN_SOURCES)),
        [
            (
                unknown("order_no"),
                Severity::Error,
                "mapper.rules[0].from",
                Some((15, 26))
            ),
            (
                unknown("sum"),
                Severity::Error,
                "mapper.rules[1].coalesce[1]",
                Some((16, 42))
            ),
            (
                unknown("amont"),
                Severity::Error,
                "mapper.rules[1].fallback",
                Some((16, 58))
            ),
        ]
    );

    let err = build_error(UNKNOWN_SOURCES, &Registry::new());
    assert_eq!(
        location(&err),
        (
            &LoadErrorKind::Diagnostic(unknown("order_no")),
            "mapper.rules[0].from",
            Some((15, 26))
        )
    );
    assert_eq!(
        err.to_string(),
        "mapper.rules[0].from: the parser outputs no field `order_no` at line 15, column 26"
    );

    let fixed = UNKNOWN_SOURCES
        .replace("from: order_no", "from: order_id")
        .replace(", sum]", "]")
        .replace("fallback: amont", "fallback: total");
    let specs = build_yaml(&fixed, &Registry::new()).expect("specs are built");
    assert!(specs.report().diagnostics().is_empty());
}

#[test]
fn reports_schema_fields_without_a_source() {
    let absent_id = (
        DiagnosticKind::AbsentRequiredField("id".into()),
        Severity::Error,
        "schemas[0].fields[0]",
        Some((5, 9)),
    );
    let absent_note = (
        DiagnosticKind::AbsentField("note".into()),
        Severity::Warning,
        "schemas[0].fields[1]",
        Some((6, 9)),
    );
    let report = check_yaml(NO_RULES);
    assert!(report.has_errors());
    assert_eq!(
        diagnostics(&report),
        [absent_id.clone(), absent_note.clone()]
    );
    let err = build_error(NO_RULES, &Registry::new());
    assert_eq!(
        location(&err),
        (
            &LoadErrorKind::Diagnostic(DiagnosticKind::AbsentRequiredField("id".into())),
            "schemas[0].fields[0]",
            Some((5, 9))
        )
    );

    let optional = NO_RULES.replace(", required: true", "");
    let specs = build_yaml(&optional, &Registry::new()).expect("specs are built");
    assert!(!specs.report().has_errors());
    assert_eq!(
        diagnostics(specs.report()),
        [
            (
                DiagnosticKind::AbsentField("id".into()),
                Severity::Warning,
                "schemas[0].fields[0]",
                Some((5, 9))
            ),
            absent_note,
        ]
    );

    let explicit =
        format!("{NO_RULES}  explicit_rules: true\n  rules: [{{target: amount, from: amount}}]\n");
    assert_eq!(diagnostics(&check_yaml(&explicit)), [absent_id]);

    let declared = NO_RULES.replace("{amount: amount}", "{id: id, note: note, amount: amount}");
    assert_eq!(check_yaml(&declared), Report::default());
}

#[test]
fn reports_parser_fields_no_rule_reads_across_bodies() {
    let config = include_str!("fixtures/order_bodies.yaml");
    let specs = build_yaml(config, &Registry::new()).expect("specs are built");
    assert_eq!(
        diagnostics(specs.report()),
        [
            (
                DiagnosticKind::UnreadField("channel".into()),
                Severity::Warning,
                "parser.fields.channel",
                Some((10, 30))
            ),
            (
                DiagnosticKind::UnreadField("note".into()),
                Severity::Warning,
                "parser.bodies[0].fields.note",
                Some((14, 33))
            ),
        ]
    );
}

#[test]
fn rejects_resp_schemas_other_than_the_mapper_schema() {
    let config = r#"schemas:
  - {name: order, version: 1, fields: [{name: id, type: string}]}
  - {name: order, version: 2, fields: [{name: id, type: string}]}
mapper:
  schema: {name: order, version: 1}
resp:
  schema: {name: order, version: 2}
"#;
    let mismatch = |version| DiagnosticKind::SchemaMismatch {
        name: "order".into(),
        version,
    };
    let report = check_yaml(config);
    assert_eq!(
        diagnostics(&report),
        [(mismatch(1), Severity::Error, "resp.schema", Some((7, 11)))]
    );
    let message = "resp.schema: the mapper maps to schema `order` version 1 at line 7, column 11";
    assert_eq!(report.diagnostics()[0].to_string(), message);
    assert_eq!(build_error(config, &Registry::new()).to_string(), message);

    let json = Config::from_json(
        r#"{"schemas": [{"name": "order", "version": 1, "fields": []}, {"name": "order", "version": 2, "fields": []}],
           "mapper": {"schema": {"name": "order", "version": 2}},
           "resp": {"schema": {"name": "order", "version": 1}}}"#,
    )
    .expect("valid JSON");
    assert_eq!(
        diagnostics(&json.check()),
        [(mismatch(2), Severity::Error, "resp.schema", None)]
    );

    let same = config.replace("{name: order, version: 2}", "{name: order, version: 1}");
    assert_eq!(check_yaml(&same), Report::default());
}

#[test]
fn checks_only_the_sections_present() {
    let without_parser = format!(
        "{ORDER_SCHEMA}mapper:\n  schema: {{name: order, version: 1}}\n  rules:\n    - {{target: id, from: order_no, fallback: number}}\n"
    );
    assert_eq!(check_yaml(&without_parser), Report::default());

    let without_mapper = format!(
        "{ORDER_SCHEMA}parser: {{format: json, fields: {{id: /id, total: /total}}}}\nresp: {{schema: {{name: order, version: 1}}}}\n"
    );
    assert_eq!(check_yaml(&without_mapper), Report::default());

    let explicit_without_parser = r#"schemas:
  - {name: order, version: 1, fields: [{name: id, type: string, required: true}]}
mapper: {schema: {name: order, version: 1}, explicit_rules: true}
"#;
    assert_eq!(
        diagnostics(&check_yaml(explicit_without_parser)),
        [(
            DiagnosticKind::AbsentRequiredField("id".into()),
            Severity::Error,
            "schemas[0].fields[0]",
            Some((2, 40))
        )]
    );

    let undefined_schema = UNKNOWN_SOURCES.replace(
        "schema: {name: order, version: 1}",
        "schema: {name: order, version: 3}",
    );
    assert_eq!(check_yaml(&undefined_schema), Report::default());
    assert!(matches!(
        build_error(&undefined_schema, &Registry::new()).kind(),
        LoadErrorKind::UnknownSchema { .. }
    ));
}

#[test]
fn checks_caller_defined_parsers_by_their_declared_fields() {
    assert_eq!(check_yaml(FIXED_ORDERS), Report::default());

    let renamed = FIXED_ORDERS.replace("from: status", "from: state");
    assert_eq!(
        diagnostics(&check_yaml(&renamed)),
        [
            (
                DiagnosticKind::UnreadField("status".into()),
                Severity::Warning,
                "parser.fields.status",
                Some((13, 35))
            ),
            (
                DiagnosticKind::UnknownSourceField("state".into()),
                Severity::Error,
                "mapper.rules[1].from",
                Some((22, 13))
            ),
        ]
    );
}

#[test]
fn rejects_references_to_undefined_schemas() {
    let registry = Registry::new();
    let err = build_error(
        "schemas:\n  - {name: order, version: 1, fields: []}\nresp:\n  schema: {name: order, version: 2}\n",
        &registry,
    );
    assert_eq!(
        location(&err),
        (
            &LoadErrorKind::UnknownSchema {
                name: "order".into(),
                version: 2
            },
            "resp.schema",
            Some((4, 11))
        )
    );
    assert_eq!(
        err.to_string(),
        "resp.schema: schema `order` version 2 is not defined at line 4, column 11"
    );

    let err = Config::from_json(r#"{"mapper": {"schema": {"name": "order", "version": 1}}}"#)
        .expect("valid JSON")
        .build(&registry)
        .expect_err("configuration is rejected");
    assert_eq!(
        location(&err),
        (
            &LoadErrorKind::UnknownSchema {
                name: "order".into(),
                version: 1
            },
            "mapper.schema",
            None
        )
    );
}

#[test]
fn rejects_schemas_defined_twice() {
    let err = build_error(
        "schemas:\n  - {name: order, version: 1, fields: []}\n  - {name: order, version: 1, fields: []}\n",
        &Registry::new(),
    );
    assert_eq!(
        location(&err),
        (
            &LoadErrorKind::DuplicateSchema {
                name: "order".into(),
                version: 1
            },
            "schemas[1]",
            Some((3, 5))
        )
    );
}
