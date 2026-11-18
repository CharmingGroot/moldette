#![cfg(feature = "config")]

use std::borrow::Cow;

use moldette::config::{Config, Registry, Specs};
use moldette::{Malformed, Mapped, Response, StatusCounts, Timestamp, Value};

const ACCESS_LOG_YAML: &str =
    include_str!("../../moldette-config/tests/fixtures/nginx_access.yaml");
const ACCESS_LOG_JSON: &str =
    include_str!("../../moldette-config/tests/fixtures/nginx_access.json");
const ACCESS_LOG: &str = include_str!("../../moldette-config/tests/fixtures/nginx_access.log");
const ORDER_BODIES: &str = include_str!("../../moldette-config/tests/fixtures/order_bodies.yaml");
const ORDER_API_YAML: &str = include_str!("../../moldette-config/tests/fixtures/order_api.yaml");
const ORDER_API_NDJSON: &str =
    include_str!("../../moldette-config/tests/fixtures/order_api.ndjson");
const ORDER_EXPORT_YAML: &str =
    include_str!("../../moldette-config/tests/fixtures/order_export.yaml");
const ORDER_EXPORT_CSV: &str =
    include_str!("../../moldette-config/tests/fixtures/order_export.csv");

fn specs(yaml: &str) -> Specs {
    Config::from_yaml(yaml)
        .expect("valid YAML")
        .build(&Registry::new())
        .expect("specs are built")
}

fn text(value: &str) -> Value<'static> {
    Value::Str(Cow::Owned(value.to_owned()))
}

fn ip(value: &str) -> Value<'static> {
    Value::Ip(value.parse().expect("ip"))
}

fn timestamp(seconds: i64, nanos: u32, offset_seconds: i32) -> Value<'static> {
    Value::Timestamp(Timestamp::new(seconds, nanos, offset_seconds).expect("timestamp"))
}

fn ok(
    values: impl IntoIterator<Item = (&'static str, Value<'static>)>,
    emptied: &[&'static str],
) -> Response<'static> {
    let mut mapped = Mapped::new();
    for (name, value) in values {
        mapped.insert(name, value);
    }
    for name in emptied {
        mapped.mark_emptied(*name);
    }
    Response::Ok(mapped)
}

