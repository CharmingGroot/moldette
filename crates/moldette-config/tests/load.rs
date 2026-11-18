use moldette_config::{
    Config, FormatEntry, LoadError, LoadErrorKind, OpEntry, SourceEntry, ValueKind,
};
use moldette_core::Type;

fn yaml_error(text: &str) -> LoadError {
    Config::from_yaml(text).expect_err("configuration is rejected")
}

fn json_error(text: &str) -> LoadError {
    Config::from_json(text).expect_err("configuration is rejected")
}

fn location(err: &LoadError) -> (&LoadErrorKind, &str, Option<(usize, usize)>) {
    (
        err.kind(),
        err.path(),
        err.position().map(|p| (p.line(), p.column())),
    )
}

fn pairs(entries: &[(&str, &str)]) -> Vec<(String, String)> {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

#[test]
fn reads_yaml_and_json_into_the_same_model() {
    let yaml = Config::from_yaml(include_str!("fixtures/nginx_access.yaml")).expect("valid YAML");
    let json = Config::from_json(include_str!("fixtures/nginx_access.json")).expect("valid JSON");
    assert_eq!(yaml, json);

    let schema = &yaml.schemas[0];
    assert_eq!((schema.name.as_str(), schema.version), ("access", 1));
    assert_eq!(schema.fields.len(), 11);
    assert_eq!(schema.fields[0].ty, Type::Ip);
    assert!(schema.fields[0].required && !schema.fields[0].deprecated);
    assert!(schema.fields[9].deprecated);

    let parser = yaml.parser.as_ref().expect("parser section");
    assert!(matches!(
        &parser.format,
        FormatEntry::Regex { pattern } if pattern.starts_with(r"^(?P<remote_addr>\S+) \S+")
    ));
    assert_eq!(parser.fields.len(), 10);
    assert_eq!(parser.fields[3], ("uri".to_owned(), "uri".to_owned()));
    assert!(parser.empty_as_none);

    let mapper = yaml.mapper.as_ref().expect("mapper section");
    assert_eq!(
        (mapper.schema.name.as_str(), mapper.schema.version),
        ("access", 1)
    );
    assert!(!mapper.explicit_rules);
    let client = &mapper.rules[0];
    assert_eq!(client.target, "client");
    assert_eq!(client.source, SourceEntry::From("forwarded_for".into()));
    assert_eq!(client.fallback.as_deref(), Some("remote_addr"));
    assert_eq!(
        client.via,
        [
            OpEntry::Split {
                separator: ',',
                index: 0
            },
            OpEntry::Trim
        ]
    );
    assert_eq!(
        mapper.rules[1].via,
        [OpEntry::Timestamp {
            formats: vec![
                "[day]/[month repr:short]/[year]:[hour]:[minute]:[second] [offset_hour][offset_minute]"
                    .into()
            ]
        }]
    );
    assert_eq!(
        mapper.rules[4].via[1],
        OpEntry::Lookup {
            table: pairs(&[
                ("1", "informational"),
                ("2", "success"),
                ("3", "redirection"),
                ("4", "client_error"),
                ("5", "server_error"),
            ])
        }
    );
    assert_eq!(
        mapper.rules[6].source,
        SourceEntry::Constant("web-01".into())
    );

    let resp = yaml.resp.as_ref().expect("resp section");
    assert_eq!(
        (resp.schema.name.as_str(), resp.schema.version),
        ("access", 1)
    );
    assert!(!resp.strict);
}

#[test]
fn keeps_scalar_text_as_written() {
    let config = Config::from_yaml(
        r#"
parser:
  format: delimited
  delimiter: ","
  fields: {id: 0, code: 01}
mapper:
  schema: {name: order, version: 1}
  rules:
    - target: code
      from: code
      via:
        - lookup: {table: {01: one, "02": two, 1.50: price, true: yes}}
    - target: tier
      constant: 007
"#,
    )
    .expect("valid YAML");
    let parser = config.parser.expect("parser section");
    assert_eq!(parser.format, FormatEntry::Delimited { delimiter: ',' });
    assert_eq!(parser.fields, pairs(&[("id", "0"), ("code", "01")]));
    let rules = config.mapper.expect("mapper section").rules;
    assert_eq!(
        rules[0].via,
        [OpEntry::Lookup {
            table: pairs(&[
                ("01", "one"),
                ("02", "two"),
                ("1.50", "price"),
                ("true", "yes")
            ])
        }]
    );
    assert_eq!(rules[1].source, SourceEntry::Constant("007".into()));
}

#[test]
fn reads_a_subset_of_sections() {
    let parser_only =
        Config::from_yaml("parser: {format: kv, pair_delimiter: ' ', key_value_delimiter: '='}")
            .expect("valid YAML");
    assert_eq!(
        parser_only.parser.map(|parser| parser.format),
        Some(FormatEntry::Kv {
            pair_delimiter: ' ',
            key_value_delimiter: '='
        })
    );
    assert!(parser_only.schemas.is_empty());
    assert!(parser_only.mapper.is_none() && parser_only.resp.is_none());

    let resp_only = Config::from_json(
        r#"{"schemas": [{"name": "order", "version": 2, "fields": []}],
            "resp": {"schema": {"name": "order", "version": 2}, "strict": true}}"#,
    )
    .expect("valid JSON");
    assert!(resp_only.parser.is_none() && resp_only.mapper.is_none());
    assert!(resp_only.resp.is_some_and(|resp| resp.strict));
}

