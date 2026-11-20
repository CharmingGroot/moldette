mod common;

use moldette_config::{Config, LoadError, Registry, Specs};
use moldette_core::{Field, OutputSchema, Type};

use common::{ORDER_SCHEMA, build_error};

const ACCESS_LOG_YAML: &str = include_str!("fixtures/nginx_access.yaml");
const ACCESS_LOG_JSON: &str = include_str!("fixtures/nginx_access.json");

fn access_schema() -> OutputSchema {
    [
        Field::new("client", Type::Ip).required(),
        Field::new("time", Type::Timestamp).required(),
        Field::new("method", Type::String),
        Field::new("path", Type::String),
        Field::new("http_version", Type::String),
        Field::new("status", Type::Int).required(),
        Field::new("status_class", Type::String),
        Field::new("bytes", Type::Int),
        Field::new("user_agent", Type::String),
        Field::new("referer", Type::String).deprecated(),
        Field::new("site", Type::String),
    ]
    .into_iter()
    .fold(OutputSchema::builder("access", 1), |builder, field| {
        builder.field(field)
    })
    .build()
    .expect("valid schema")
}

fn deprecated_schema() -> OutputSchema {
    OutputSchema::builder("order", 1)
        .field(Field::new("id", Type::String).required())
        .field(Field::new("coupon", Type::String).required().deprecated())
        .field(Field::new("note", Type::String).deprecated())
        .field(Field::new("channel", Type::String).deprecated())
        .field(Field::new("amount", Type::Float))
        .build()
        .expect("valid schema")
}

fn access_log_specs() -> [Specs; 2] {
    [
        Config::from_yaml(ACCESS_LOG_YAML).expect("valid YAML"),
        Config::from_json(ACCESS_LOG_JSON).expect("valid JSON"),
    ]
    .map(|config| config.build(&Registry::new()).expect("specs are built"))
}

fn mapper_error(rules: &str) -> LoadError {
    build_error(
        &format!("{ORDER_SCHEMA}mapper:\n  schema: {{name: order, version: 1}}\n  rules:\n{rules}"),
        &Registry::new(),
    )
}

mod lower {
    mod core {
        mod schema {
            use moldette_config::Registry;
            use moldette_core::SpecError;

            use crate::common::{DEPRECATED_FIELDS, build_error, build_yaml, location, spec};
            use crate::deprecated_schema;

            #[test]
            fn wraps_spec_errors_with_key_path() {
                let err = build_error(
                    "schemas:\n  - {name: order, version: 1, fields: [{name: id, type: string}, {name: id, type: int}]}\n",
                    &Registry::new(),
                );
                assert_eq!(
                    location(&err),
                    (
                        &spec(SpecError::DuplicateField("id".into())),
                        "schemas[0].fields[1].name",
                        None
                    )
                );
            }

            #[test]
            fn keeps_required_and_deprecated_flags() {
                let specs =
                    build_yaml(DEPRECATED_FIELDS, &Registry::new()).expect("specs are built");
                assert_eq!(specs.schemas(), [deprecated_schema()]);
            }
        }
    }

    mod parser {
        mod spec {
            use moldette_config::Registry;
            use moldette_core::SpecError;
            use moldette_parser::{Format, ParserSpec};

            use crate::common::{build_error, build_yaml, invalid_argument, location, spec};

            #[test]
            fn wraps_spec_errors_with_key_path() {
                let registry = Registry::new();
                let cases = [
                    (
                        "parser: {format: delimited, delimiter: ',', fields: {id: '0', amount: x}}",
                        spec(SpecError::InvalidPath("x".into())),
                        "parser.fields.amount",
                    ),
                    (
                        "parser: {format: regex, pattern: '(?P<id>', fields: {id: id}}",
                        spec(invalid_argument("pattern", "(?P<id>")),
                        "parser.pattern",
                    ),
                    (
                        "parser: {format: delimited, delimiter: '\"'}",
                        spec(invalid_argument("delimiter", "\"")),
                        "parser.delimiter",
                    ),
                    (
                        "parser: {format: delimited, delimiter: ',', bodies: [{path: body, format: json}]}",
                        spec(SpecError::InvalidPath("body".into())),
                        "parser.bodies[0].path",
                    ),
                    (
                        "parser: {format: delimited, delimiter: ',', bodies: [{path: '1', format: json, fields: {id: order_id}}]}",
                        spec(SpecError::InvalidPath("order_id".into())),
                        "parser.bodies[0].fields.id",
                    ),
                    (
                        "parser: {format: delimited, delimiter: ',', fields: {id: '0'}, bodies: [{path: '1', format: json, fields: {id: /id}}]}",
                        spec(SpecError::DuplicateField("id".into())),
                        "parser.bodies[0].fields.id",
                    ),
                ];
                for (text, kind, path) in cases {
                    let err = build_error(text, &registry);
                    assert_eq!(location(&err), (&kind, path, None), "{text}");
                }
            }

