use std::borrow::Cow;
use std::ops::Range;
use std::sync::Arc;
use std::thread;

use moldette_config::{
    Arguments, Config, LoadError, LoadErrorKind, Registry, RegistryError, Specs, ValueKind,
};
use moldette_core::{Malformed, Response, SpecError, Timestamp, Value};
use moldette_mapper::Op;
use moldette_parser::{Format, Parser};

const ACCESS_LINES: [&str; 3] = [
    r#"203.0.113.7 - - [18/Nov/2026:10:15:32 +0000] "GET /orders?id=A-1 HTTP/1.1" 200 512 "-" "curl/8.5.0" "198.51.100.2, 203.0.113.7""#,
    r#"203.0.113.9 - - [18/Nov/2026:10:15:33 +0900] "POST /orders HTTP/2.0" 404 0 "https://shop.example/cart" "Mozilla/5.0" "-""#,
    r#"203.0.113.9 - - [yesterday] "GET / HTTP/1.1" 200 12 "-" "curl/8.5.0" "-""#,
];

fn build_yaml(text: &str, registry: &Registry) -> Result<Specs, LoadError> {
    Config::from_yaml(text).expect("valid YAML").build(registry)
}

fn build_error(text: &str, registry: &Registry) -> LoadError {
    build_yaml(text, registry).expect_err("configuration is rejected")
}

fn location(err: &LoadError) -> (&LoadErrorKind, &str, Option<(usize, usize)>) {
    (
        err.kind(),
        err.path(),
        err.position().map(|p| (p.line(), p.column())),
    )
}

fn spec(error: SpecError) -> LoadErrorKind {
    LoadErrorKind::Spec(error)
}

fn invalid_argument(name: &str, value: &str) -> SpecError {
    SpecError::InvalidArgument {
        name: name.into(),
        value: value.into(),
    }
}

fn respond<'a>(specs: &'a Specs, raw: &'a str) -> Response<'a> {
    let record = specs
        .parser()
        .expect("parser spec")
        .parse(raw)
        .expect("record is parsed");
    let mapped = specs.mapper().expect("mapper spec").map(&record);
    specs
        .resp()
        .expect("resp spec")
        .respond(mapped)
        .into_owned()
}

fn text(value: &str) -> Value<'static> {
    Value::Str(Cow::Owned(value.to_owned()))
}

#[test]
fn builds_specs_that_parse_map_and_respond_to_access_log_records() {
    let registry = Registry::new();
    for config in [
        Config::from_yaml(include_str!("fixtures/nginx_access.yaml")).expect("valid YAML"),
        Config::from_json(include_str!("fixtures/nginx_access.json")).expect("valid JSON"),
    ] {
        let specs = config.build(&registry).expect("specs are built");
        let schema = &specs.schemas()[0];
        assert_eq!((schema.name(), schema.version()), ("access", 1));
        assert_eq!(specs.mapper().map(|mapper| mapper.schema()), Some(schema));
        assert_eq!(specs.resp().map(|resp| resp.schema()), Some(schema));

        let Response::Ok(first) = respond(&specs, ACCESS_LINES[0]) else {
            panic!("first record is ok");
        };
        let expected = [
            ("client", Value::Ip("198.51.100.2".parse().expect("ip"))),
            (
                "time",
                Value::Timestamp(Timestamp::new(1_794_996_932, 0, 0).expect("timestamp")),
            ),
            ("method", text("GET")),
            ("path", text("/orders")),
            ("http_version", text("1.1")),
            ("status", Value::Int(200)),
            ("status_class", text("success")),
            ("bytes", Value::Int(512)),
            ("user_agent", text("curl/8.5.0")),
            ("referer", text("-")),
            ("site", text("web-01")),
        ];
        for (name, value) in &expected {
            assert_eq!(first.get(name), Some(value), "{name}");
        }
        assert_eq!(first.iter().count(), expected.len());

        let Response::Ok(second) = respond(&specs, ACCESS_LINES[1]) else {
            panic!("second record is ok");
        };
        assert_eq!(
            second.get("client"),
            Some(&Value::Ip("203.0.113.9".parse().expect("ip")))
        );
        assert_eq!(
            second.get("time"),
            Some(&Value::Timestamp(
                Timestamp::new(1_794_964_533, 0, 32_400).expect("timestamp")
            ))
        );
        assert_eq!(second.get("status_class"), Some(&text("client_error")));
        assert_eq!(second.get("http_version"), Some(&text("2.0")));

        assert_eq!(
            respond(&specs, ACCESS_LINES[2]),
            Response::MissingRequired(vec!["time".into()])
        );
    }
}

