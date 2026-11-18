#![cfg(feature = "config")]

use moldette::config::{Config, Registry, Specs};

const ACCESS_LOG_YAML: &str =
    include_str!("../../moldette-config/tests/fixtures/nginx_access.yaml");
const ACCESS_LOG_JSON: &str =
    include_str!("../../moldette-config/tests/fixtures/nginx_access.json");
const ORDER_BODIES: &str = include_str!("../../moldette-config/tests/fixtures/order_bodies.yaml");

fn specs(yaml: &str) -> Specs {
    Config::from_yaml(yaml)
        .expect("valid YAML")
        .build(&Registry::new())
        .expect("specs are built")
}

mod regex {
    use std::borrow::Cow;
    use std::sync::{Arc, RwLock};
    use std::thread;

    use moldette::config::{Config, Lineage, Registry, Specs};
    use moldette::{Pipeline, Response, StatusCounts, Timestamp, Value};

    use crate::{ACCESS_LOG_JSON, ACCESS_LOG_YAML, specs};

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

    fn counts(counts: StatusCounts) -> (u64, u64, u64, u64) {
        (
            counts.ok(),
            counts.malformed(),
            counts.missing_required(),
            counts.emptied(),
        )
    }

    fn text(value: &str) -> Value<'static> {
        Value::Str(Cow::Owned(value.to_owned()))
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
    use moldette::Value;

    use crate::{ORDER_BODIES, specs};

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
    use moldette::{Pipeline, Response, Value};

    use crate::specs;

    const ORDERS: &str = r#"schemas:
  - name: order
    version: 1
    fields:
      - {name: id, type: string, required: true}
      - {name: amount, type: float}
      - {name: paid, type: bool}
parser:
  format: json
  fields: {id: /order/id, amount: /order/amount, paid: /order/paid}
mapper:
  schema: {name: order, version: 1}
resp:
  schema: {name: order, version: 1}
"#;

    #[test]
    fn processes_order_records() {
        let pipeline = Pipeline::from_specs(&specs(ORDERS)).expect("all three sections");

        let Response::Ok(mapped) =
            pipeline.process(r#"{"order": {"id": "A-1", "amount": 12.50, "paid": true}}"#)
        else {
            panic!("record is ok");
        };
        assert_eq!(mapped.get("id"), Some(&Value::Str("A-1".into())));
        assert_eq!(mapped.get("amount"), Some(&Value::Float(12.5)));
        assert_eq!(mapped.get("paid"), Some(&Value::Bool(true)));
        assert_eq!(
            pipeline.process(r#"{"order": {"amount": 3}}"#),
            Response::MissingRequired(vec!["id".into()])
        );
        assert!(matches!(
            pipeline.process(r#"[{"order": {}}]"#),
            Response::Malformed(_)
        ));
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