            #[test]
            fn builds_bodies_recursively() {
                let specs = build_yaml(
                    r#"
parser:
  format: regex
  pattern: '^(?P<time>\S+) (?P<app>[^\[]+)\[(?P<pid>\d+)\]:(?: (?P<body>.*))?$'
  fields: {time: time, app: app}
  bodies:
    - path: body
      format: json
      fields: {order_id: /order_id}
      bodies:
        - path: /items
          format: delimited
          delimiter: ";"
          fields: {first_item: "0"}
"#,
                    &Registry::new(),
                )
                .expect("specs are built");
                let items = ParserSpec::builder(Format::Delimited { delimiter: ';' })
                    .field("first_item", "0")
                    .build()
                    .expect("valid spec");
                let order = ParserSpec::builder(Format::Json)
                    .field("order_id", "/order_id")
                    .body("/items", items)
                    .build()
                    .expect("valid spec");
                let expected = ParserSpec::builder(Format::Regex {
                    pattern: r"^(?P<time>\S+) (?P<app>[^\[]+)\[(?P<pid>\d+)\]:(?: (?P<body>.*))?$"
                        .into(),
                })
                .field("time", "time")
                .field("app", "app")
                .body("body", order)
                .build()
                .expect("valid spec");
                assert_eq!(specs.parser(), Some(&expected));
            }
        }

        mod kv {
            use moldette_config::Registry;
            use moldette_parser::{Format, ParserSpec};

            use crate::common::build_yaml;

            #[test]
            fn builds_a_parser_only_section() {
                let specs = build_yaml(
                    "parser: {format: kv, pair_delimiter: ' ', key_value_delimiter: '=', fields: {id: id}}",
                    &Registry::new(),
                )
                .expect("specs are built");
                let expected = ParserSpec::builder(Format::Kv {
                    pair_delimiter: ' ',
                    key_value_delimiter: '=',
                })
                .field("id", "id")
                .build()
                .expect("valid spec");
                assert_eq!(specs.parser(), Some(&expected));
                assert!(specs.schemas().is_empty());
                assert!(specs.mapper().is_none() && specs.resp().is_none());
            }
        }

        mod regex {
            use moldette_parser::{Format, ParserSpec};

            use crate::access_log_specs;

            #[test]
            fn builds_the_access_log_parser_spec() {
                let expected = [
                    "remote_addr",
                    "time_local",
                    "method",
                    "uri",
                    "protocol",
                    "status",
                    "bytes",
                    "referer",
                    "agent",
                    "forwarded_for",
                ]
                .into_iter()
                .fold(
                    ParserSpec::builder(Format::Regex {
                        pattern: r#"^(?P<remote_addr>\S+) \S+ \S+ \[(?P<time_local>[^\]]+)\] "(?P<method>\S+) (?P<uri>\S+) (?P<protocol>[^"]+)" (?P<status>\d{3}) (?P<bytes>\d+|-) "(?P<referer>[^"]*)" "(?P<agent>[^"]*)" "(?P<forwarded_for>[^"]*)"$"#
                            .into(),
                    }),
                    |builder, name| builder.field(name, name),
                )
                .empty_as_none()
                .build()
                .expect("valid spec");
                for specs in access_log_specs() {
                    assert_eq!(specs.parser(), Some(&expected));
                }
            }
        }

        mod custom {
            use crate::common::{FIXED_ORDERS, build_yaml, registry};

            #[test]
            fn builds_caller_defined_formats() {
                let specs = build_yaml(FIXED_ORDERS, &registry()).expect("specs are built");
                let record = specs
                    .parser()
                    .expect("parser spec")
                    .parse("A-1...paid....12.50...")
                    .expect("record is parsed");
                let fields: Vec<_> = record.iter().collect();
                assert_eq!(
                    fields,
                    [("order_id", "A-1"), ("status", "paid"), ("amount", "12.50")]
                );
            }
        }
    }