fn missing(fields: &[&'static str]) -> Response<'static> {
    Response::MissingRequired(fields.iter().map(|field| Cow::Borrowed(*field)).collect())
}

fn malformed(reason: &'static str) -> Response<'static> {
    Response::Malformed(Malformed::new(reason))
}

fn assert_responses(actual: &[Response<'_>], expected: &[Response<'_>]) {
    assert_eq!(actual.len(), expected.len(), "record count");
    for (record, (actual, expected)) in (1..).zip(actual.iter().zip(expected)) {
        match (actual, expected) {
            (Response::Malformed(actual), Response::Malformed(expected)) => assert!(
                actual.reason().contains(expected.reason()),
                "record {record}: {}",
                actual.reason()
            ),
            _ => assert_eq!(actual, expected, "record {record}"),
        }
    }
}

fn counts(counts: StatusCounts) -> (u64, u64, u64, u64) {
    (
        counts.ok(),
        counts.malformed(),
        counts.missing_required(),
        counts.emptied(),
    )
}

mod regex {
    use std::sync::{Arc, RwLock};
    use std::thread;

    use moldette::config::{Config, Lineage, Registry, Specs};
    use moldette::{Pipeline, Response, StatusCounts, Timestamp, Value};

    use crate::{
        ACCESS_LOG, ACCESS_LOG_JSON, ACCESS_LOG_YAML, assert_responses, counts, ip, malformed,
        missing, ok, specs, text, timestamp,
    };

    const OK_LINES: [&str; 2] = [
        r#"203.0.113.7 - - [18/Nov/2026:10:15:32 +0000] "GET /orders?id=A-1 HTTP/1.1" 200 512 "-" "curl/8.5.0" "198.51.100.2, 203.0.113.7""#,
        r#"203.0.113.9 - - [18/Nov/2026:10:15:33 +0900] "POST /orders HTTP/2.0" 404 0 "https://shop.example/cart" "Mozilla/5.0" "-""#,
    ];
    const BAD_TIME_LINE: &str =
        r#"203.0.113.9 - - [yesterday] "GET / HTTP/1.1" 200 12 "-" "curl/8.5.0" "-""#;
    const MALFORMED_LINE: &str = "GET /orders 200";

    fn access_log() -> Pipeline {
        Pipeline::from_specs(&specs(ACCESS_LOG_YAML)).expect("all three sections")
    }

    fn batch() -> [&'static str; 5] {
        [
            MALFORMED_LINE,
            OK_LINES[0],
            BAD_TIME_LINE,
            MALFORMED_LINE,
            OK_LINES[1],
        ]
    }

    #[test]
    fn processes_access_log_file_with_fallback_and_malformed_lines() {
        let pipeline = access_log();

        let (responses, status_counts) = pipeline.process_batch(ACCESS_LOG.lines());

        assert_responses(
            &responses,
            &[
                ok(
                    [
                        ("client", ip("198.51.100.2")),
                        ("time", timestamp(1_794_996_932, 0, 0)),
                        ("method", text("GET")),
                        ("path", text("/orders")),
                        ("http_version", text("1.1")),
                        ("status", Value::Int(200)),
                        ("status_class", text("success")),
                        ("bytes", Value::Int(512)),
                        ("user_agent", text("curl/8.5.0")),
                        ("referer", text("-")),
                        ("site", text("web-01")),
                    ],
                    &[],
                ),
                ok(
                    [
                        ("client", ip("203.0.113.8")),
                        ("time", timestamp(1_794_996_940, 0, 0)),
                        ("method", text("POST")),
                        ("path", text("/orders")),
                        ("http_version", text("2.0")),
                        ("status", Value::Int(201)),
                        ("status_class", text("success")),
                        ("bytes", Value::Int(64)),
                        ("user_agent", text("Mozilla/5.0")),
                        ("referer", text("https://shop.example/cart")),
                        ("site", text("web-01")),
                    ],
                    &[],
                ),
                ok(
                    [
                        ("client", ip("203.0.113.9")),
                        ("time", timestamp(1_794_996_965, 0, 32_400)),
                        ("method", text("GET")),
                        ("path", text("/search")),
                        ("http_version", text("1.0")),
                        ("status", Value::Int(304)),
                        ("status_class", text("redirection")),
                        ("user_agent", text("Mozilla/5.0")),
                        ("site", text("web-01")),
                    ],
                    &["bytes"],
                ),
                ok(
                    [
                        ("client", ip("2001:db8::1")),
                        ("time", timestamp(1_794_996_970, 0, 0)),
                        ("method", text("DELETE")),
                        ("path", text("/orders/A-3")),
                        ("http_version", text("1.1")),
                        ("status", Value::Int(503)),
                        ("status_class", text("server_error")),
                        ("bytes", Value::Int(0)),
                        ("user_agent", text("curl/8.5.0")),
                        ("referer", text("-")),
                        ("site", text("web-01")),
                    ],
                    &[],
                ),
                missing(&["client"]),
                missing(&["time"]),
                malformed("record does not match the pattern"),
                malformed("record does not match the pattern"),
                ok(
                    [
                        ("client", ip("203.0.113.13")),
                        ("time", timestamp(1_794_997_000, 0, 0)),
                        ("method", text("GET")),
                        ("path", text("/health")),
                        ("http_version", text("3")),
                        ("status", Value::Int(200)),
                        ("status_class", text("success")),
                        ("bytes", Value::Int(2)),
                        ("user_agent", text("curl/8.5.0")),
                        ("referer", text("-")),
                        ("site", text("web-01")),
                    ],
                    &[],
                ),
            ],
        );
        assert_eq!(counts(status_counts), (5, 2, 2, 0));
    }

    #[test]
    fn processes_access_log_records_from_yaml_and_json() {
        for config in [
            Config::from_yaml(ACCESS_LOG_YAML).expect("valid YAML"),
            Config::from_json(ACCESS_LOG_JSON).expect("valid JSON"),
        ] {
            let specs = config.build(&Registry::new()).expect("specs are built");
            let schema = &specs.schemas()[0];
            assert_eq!((schema.name(), schema.version()), ("access", 1));
            assert_eq!(specs.mapper().map(|mapper| mapper.schema()), Some(schema));
            assert_eq!(specs.resp().map(|resp| resp.schema()), Some(schema));
            let pipeline = Pipeline::from_specs(&specs).expect("all three sections");

            let Response::Ok(first) = pipeline.process(OK_LINES[0]) else {
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

            let Response::Ok(second) = pipeline.process(OK_LINES[1]) else {
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
                pipeline.process(BAD_TIME_LINE),
                Response::MissingRequired(vec!["time".into()])
            );
            let Response::Malformed(malformed) = pipeline.process(MALFORMED_LINE) else {
                panic!("record is malformed");
            };
            assert!(!malformed.reason().is_empty());
        }
    }

    #[test]
    fn batch_keeps_input_order_and_counts_malformed_records() {
        let pipeline = access_log();
        let lines = batch();

        let (responses, status_counts) = pipeline.process_batch(lines);

        let expected: Vec<Response<'_>> = lines.iter().map(|line| pipeline.process(line)).collect();
        assert_eq!(responses, expected);
        assert_eq!(counts(status_counts), (2, 2, 1, 0));

        let (responses, status_counts) = pipeline.process_batch([MALFORMED_LINE]);
        assert!(matches!(responses[..], [Response::Malformed(_)]));
        assert_eq!(counts(status_counts), (0, 1, 0, 0));
        assert_eq!(
            pipeline.process_batch([]),
            (Vec::new(), StatusCounts::default())
        );
    }

    #[test]
    fn explain_responds_as_process_and_skips_malformed_records() {
        let pipeline = access_log();

        for line in batch() {
            let (response, explain) = pipeline.explain(line);
            assert_eq!(response, pipeline.process(line));
            assert_eq!(explain.is_none(), line == MALFORMED_LINE);
        }

        let (_, explain) = pipeline.explain(OK_LINES[0]);
        let explain = explain.expect("record is parsed");
        assert_eq!((explain.schema(), explain.version()), ("access", 1));
        let client = explain.field("client").expect("client is recorded");
        assert_eq!(client.reads()[0].text(), Some("198.51.100.2, 203.0.113.7"));
    }

    #[test]
    fn needs_parser_mapper_and_resp_sections() {
        let (without_resp, _) = ACCESS_LOG_YAML.split_once("\nresp:").expect("resp section");
        assert!(Pipeline::from_specs(&specs(without_resp)).is_none());

        let parser_only =
            "parser: {format: kv, pair_delimiter: ' ', key_value_delimiter: '=', fields: {id: id}}";
        assert!(Pipeline::from_specs(&specs(parser_only)).is_none());
    }

    #[test]
    fn shares_one_pipeline_across_threads() {
        let pipeline = Arc::new(access_log());
        let expected = pipeline.process_batch(batch());

        thread::scope(|scope| {
            let workers: Vec<_> = (0..8)
                .map(|_| {
                    let pipeline = Arc::clone(&pipeline);
                    scope.spawn(move || {
                        let (responses, counts) = pipeline.process_batch(batch());
                        (
                            responses
                                .into_iter()
                                .map(Response::into_owned)
                                .collect::<Vec<_>>(),
                            counts,
                        )
                    })
                })
                .collect();
            for worker in workers {
                assert_eq!(worker.join().expect("worker finishes"), expected);
            }
        });
    }

    fn respond_all(specs: &Specs) -> (String, Vec<Response<'static>>, Option<Lineage>) {
        let pipeline = Pipeline::from_specs(specs).expect("all three sections");
        let lines = [OK_LINES[0], OK_LINES[1], BAD_TIME_LINE];
        let responses = lines
            .iter()
            .map(|line| pipeline.process(line).into_owned())
            .collect();
        (
            specs.digest().to_owned(),
            responses,
            specs.lineage().cloned(),
        )
    }

    #[test]
    fn shares_specs_across_threads_and_replaces_them_by_arc() {
        let first = Arc::new(specs(ACCESS_LOG_YAML));
        let second = Arc::new(specs(
            &ACCESS_LOG_YAML.replace("constant: web-01", "constant: web-02"),
        ));
        assert_ne!(first.digest(), second.digest());
        let expected = [respond_all(&first), respond_all(&second)];
        assert_ne!(expected[0].1, expected[1].1);
        assert_ne!(expected[0].2, expected[1].2);

        let current = RwLock::new(Arc::clone(&first));
        thread::scope(|scope| {
            let workers: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        let specs = Arc::clone(&current.read().expect("lock is not poisoned"));
                        respond_all(&specs)
                    })
                })
                .collect();
            let held = Arc::clone(&current.read().expect("lock is not poisoned"));
            *current.write().expect("lock is not poisoned") = Arc::clone(&second);
            assert_eq!(respond_all(&held), expected[0]);
            for worker in workers {
                assert!(expected.contains(&worker.join().expect("worker finishes")));
            }
        });
        let latest = Arc::clone(&current.read().expect("lock is not poisoned"));
        assert_eq!(respond_all(&latest), expected[1]);
    }
}