#[test]
fn shares_specs_across_threads() {
    let specs = build_yaml(include_str!("fixtures/nginx_access.yaml"), &Registry::new())
        .expect("specs are built");
    let expected: Vec<_> = ACCESS_LINES
        .iter()
        .map(|line| respond(&specs, line))
        .collect();
    thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    ACCESS_LINES
                        .iter()
                        .map(|line| respond(&specs, line))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for worker in workers {
            assert_eq!(worker.join().expect("worker finishes"), expected);
        }
    });
}

#[test]
fn builds_only_the_sections_present() {
    let registry = Registry::new();
    let parser_only = build_yaml(
        "parser: {format: kv, pair_delimiter: ' ', key_value_delimiter: '=', fields: {id: id}}",
        &registry,
    )
    .expect("specs are built");
    let record = parser_only
        .parser()
        .expect("parser spec")
        .parse("id=A-1 amount=12.50")
        .expect("record is parsed");
    assert_eq!(record.get("id"), Some("A-1"));
    assert!(parser_only.schemas().is_empty());
    assert!(parser_only.mapper().is_none() && parser_only.resp().is_none());

    let schemas_and_mapper = build_yaml(
        r#"
schemas:
  - {name: order, version: 1, fields: [{name: id, type: string}, {name: note, type: string}]}
mapper:
  schema: {name: order, version: 1}
  explicit_rules: true
  rules:
    - {target: id, from: order_id}
"#,
        &registry,
    )
    .expect("specs are built");
    assert!(schemas_and_mapper.parser().is_none() && schemas_and_mapper.resp().is_none());
    let mut record = moldette_core::FieldRecord::new();
    record.insert("order_id", "A-1");
    record.insert("note", "gift");
    let mapped = schemas_and_mapper
        .mapper()
        .expect("mapper spec")
        .map(&record);
    assert_eq!(mapped.get("id"), Some(&Value::Str("A-1".into())));
    assert_eq!(mapped.get("note"), None);

    let schemas_and_resp = build_yaml(
        r#"
schemas:
  - {name: order, version: 1, fields: [{name: id, type: string, required: true}]}
  - {name: order, version: 2, fields: [{name: id, type: string}]}
resp: {schema: {name: order, version: 2}, strict: true}
"#,
        &registry,
    )
    .expect("specs are built");
    assert_eq!(schemas_and_resp.schemas().len(), 2);
    let resp = schemas_and_resp.resp().expect("resp spec");
    assert_eq!(resp.schema().version(), 2);
    let mut mapped = moldette_core::Mapped::new();
    mapped.mark_emptied("id");
    assert_eq!(resp.respond(mapped), Response::Emptied(vec!["id".into()]));
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
    let record = specs
        .parser()
        .expect("parser spec")
        .parse(r#"2026-11-18T10:00:00Z shop[42]: {"order_id":"A-1","items":"B-7;C-3"}"#)
        .expect("record is parsed");
    let fields: Vec<_> = record.iter().collect();
    assert_eq!(
        fields,
        [
            ("time", "2026-11-18T10:00:00Z"),
            ("app", "shop"),
            ("order_id", "A-1"),
            ("first_item", "B-7")
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

const ORDER_SCHEMA: &str = r#"
schemas:
  - name: order
    version: 1
    fields:
      - {name: id, type: string}
      - {name: amount, type: float}
      - {name: placed_at, type: timestamp}
"#;

fn mapper_error(rules: &str) -> LoadError {
    build_error(
        &format!("{ORDER_SCHEMA}mapper:\n  schema: {{name: order, version: 1}}\n  rules:\n{rules}"),
        &Registry::new(),
    )
}

#[test]
fn wraps_spec_errors_of_schemas_and_parsers_with_key_path() {
    let registry = Registry::new();
    let err = build_error(
        "schemas:\n  - {name: order, version: 1, fields: [{name: id, type: string}, {name: id, type: int}]}\n",
        &registry,
    );
    assert_eq!(
        location(&err),
        (
            &spec(SpecError::DuplicateField("id".into())),
            "schemas[0].fields[1].name",
            None
        )
    );

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
fn wraps_spec_errors_of_mapper_rules_with_key_path() {
    let err =
        mapper_error("    - {target: id, from: id}\n    - {target: discount, from: discount}\n");
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
    ];
    for (rules, kind, path) in cases {
        let err = mapper_error(rules);
        assert_eq!(location(&err), (&kind, path, None), "{rules}");
    }
}

#[derive(Debug)]
struct Pad {
    width: usize,
    fill: char,
}

impl Op for Pad {
    fn apply<'a>(&'a self, text: &'a str) -> Option<Cow<'a, str>> {
        let missing = self.width.saturating_sub(text.chars().count());
        if missing == 0 {
            return Some(Cow::Borrowed(text));
        }
        Some(Cow::Owned(
            std::iter::repeat_n(self.fill, missing)
                .chain(text.chars())
                .collect(),
        ))
    }

    fn name(&self) -> &str {
        "pad"
    }
}

fn pad(arguments: &mut Arguments) -> Result<Arc<dyn Op>, LoadError> {
    let width = arguments.integer("width")?;
    if width == 0 {
        return Err(arguments.spec_error("width", invalid_argument("pad.width", "0")));
    }
    let fill = if arguments.contains_key("fill") {
        arguments.character("fill")?
    } else {
        ' '
    };
    Ok(Arc::new(Pad { width, fill }))
}

#[derive(Debug)]
struct Upper;

impl Op for Upper {
    fn apply<'a>(&'a self, text: &'a str) -> Option<Cow<'a, str>> {
        Some(Cow::Owned(text.to_uppercase()))
    }

    fn name(&self) -> &str {
        "upper"
    }
}

#[derive(Debug)]
struct Replace(Vec<(String, String)>);

impl Op for Replace {
    fn apply<'a>(&'a self, text: &'a str) -> Option<Cow<'a, str>> {
        let replaced = self.0.iter().find(|(from, _)| from == text);
        Some(Cow::Borrowed(replaced.map_or(text, |(_, to)| to.as_str())))
    }

    fn name(&self) -> &str {
        "replace"
    }
}

fn replace(arguments: &mut Arguments) -> Result<Arc<dyn Op>, LoadError> {
    let pairs = arguments
        .map("pairs")?
        .into_iter()
        .map(|(from, to)| Ok((from.to_owned(), to.string()?)))
        .collect::<Result<_, LoadError>>()?;
    Ok(Arc::new(Replace(pairs)))
}

#[derive(Debug)]
struct FixedWidth {
    columns: Vec<Range<usize>>,
    padding: char,
}

impl Parser for FixedWidth {
    type Path = Range<usize>;

    fn name(&self) -> &str {
        "fixed_width"
    }

    fn path(&self, path: &str) -> Option<Range<usize>> {
        self.columns.get(path.parse::<usize>().ok()?).cloned()
    }

    fn read<'a>(
        &self,
        raw: &'a str,
        paths: &[Range<usize>],
        values: &mut [Option<Cow<'a, str>>],
    ) -> Result<(), Malformed> {
        for (path, value) in paths.iter().zip(values) {
            let column = raw
                .get(path.clone())
                .ok_or_else(|| Malformed::new("record is shorter than its columns"))?;
            *value = Some(Cow::Borrowed(column.trim_end_matches(self.padding)));
        }
        Ok(())
    }
}

fn fixed_width(arguments: &mut Arguments) -> Result<Format, LoadError> {
    let padding = if arguments.contains_key("padding") {
        arguments.character("padding")?
    } else {
        ' '
    };
    let mut columns = Vec::new();
    let mut start = 0;
    for width in arguments.list("widths")? {
        let size: usize = width.integer()?;
        if size == 0 {
            return Err(width.spec_error(invalid_argument("fixed_width.widths", "0")));
        }
        columns.push(start..start + size);
        start += size;
    }
    Ok(Format::custom(FixedWidth { columns, padding }))
}

fn registry() -> Registry {
    Registry::new()
        .op("pad", pad)
        .and_then(|registry| registry.op("replace", replace))
        .and_then(|registry| {
            registry.op("upper", |_: &mut Arguments| {
                Ok(Arc::new(Upper) as Arc<dyn Op>)
            })
        })
        .and_then(|registry| registry.format("fixed_width", fixed_width))
        .expect("names are free")
}

const FIXED_ORDERS: &str = r#"
schemas:
  - name: order
    version: 1
    fields:
      - {name: order_id, type: string, required: true}
      - {name: status, type: string}
      - {name: amount, type: float}
parser:
  format: fixed_width
  padding: "."
  widths: [6, 8, 8]
  fields: {order_id: "0", status: "1", amount: "2"}
mapper:
  schema: {name: order, version: 1}
  rules:
    - target: order_id
      from: order_id
      via:
        - pad: {width: 8, fill: "0"}
    - target: status
      from: status
      via:
        - replace: {pairs: {paid: settled, open: pending}}
        - upper
resp:
  schema: {name: order, version: 1}
"#;

#[test]
fn builds_caller_defined_ops_and_formats() {
    let specs = build_yaml(FIXED_ORDERS, &registry()).expect("specs are built");
    let Response::Ok(mapped) = respond(&specs, "A-1...paid....12.50...") else {
        panic!("record is ok");
    };
    assert_eq!(mapped.get("order_id"), Some(&text("00000A-1")));
    assert_eq!(mapped.get("status"), Some(&text("SETTLED")));
    assert_eq!(mapped.get("amount"), Some(&Value::Float(12.5)));
}

#[test]
fn reads_caller_defined_arguments_with_key_path_and_position() {
    let registry = registry();
    let cases = [
        (
            FIXED_ORDERS.replace("width: 8", "widht: 8"),
            LoadErrorKind::MissingKey("width".into()),
            "mapper.rules[0].via[0].pad",
            Some((20, 16)),
        ),
        (
            FIXED_ORDERS.replace("width: 8", "width: wide"),
            LoadErrorKind::TypeMismatch(ValueKind::Integer),
            "mapper.rules[0].via[0].pad.width",
            Some((20, 24)),
        ),
        (
            FIXED_ORDERS.replace("width: 8", "width: 0"),
            spec(invalid_argument("pad.width", "0")),
            "mapper.rules[0].via[0].pad.width",
            Some((20, 24)),
        ),
        (
            FIXED_ORDERS.replace("fill: \"0\"", "fill: \"00\""),
            LoadErrorKind::TypeMismatch(ValueKind::Character),
            "mapper.rules[0].via[0].pad.fill",
            Some((20, 33)),
        ),
        (
            FIXED_ORDERS.replace("[6, 8, 8]", "[6, eight, 8]"),
            LoadErrorKind::TypeMismatch(ValueKind::Integer),
            "parser.widths[1]",
            Some((12, 15)),
        ),
        (
            FIXED_ORDERS.replace("[6, 8, 8]", "[6, 0, 8]"),
            spec(invalid_argument("fixed_width.widths", "0")),
            "parser.widths[1]",
            Some((12, 15)),
        ),
        (
            FIXED_ORDERS.replace("[6, 8, 8]", "6"),
            LoadErrorKind::TypeMismatch(ValueKind::Sequence),
            "parser.widths",
            Some((12, 11)),
        ),
        (
            FIXED_ORDERS.replace("paid: settled", "paid: [settled]"),
            LoadErrorKind::TypeMismatch(ValueKind::String),
            "mapper.rules[1].via[0].replace.pairs.paid",
            Some((24, 35)),
        ),
        (
            FIXED_ORDERS.replace("{pairs: {paid: settled, open: pending}}", "{pairs: [paid]}"),
            LoadErrorKind::TypeMismatch(ValueKind::Map),
            "mapper.rules[1].via[0].replace.pairs",
            Some((24, 28)),
        ),
    ];
    for (text, kind, path, position) in cases {
        let err = build_error(&text, &registry);
        assert_eq!(location(&err), (&kind, path, position), "{path}");
    }
}

#[test]
fn rejects_arguments_the_constructor_does_not_read() {
    let registry = registry();
    let err = build_error(
        &FIXED_ORDERS.replace("width: 8, fill", "width: 8, fil"),
        &registry,
    );
    assert_eq!(
        location(&err),
        (
            &LoadErrorKind::UnknownKey("fil".into()),
            "mapper.rules[0].via[0].pad",
            Some((20, 27))
        )
    );

    let err = build_error(&FIXED_ORDERS.replace("  padding:", "  paddng:"), &registry);
    assert_eq!(
        location(&err),
        (
            &LoadErrorKind::UnknownKey("paddng".into()),
            "parser",
            Some((11, 3))
        )
    );

    let err = build_error(&FIXED_ORDERS.replace("  widths:", "  width:"), &registry);
    assert_eq!(
        location(&err),
        (
            &LoadErrorKind::MissingKey("widths".into()),
            "parser",
            Some((10, 11))
        )
    );

    let err = build_error(
        &FIXED_ORDERS.replace("- upper", "- upper: {case: title}"),
        &registry,
    );
    assert_eq!(
        location(&err),
        (
            &LoadErrorKind::UnknownKey("case".into()),
            "mapper.rules[1].via[1].upper",
            Some((25, 19))
        )
    );
}

#[test]
fn rejects_names_missing_from_the_registry() {
    let err = build_error(FIXED_ORDERS, &Registry::new());
    assert_eq!(
        location(&err),
        (
            &LoadErrorKind::UnknownFormat("fixed_width".into()),
            "parser",
            Some((10, 11))
        )
    );

    let err = build_error(&FIXED_ORDERS.replace("- upper", "- uper"), &registry());
    assert_eq!(
        location(&err),
        (
            &LoadErrorKind::UnknownOp("uper".into()),
            "mapper.rules[1].via[1].uper",
            Some((25, 11))
        )
    );
}

#[test]
fn rejects_built_in_and_repeated_names() {
    assert_eq!(
        Registry::new().op("trim", pad).map(drop),
        Err(RegistryError::BuiltInName("trim".into()))
    );
    assert_eq!(
        Registry::new().format("json", fixed_width).map(drop),
        Err(RegistryError::BuiltInName("json".into()))
    );
    assert_eq!(
        registry().op("pad", pad).map(drop),
        Err(RegistryError::DuplicateName("pad".into()))
    );
    assert!(Registry::new().op("json", pad).is_ok());
}