    mod mapper {
        mod spec {
            use moldette_config::Registry;
            use moldette_core::{Field, OutputSchema, SpecError, Type};
            use moldette_mapper::{MapperSpec, Rule, TimestampDefaults, Unmatched};

            use crate::common::{DEPRECATED_FIELDS, build_yaml, invalid_argument, location, spec};
            use crate::{access_log_specs, access_schema, deprecated_schema, mapper_error};

            #[test]
            fn wraps_spec_errors_of_rules_with_key_path() {
                let err = mapper_error(
                    "    - {target: id, from: id}\n    - {target: discount, from: discount}\n",
                );
                assert_eq!(
                    location(&err),
                    (
                        &spec(SpecError::UnknownField("discount".into())),
                        "mapper.rules[1].target",
                        None
                    )
                );
                assert_eq!(
                    err.to_string(),
                    "mapper.rules[1].target: field `discount` is not in the output schema"
                );

                let cases = [
                    (
                        "    - {target: id, from: id}\n    - {target: amount, from: total}\n    - {target: id, from: order_id}\n",
                        spec(SpecError::DuplicateRule("id".into())),
                        "mapper.rules[2].target",
                    ),
                    (
                        "    - {target: id, coalesce: []}\n",
                        spec(invalid_argument("coalesce.from", "")),
                        "mapper.rules[0].coalesce",
                    ),
                    (
                        "    - target: id\n      from: id\n      via: [trim, {extract: {pattern: '^(\\d)'}}, {extract: {pattern: '('}}]\n",
                        spec(invalid_argument("extract.pattern", "(")),
                        "mapper.rules[0].via[2].extract.pattern",
                    ),
                    (
                        "    - target: amount\n      from: total\n      via: [{extract: {pattern: '('}}]\n    - target: id\n      from: id\n      via: [{extract: {pattern: '('}}]\n",
                        spec(invalid_argument("extract.pattern", "(")),
                        "mapper.rules[1].via[0].extract.pattern",
                    ),
                    (
                        "    - target: placed_at\n      from: time\n      via: [{timestamp: {formats: ['[year]-[month]-[day]', '[yaer]']}}]\n",
                        spec(invalid_argument("timestamp.formats", "[yaer]")),
                        "mapper.rules[0].via[0].timestamp.formats[1]",
                    ),
                    (
                        "    - target: placed_at\n      from: time\n      via: [{timestamp: {formats: []}}]\n",
                        spec(invalid_argument("timestamp.formats", "")),
                        "mapper.rules[0].via[0].timestamp.formats",
                    ),
                    (
                        "    - target: placed_at\n      from: time\n      via: [trim, {timestamp: {formats: ['[month repr:short] [day]'], year: 10000}}]\n",
                        spec(invalid_argument("timestamp.year", "10000")),
                        "mapper.rules[0].via[1].timestamp.year",
                    ),
                    (
                        "    - {target: id, from: id}\n    - {target: amount, from: total, default: none}\n",
                        spec(invalid_argument("default", "none")),
                        "mapper.rules[1].default",
                    ),
                    (
                        "    - target: placed_at\n      from: time\n      via: [{timestamp: {formats: ['[month repr:short] [day]'], offset: '09:00'}}]\n",
                        spec(invalid_argument("timestamp.offset", "09:00")),
                        "mapper.rules[0].via[0].timestamp.offset",
                    ),
                ];
                for (rules, kind, path) in cases {
                    let err = mapper_error(rules);
                    assert_eq!(location(&err), (&kind, path, None), "{rules}");
                }
            }