mod delimited {
    use moldette::{Pipeline, Response, Value};

    use crate::{
        ORDER_BODIES, ORDER_EXPORT_CSV, ORDER_EXPORT_YAML, assert_responses, counts, malformed,
        missing, ok, specs, text, timestamp,
    };

    fn order_export(yaml: &str) -> Pipeline {
        Pipeline::from_specs(&specs(yaml)).expect("all three sections")
    }

    const HEADER: &str = "id,placed_at,customer,product,quantity,unit_price,gift,note\n";

    fn rows() -> &'static str {
        ORDER_EXPORT_CSV
            .strip_prefix(HEADER)
            .expect("header row names the columns in order")
    }

    #[test]
    fn processes_csv_rows_after_the_header_row() {
        let pipeline = order_export(ORDER_EXPORT_YAML);
        assert_eq!(
            pipeline.process(HEADER),
            missing(&["placed_at", "quantity"])
        );

        let (responses, status_counts) = pipeline.process_batch(rows().lines());

        assert_responses(
            &responses,
            &[
                ok(
                    [
                        ("id", text("A-2001")),
                        ("placed_at", timestamp(1_794_994_200, 0, 0)),
                        ("customer", text("C-100")),
                        ("product", text("Notebook")),
                        ("quantity", Value::Int(3)),
                        ("unit_price", Value::Float(4.5)),
                        ("gift", Value::Bool(false)),
                    ],
                    &[],
                ),
                ok(
                    [
                        ("id", text("A-2002")),
                        ("placed_at", timestamp(1_794_994_275, 0, 0)),
                        ("customer", text("C-101")),
                        ("product", text("Desk lamp, brass")),
                        ("quantity", Value::Int(1)),
                        ("unit_price", Value::Float(39.9)),
                        ("gift", Value::Bool(true)),
                        ("note", text(r#"Leave at the "side" door"#)),
                    ],
                    &[],
                ),
                ok(
                    [
                        ("id", text("A-2003")),
                        ("placed_at", timestamp(1_794_994_320, 0, 0)),
                        ("product", text("Pencil")),
                        ("quantity", Value::Int(12)),
                        ("gift", Value::Bool(false)),
                    ],
                    &[],
                ),
                ok(
                    [
                        ("id", text("A-2004")),
                        ("placed_at", timestamp(1_794_994_380, 0, 0)),
                        ("customer", text("C-102")),
                        ("product", text("Marker")),
                        ("quantity", Value::Int(2)),
                    ],
                    &["unit_price", "gift"],
                ),
                missing(&["quantity"]),
                missing(&["id"]),
                missing(&["placed_at"]),
                missing(&["quantity"]),
                ok(
                    [
                        ("id", text("A-2009")),
                        ("placed_at", timestamp(1_794_994_680, 0, 0)),
                        ("customer", text("C-107")),
                        ("product", text("Folder")),
                        ("quantity", Value::Int(5)),
                        ("unit_price", Value::Float(0.95)),
                        ("gift", Value::Bool(true)),
                    ],
                    &[],
                ),
                malformed("quoted field is not closed"),
                malformed("text follows a closing quote"),
            ],
        );
        assert_eq!(counts(status_counts), (5, 2, 4, 0));
    }

    #[test]
    fn strict_resp_fails_csv_rows_with_emptied_fields() {
        let pipeline =
            order_export(&ORDER_EXPORT_YAML.replace("resp:\n", "resp:\n  strict: true\n"));

        let (responses, status_counts) = pipeline.process_batch(rows().lines());

        assert_eq!(
            responses[3],
            Response::Emptied(vec!["unit_price".into(), "gift".into()])
        );
        assert_eq!(counts(status_counts), (4, 2, 4, 1));
    }

    #[test]
    fn maps_coalesce_candidates_read_from_nested_bodies() {
        let specs = specs(ORDER_BODIES);
        let record = specs
            .parser()
            .expect("parser spec")
            .parse(r#"A-1|web|{"sku":"S-9","items":"B-7;C-3"}"#)
            .expect("record is parsed");
        let mapped = specs.mapper().expect("mapper spec").map(&record);
        assert_eq!(mapped.get("item"), Some(&Value::Str("B-7".into())));
    }
}

mod json {
    use moldette::{Pipeline, Value};

    use crate::{
        ORDER_API_NDJSON, ORDER_API_YAML, assert_responses, counts, malformed, missing, ok, specs,
        text, timestamp,
    };

    #[test]
    fn processes_api_responses_one_record_per_line() {
        let pipeline = Pipeline::from_specs(&specs(ORDER_API_YAML)).expect("all three sections");

        let (responses, status_counts) = pipeline.process_batch(ORDER_API_NDJSON.lines());

        assert_responses(
            &responses,
            &[
                ok(
                    [
                        ("id", text("A-1001")),
                        ("status", text("paid")),
                        ("customer", text("C-100")),
                        ("city", text("Springfield")),
                        ("first_sku", text("S-1")),
                        ("first_quantity", Value::Int(2)),
                        ("total", Value::Float(25.5)),
                        ("paid", Value::Bool(true)),
                        ("created_at", timestamp(1_794_994_200, 0, 0)),
                        ("coupon", text("WELCOME")),
                    ],
                    &[],
                ),
                ok(
                    [
                        ("id", text("A-1002")),
                        ("status", text("on hold")),
                        ("customer", text("C-101")),
                        ("first_sku", text("S-3")),
                        ("first_quantity", Value::Int(1)),
                        ("total", Value::Float(40.0)),
                        ("paid", Value::Bool(false)),
                        ("created_at", timestamp(1_794_995_110, 0, 32_400)),
                    ],
                    &[],
                ),
                ok(
                    [
                        ("id", text("A-1003")),
                        ("status", text("pending")),
                        ("customer", text("C-102")),
                        ("total", Value::Float(12.5)),
                        ("paid", Value::Bool(false)),
                        ("created_at", timestamp(1_794_996_000, 250_000_000, 0)),
                        ("coupon", text("")),
                    ],
                    &[],
                ),
                ok(
                    [
                        ("id", text("A-1004")),
                        ("status", text("paid")),
                        ("customer", text("C-103")),
                        ("first_sku", text("S-4")),
                        ("total", Value::Float(9.99)),
                        ("created_at", timestamp(1_794_999_600, 0, 0)),
                    ],
                    &["first_quantity", "paid"],
                ),
                missing(&["total"]),
                missing(&["id", "created_at"]),
                malformed("EOF while parsing"),
                malformed("expected a JSON object"),
                malformed("trailing characters"),
            ],
        );
        assert_eq!(counts(status_counts), (4, 3, 2, 0));
    }
}

mod kv {
    use moldette::mapper::MapperSpec;
    use moldette::parser::{Format, ParserSpec};
    use moldette::resp::RespSpec;
    use moldette::{Field, OutputSchema, Pipeline, Response, Type, Value};

    #[test]
    fn runs_layer_specs_built_without_configuration() {
        let schema = OutputSchema::builder("order", 1)
            .field(Field::new("id", Type::String).required())
            .field(Field::new("amount", Type::Float))
            .build()
            .expect("valid schema");
        let parser = ParserSpec::builder(Format::Kv {
            pair_delimiter: ' ',
            key_value_delimiter: '=',
        })
        .field("id", "id")
        .field("amount", "amount")
        .build()
        .expect("valid spec");
        let mapper = MapperSpec::builder(schema.clone())
            .build()
            .expect("valid spec");
        let pipeline = Pipeline::new(parser, mapper, RespSpec::builder(schema).build());

        let Response::Ok(mapped) = pipeline.process("id=A-1 amount=12.5") else {
            panic!("record is ok");
        };
        assert_eq!(mapped.get("amount"), Some(&Value::Float(12.5)));
        assert_eq!(
            pipeline.process("amount=3"),
            Response::MissingRequired(vec!["id".into()])
        );
        assert!(matches!(pipeline.process("=3"), Response::Malformed(_)));
    }
}