#[test]
fn reads_bodies_with_their_path() {
    let config = Config::from_yaml(
        r#"
parser:
  format: regex
  pattern: '^(?P<time>\S+) (?P<app>[^\[]+)\[(?P<pid>\d+)\]:(?: (?P<body>.*))?$'
  fields: {time: time, app: app}
  bodies:
    - path: body
      format: json
      fields: {order_id: /order_id, amount: /amount}
"#,
    )
    .expect("valid YAML");
    let body = &config.parser.expect("parser section").bodies[0];
    assert_eq!(body.path, "body");
    assert_eq!(body.parser.format, FormatEntry::Json);
    assert_eq!(
        body.parser.fields,
        pairs(&[("order_id", "/order_id"), ("amount", "/amount")])
    );
}

#[test]
fn keeps_names_and_arguments_of_caller_defined_ops_and_parsers() {
    let config = Config::from_yaml(
        r#"
parser:
  format: fixed_width
  widths: [4, 8]
  fields: {id: "0"}
mapper:
  schema: {name: order, version: 1}
  rules:
    - target: id
      from: id
      via:
        - mask
        - pad: {width: 8}
"#,
    )
    .expect("valid YAML");
    let parser = config.parser.expect("parser section");
    assert!(matches!(
        &parser.format,
        FormatEntry::Custom { name, arguments } if name == "fixed_width" && !arguments.is_empty()
    ));
    assert_eq!(parser.fields, pairs(&[("id", "0")]));
    let via = &config.mapper.expect("mapper section").rules[0].via;
    assert!(matches!(
        &via[0],
        OpEntry::Custom { name, arguments } if name == "mask" && arguments.is_empty()
    ));
    assert!(matches!(
        &via[1],
        OpEntry::Custom { name, arguments } if name == "pad" && !arguments.is_empty()
    ));
}

const UNKNOWN_ARGUMENT: &str = r#"mapper:
  schema: {name: order, version: 1}
  rules:
    - target: id
      from: id
      via:
        - split: {separator: "-", indx: 0}
"#;

#[test]
fn rejects_unknown_key_with_path_and_position() {
    let err = yaml_error(UNKNOWN_ARGUMENT);
    assert_eq!(
        location(&err),
        (
            &LoadErrorKind::UnknownKey("indx".into()),
            "mapper.rules[0].via[0].split",
            Some((7, 35))
        )
    );
    assert_eq!(
        err.to_string(),
        "mapper.rules[0].via[0].split: unknown key `indx` at line 7, column 35"
    );

    let err = json_error(
        r#"{"mapper": {"schema": {"name": "order", "version": 1},
            "rules": [{"target": "id", "from": "id", "via": [{"split": {"separator": "-", "indx": 0}}]}]}}"#,
    );
    assert_eq!(
        location(&err),
        (
            &LoadErrorKind::UnknownKey("indx".into()),
            "mapper.rules[0].via[0].split",
            None
        )
    );
}

#[test]
fn rejects_keys_of_another_format() {
    let err = yaml_error("parser:\n  format: json\n  delimiter: ','\n");
    assert_eq!(
        location(&err),
        (
            &LoadErrorKind::UnknownKey("delimiter".into()),
            "parser",
            Some((3, 3))
        )
    );
}