            #[test]
            fn builds_the_access_log_rules() {
                let expected = MapperSpec::builder(access_schema())
                    .rule(
                        Rule::new("client", "forwarded_for")
                            .fallback("remote_addr")
                            .split(',', 0)
                            .trim(),
                    )
                    .rule(Rule::new("time", "time_local").timestamp([
                        "[day]/[month repr:short]/[year]:[hour]:[minute]:[second] [offset_hour][offset_minute]",
                    ]))
                    .rule(Rule::new("path", "uri").split('?', 0))
                    .rule(Rule::new("http_version", "protocol").extract(r"^HTTP/(\d(?:\.\d)?)$"))
                    .rule(Rule::new("status_class", "status").extract(r"^(\d)").lookup([
                        ("1", "informational"),
                        ("2", "success"),
                        ("3", "redirection"),
                        ("4", "client_error"),
                        ("5", "server_error"),
                    ]))
                    .rule(Rule::new("user_agent", "agent"))
                    .rule(Rule::constant("site", "web-01"))
                    .build()
                    .expect("valid spec");
                for specs in access_log_specs() {
                    assert_eq!(specs.mapper(), Some(&expected));
                }
            }

            #[test]
            fn builds_a_mapper_section_without_parser_and_resp() {
                let specs = build_yaml(
                    r#"
schemas:
  - {name: order, version: 1, fields: [{name: id, type: string}, {name: note, type: string}]}
mapper:
  schema: {name: order, version: 1}
  explicit_rules: true
  rules:
    - {target: id, from: order_id}
"#,
                    &Registry::new(),
                )
                .expect("specs are built");
                assert!(specs.parser().is_none() && specs.resp().is_none());
                let schema = OutputSchema::builder("order", 1)
                    .field(Field::new("id", Type::String))
                    .field(Field::new("note", Type::String))
                    .build()
                    .expect("valid schema");
                let rule = || Rule::new("id", "order_id");
                let explicit = MapperSpec::builder(schema.clone())
                    .explicit_rules()
                    .rule(rule())
                    .build()
                    .expect("valid spec");
                let same_name = MapperSpec::builder(schema)
                    .rule(rule())
                    .build()
                    .expect("valid spec");
                assert_eq!(specs.mapper(), Some(&explicit));
                assert_ne!(specs.mapper(), Some(&same_name));
            }

            #[test]
            fn builds_timestamp_defaults() {
                let specs = build_yaml(
                    r#"
schemas:
  - {name: login, version: 1, fields: [{name: at, type: timestamp}]}
mapper:
  schema: {name: login, version: 1}
  rules:
    - target: at
      from: time
      via:
        - timestamp:
            formats: ["[month repr:short] [day padding:space] [hour]:[minute]:[second]"]
            year: 2023
            offset: "+09:00"
"#,
                    &Registry::new(),
                )
                .expect("specs are built");
                let schema = OutputSchema::builder("login", 1)
                    .field(Field::new("at", Type::Timestamp))
                    .build()
                    .expect("valid schema");
                let expected = MapperSpec::builder(schema)
                    .rule(Rule::new("at", "time").timestamp_with(
                        ["[month repr:short] [day padding:space] [hour]:[minute]:[second]"],
                        TimestampDefaults::new().year(2023).offset("+09:00"),
                    ))
                    .build()
                    .expect("valid spec");
                assert_eq!(specs.mapper(), Some(&expected));
            }

            #[test]
            fn builds_lookup_values_for_texts_outside_the_table() {
                let specs = build_yaml(
                    r#"
schemas:
  - {name: event, version: 1, fields: [{name: action, type: string}, {name: outcome, type: string}, {name: kind, type: string}]}
mapper:
  schema: {name: event, version: 1}
  rules:
    - {target: action, from: id, via: [{lookup: {table: {"1": create}, keep_unmatched: true}}]}
    - {target: outcome, from: error, via: [{lookup: {table: {}, default: failure}}]}
    - {target: kind, from: id, via: [{lookup: {table: {"1": create}, keep_unmatched: false}}]}
"#,
                    &Registry::new(),
                )
                .expect("specs are built");
                let schema = OutputSchema::builder("event", 1)
                    .field(Field::new("action", Type::String))
                    .field(Field::new("outcome", Type::String))
                    .field(Field::new("kind", Type::String))
                    .build()
                    .expect("valid schema");
                let no_entries: [(&str, &str); 0] = [];
                let expected = MapperSpec::builder(schema)
                    .rule(Rule::new("action", "id").lookup_with([("1", "create")], Unmatched::Keep))
                    .rule(
                        Rule::new("outcome", "error")
                            .lookup_with(no_entries, Unmatched::Value("failure".into())),
                    )
                    .rule(Rule::new("kind", "id").lookup([("1", "create")]))
                    .build()
                    .expect("valid spec");
                assert_eq!(specs.mapper(), Some(&expected));
            }

