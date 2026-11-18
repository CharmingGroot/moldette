#![cfg(feature = "config")]

use std::sync::Arc;
use std::thread;

use moldette::config::{Config, Registry, Specs};
use moldette::mapper::MapperSpec;
use moldette::parser::{Format, ParserSpec};
use moldette::resp::RespSpec;
use moldette::{Field, OutputSchema, Pipeline, Response, StatusCounts, Timestamp, Type, Value};

const ACCESS_LOG: &str = include_str!("../../moldette-config/tests/fixtures/nginx_access.yaml");

const OK_LINES: [&str; 2] = [
    r#"203.0.113.7 - - [18/Nov/2026:10:15:32 +0000] "GET /orders?id=A-1 HTTP/1.1" 200 512 "-" "curl/8.5.0" "198.51.100.2, 203.0.113.7""#,
    r#"203.0.113.9 - - [18/Nov/2026:10:15:33 +0900] "POST /orders HTTP/2.0" 404 0 "https://shop.example/cart" "Mozilla/5.0" "-""#,
];
const BAD_TIME_LINE: &str =
    r#"203.0.113.9 - - [yesterday] "GET / HTTP/1.1" 200 12 "-" "curl/8.5.0" "-""#;
const MALFORMED_LINE: &str = "GET /orders 200";

fn specs(yaml: &str) -> Specs {
    Config::from_yaml(yaml)
        .expect("valid YAML")
        .build(&Registry::new())
        .expect("specs are built")
}

fn access_log() -> Pipeline {
    Pipeline::from_specs(&specs(ACCESS_LOG)).expect("all three sections")
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

fn counts(counts: StatusCounts) -> (u64, u64, u64, u64) {
    (
        counts.ok(),
        counts.malformed(),
        counts.missing_required(),
        counts.emptied(),
    )
}

#[test]
fn processes_access_log_records_into_responses() {
    let pipeline = access_log();

    let Response::Ok(mapped) = pipeline.process(OK_LINES[0]) else {
        panic!("record is ok");
    };
    assert_eq!(
        mapped.get("client"),
        Some(&Value::Ip("198.51.100.2".parse().expect("ip")))
    );
    assert_eq!(
        mapped.get("time"),
        Some(&Value::Timestamp(
            Timestamp::new(1_794_996_932, 0, 0).expect("timestamp")
        ))
    );
    assert_eq!(
        mapped.get("status_class"),
        Some(&Value::Str("success".into()))
    );

    assert_eq!(
        pipeline.process(BAD_TIME_LINE),
        Response::MissingRequired(vec!["time".into()])
    );
    let Response::Malformed(malformed) = pipeline.process(MALFORMED_LINE) else {
        panic!("record is malformed");
    };
    assert!(!malformed.reason().is_empty());
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
    let (without_resp, _) = ACCESS_LOG.split_once("\nresp:").expect("resp section");
    assert!(Pipeline::from_specs(&specs(without_resp)).is_none());

    let parser_only =
        "parser: {format: kv, pair_delimiter: ' ', key_value_delimiter: '=', fields: {id: id}}";
    assert!(Pipeline::from_specs(&specs(parser_only)).is_none());
}

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