#[test]
fn rejects_value_of_wrong_kind_at_its_path() {
    let err = yaml_error(&UNKNOWN_ARGUMENT.replace("indx: 0", "index: first"));
    assert_eq!(
        location(&err),
        (
            &LoadErrorKind::TypeMismatch(ValueKind::Integer),
            "mapper.rules[0].via[0].split.index",
            Some((7, 42))
        )
    );

    let err = yaml_error(
        "schemas:\n  - {name: order, version: 1, fields: [{name: id, type: decimal}]}\n",
    );
    assert_eq!(
        location(&err),
        (
            &LoadErrorKind::UnknownFieldType("decimal".into()),
            "schemas[0].fields[0].type",
            Some((2, 57))
        )
    );

    for version in [r#""1""#, "1.0", "-1"] {
        let err = json_error(&format!(
            r#"{{"resp": {{"schema": {{"name": "order", "version": {version}}}}}}}"#
        ));
        assert_eq!(
            location(&err),
            (
                &LoadErrorKind::TypeMismatch(ValueKind::Integer),
                "resp.schema.version",
                None
            )
        );
    }

    let err = yaml_error("resp: {schema: {name: order, version: 4294967296}}");
    assert_eq!(
        err.kind(),
        &LoadErrorKind::IntegerOutOfRange("4294967296".into())
    );
}

#[test]
fn rejects_duplicate_keys() {
    let err = yaml_error("parser:\n  format: json\n  format: kv\n");
    assert_eq!(
        location(&err),
        (
            &LoadErrorKind::DuplicateKey("format".into()),
            "parser",
            Some((3, 3))
        )
    );

    let err =
        json_error(r#"{"parser": {"format": "json", "fields": {"id": "/id", "id": "/key"}}}"#);
    assert_eq!(
        location(&err),
        (
            &LoadErrorKind::DuplicateKey("id".into()),
            "parser.fields",
            None
        )
    );
}

#[test]
fn rejects_conflicting_or_missing_rule_source() {
    let err = yaml_error(
        "mapper:\n  schema: {name: order, version: 1}\n  rules:\n    - {target: id, from: id, constant: x}\n",
    );
    assert_eq!(
        location(&err),
        (
            &LoadErrorKind::ConflictingKeys("from".into(), "constant".into()),
            "mapper.rules[0]",
            Some((4, 30))
        )
    );

    let err =
        yaml_error("mapper:\n  schema: {name: order, version: 1}\n  rules:\n    - {target: id}\n");
    assert_eq!(
        location(&err),
        (
            &LoadErrorKind::MissingOneOf(vec!["from".into(), "coalesce".into(), "constant".into()]),
            "mapper.rules[0]",
            Some((4, 7))
        )
    );
}

#[test]
fn rejects_more_than_one_op_name_in_a_via_item() {
    let err = yaml_error(
        "mapper:\n  schema: {name: order, version: 1}\n  rules:\n    - target: id\n      from: id\n      via:\n        - split: {separator: '-', index: 0}\n          trim:\n",
    );
    assert_eq!(
        location(&err),
        (
            &LoadErrorKind::ConflictingKeys("split".into(), "trim".into()),
            "mapper.rules[0].via[0]",
            Some((8, 11))
        )
    );
}

#[test]
fn rejects_anchors_aliases_and_tags() {
    let err = yaml_error("schemas: &all []\n");
    assert_eq!(
        location(&err),
        (&LoadErrorKind::AnchorOrAlias, "schemas", Some((1, 15)))
    );

    let err = yaml_error("parser: {format: json}\nresp: *missing\n");
    assert!(matches!(err.kind(), LoadErrorKind::Syntax(_)));

    let err = yaml_error("resp:\n  schema: {name: order, version: !!int 1}\n");
    assert_eq!(
        location(&err),
        (&LoadErrorKind::Tag, "resp.schema.version", Some((2, 40)))
    );
}

#[test]
fn rejects_more_than_one_document() {
    let err = yaml_error("parser: {format: json}\n---\nresp: {}\n");
    assert_eq!(
        location(&err),
        (&LoadErrorKind::MultipleDocuments, "", Some((2, 1)))
    );
}

#[test]
fn rejects_deep_nesting() {
    let text = format!("parser: {}{}", "[".repeat(200), "]".repeat(200));
    assert_eq!(yaml_error(&text).kind(), &LoadErrorKind::NestingTooDeep);
}

#[test]
fn reports_syntax_errors_with_position() {
    let err = json_error("{\"parser\": }");
    assert!(matches!(err.kind(), LoadErrorKind::Syntax(message) if !message.contains("line")));
    assert_eq!(
        err.position().map(|p| (p.line(), p.column())),
        Some((1, 12))
    );

    let err = yaml_error("parser: {format: json\n");
    assert!(matches!(err.kind(), LoadErrorKind::Syntax(_)));
    assert!(err.position().is_some());

    assert_eq!(
        yaml_error("").kind(),
        &LoadErrorKind::TypeMismatch(ValueKind::Map)
    );
}