            #[test]
            fn builds_rule_defaults() {
                let specs = build_yaml(
                    r#"
schemas:
  - {name: event, version: 1, fields: [{name: outcome, type: string}, {name: attempts, type: int}]}
mapper:
  schema: {name: event, version: 1}
  rules:
    - {target: outcome, from: error, via: [{lookup: {table: {}, default: failure}}], default: success}
    - {target: attempts, coalesce: [tries, retries], default: "0"}
"#,
                    &Registry::new(),
                )
                .expect("specs are built");
                let schema = OutputSchema::builder("event", 1)
                    .field(Field::new("outcome", Type::String))
                    .field(Field::new("attempts", Type::Int))
                    .build()
                    .expect("valid schema");
                let no_entries: [(&str, &str); 0] = [];
                let expected = MapperSpec::builder(schema)
                    .rule(
                        Rule::new("outcome", "error")
                            .lookup_with(no_entries, Unmatched::Value("failure".into()))
                            .default("success"),
                    )
                    .rule(Rule::coalesce("attempts", ["tries", "retries"]).default("0"))
                    .build()
                    .expect("valid spec");
                assert_eq!(specs.mapper(), Some(&expected));
            }

            #[test]
            fn builds_rules_of_deprecated_fields() {
                let specs =
                    build_yaml(DEPRECATED_FIELDS, &Registry::new()).expect("specs are built");
                let expected = MapperSpec::builder(deprecated_schema())
                    .rule(Rule::new("amount", "total"))
                    .rule(Rule::constant("channel", "web"))
                    .build()
                    .expect("valid spec");
                assert_eq!(specs.mapper(), Some(&expected));
            }
        }

        mod op {
            use moldette_core::{FieldRecord, Value};

            use crate::common::{FIXED_ORDERS, build_yaml, registry, text};

            #[test]
            fn builds_caller_defined_ops() {
                let specs = build_yaml(FIXED_ORDERS, &registry()).expect("specs are built");
                let mut record = FieldRecord::new();
                record.insert("order_id", "A-1");
                record.insert("status", "paid");
                record.insert("amount", "12.50");
                let mapped = specs.mapper().expect("mapper spec").map(&record);
                assert_eq!(mapped.get("order_id"), Some(&text("00000A-1")));
                assert_eq!(mapped.get("status"), Some(&text("SETTLED")));
                assert_eq!(mapped.get("amount"), Some(&Value::Float(12.5)));
            }
        }
    }

    mod resp {
        mod spec {
            use moldette_config::Registry;
            use moldette_core::{Field, OutputSchema, Type};
            use moldette_resp::RespSpec;

            use crate::common::{DEPRECATED_FIELDS, build_yaml};
            use crate::{access_log_specs, access_schema, deprecated_schema};

            #[test]
            fn builds_a_resp_section_for_the_referenced_version() {
                let specs = build_yaml(
                    r#"
schemas:
  - {name: order, version: 1, fields: [{name: id, type: string, required: true}]}
  - {name: order, version: 2, fields: [{name: id, type: string}]}
resp: {schema: {name: order, version: 2}, strict: true}
"#,
                    &Registry::new(),
                )
                .expect("specs are built");
                assert_eq!(specs.schemas().len(), 2);
                assert!(specs.parser().is_none() && specs.mapper().is_none());
                let schema = OutputSchema::builder("order", 2)
                    .field(Field::new("id", Type::String))
                    .build()
                    .expect("valid schema");
                assert_eq!(
                    specs.resp(),
                    Some(&RespSpec::builder(schema.clone()).strict().build())
                );
                assert_ne!(specs.resp(), Some(&RespSpec::builder(schema).build()));
            }

            #[test]
            fn builds_the_access_log_resp_spec() {
                let expected = RespSpec::builder(access_schema()).build();
                for specs in access_log_specs() {
                    assert_eq!(specs.resp(), Some(&expected));
                }
            }

            #[test]
            fn builds_a_resp_section_with_deprecated_fields() {
                let specs =
                    build_yaml(DEPRECATED_FIELDS, &Registry::new()).expect("specs are built");
                assert_eq!(
                    specs.resp(),
                    Some(&RespSpec::builder(deprecated_schema()).build())
                );
            }
        }
    }
}
