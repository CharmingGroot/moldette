#![cfg(all(feature = "tracing", feature = "config"))]

use moldette::Pipeline;
use moldette::config::{Config, Registry};
use moldette_test_tracing::{Collector, Signal};
use tracing::Level;

const ACCESS_LOG: &str = include_str!("../../moldette-config/tests/fixtures/nginx_access.yaml");
const OK_LINE: &str = r#"203.0.113.7 - - [18/Nov/2026:10:15:32 +0000] "GET /orders?id=A-1 HTTP/1.1" 200 512 "-" "curl/8.5.0" "198.51.100.2, 203.0.113.7""#;

#[test]
fn batch_span_holds_the_parser_and_mapper_signals_of_each_record() {
    let specs = Config::from_yaml(ACCESS_LOG)
        .expect("valid YAML")
        .build(&Registry::new())
        .expect("specs are built");
    let pipeline = Pipeline::from_specs(&specs).expect("all three sections");
    let collector = Collector::default();

    let signals = collector.collect(|| {
        pipeline.process_batch([OK_LINE, "GET /orders 200"]);
    });

    let parse = Signal::new(
        "moldette.parser.parse",
        Level::TRACE,
        Some("moldette.resp.respond_batch"),
        &[("moldette.parser.format", "regex")],
    );
    assert_eq!(
        signals,
        [
            Signal::new(
                "moldette.resp.respond_batch",
                Level::INFO,
                None,
                &[
                    ("moldette.schema.name", "access"),
                    ("moldette.schema.version", "1"),
                    ("moldette.resp.strict", "false"),
                    ("moldette.resp.ok", "1"),
                    ("moldette.resp.missing_required", "0"),
                    ("moldette.resp.emptied", "0"),
                ],
            ),
            parse.clone(),
            Signal::new(
                "moldette.mapper.map",
                Level::TRACE,
                Some("moldette.resp.respond_batch"),
                &[
                    ("moldette.schema.name", "access"),
                    ("moldette.schema.version", "1"),
                ],
            ),
            parse,
            Signal::new(
                "moldette.parser.malformed",
                Level::TRACE,
                Some("moldette.parser.parse"),
                &[("moldette.parser.format", "regex")],
            ),
        ]
    );
}
