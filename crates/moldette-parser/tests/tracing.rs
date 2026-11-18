#![cfg(feature = "tracing")]

use std::borrow::Cow;

use moldette_core::{Malformed, SpecError};
use moldette_parser::{Format, Parser, ParserSpec};
use moldette_test_tracing::{Collector, Signal};
use tracing::Level;

const APP_HEADER: &str = r"^(?P<time>\S+) (?P<app>[^\[]+)\[(?P<pid>\d+)\]:(?: (?P<body>.*))?$";

#[derive(Debug)]
struct FirstWord;

impl Parser for FirstWord {
    type Path = ();

    fn name(&self) -> &str {
        "first_word"
    }

    fn path(&self, _: &str) -> Option<()> {
        Some(())
    }

    fn read<'a>(
        &self,
        raw: &'a str,
        _: &[()],
        values: &mut [Option<Cow<'a, str>>],
    ) -> Result<(), Malformed> {
        let (word, _) = raw
            .split_once(' ')
            .ok_or_else(|| Malformed::new(format!("no space in `{raw}`")))?;
        values.fill(Some(Cow::Borrowed(word)));
        Ok(())
    }
}

fn header_with_json_body() -> ParserSpec {
    let body = ParserSpec::builder(Format::Json)
        .field("order_id", "/order_id")
        .field("amount", "/amount")
        .build()
        .expect("valid spec");
    ParserSpec::builder(Format::Regex {
        pattern: APP_HEADER.into(),
    })
    .field("time", "time")
    .field("app", "app")
    .body("body", body)
    .build()
    .expect("valid spec")
}

fn body_span() -> Signal {
    Signal::new(
        "moldette.parser.body",
        Level::TRACE,
        Some("moldette.parser.parse"),
        &[
            ("moldette.parser.path", "body"),
            ("moldette.parser.format", "json"),
        ],
    )
}

fn parse_span(format: &str) -> Signal {
    Signal::new(
        "moldette.parser.parse",
        Level::TRACE,
        None,
        &[("moldette.parser.format", format)],
    )
}

fn malformed(parent: &'static str, format: &str) -> Signal {
    Signal::new(
        "moldette.parser.malformed",
        Level::TRACE,
        Some(parent),
        &[("moldette.parser.format", format)],
    )
}

#[test]
fn build_and_parse_nest_the_body_span_without_record_values() {
    let collector = Collector::default();

    let signals = collector.collect(|| {
        let spec = header_with_json_body();
        spec.parse(r#"2026-11-18T10:00:00Z app[42]: {"order_id":"A-1","amount":12.5}"#)
            .expect("valid record");
    });

    assert_eq!(
        signals,
        [
            Signal::new(
                "moldette.parser.build",
                Level::INFO,
                None,
                &[
                    ("moldette.parser.format", "json"),
                    ("moldette.parser.fields", "2"),
                    ("moldette.parser.bodies", "0"),
                ],
            ),
            Signal::new(
                "moldette.parser.build",
                Level::INFO,
                None,
                &[
                    ("moldette.parser.format", "regex"),
                    ("moldette.parser.fields", "2"),
                    ("moldette.parser.bodies", "1"),
                ],
            ),
            parse_span("regex"),
            body_span(),
        ]
    );
}

#[test]
fn malformed_event_is_emitted_once_in_the_span_of_the_failing_format() {
    let collector = Collector::default();
    let spec = header_with_json_body();

    let signals = collector.collect(|| {
        spec.parse(r#"app: {"order_id":"A-1"}"#)
            .expect_err("malformed header");
        spec.parse(r#"2026-11-18T10:00:00Z app[42]: {"order_id":"#)
            .expect_err("malformed body");
        spec.parse("2026-11-18T10:00:00Z app[42]:")
            .expect("record without a body");
    });

    assert_eq!(
        signals,
        [
            parse_span("regex"),
            malformed("moldette.parser.parse", "regex"),
            parse_span("regex"),
            body_span(),
            malformed("moldette.parser.body", "json"),
            parse_span("regex"),
        ]
    );
}

#[test]
fn custom_parser_name_labels_signals_and_malformed_reasons_are_left_out() {
    let collector = Collector::default();

    let signals = collector.collect(|| {
        let spec = ParserSpec::builder(Format::custom(FirstWord))
            .field("order_id", "first")
            .build()
            .expect("valid spec");
        spec.parse("A-1").expect_err("malformed record");
    });

    assert_eq!(
        signals,
        [
            Signal::new(
                "moldette.parser.build",
                Level::INFO,
                None,
                &[
                    ("moldette.parser.format", "first_word"),
                    ("moldette.parser.fields", "1"),
                    ("moldette.parser.bodies", "0"),
                ],
            ),
            parse_span("first_word"),
            malformed("moldette.parser.parse", "first_word"),
        ]
    );
}

#[test]
fn failed_build_emits_spec_error_inside_the_build_span() {
    let collector = Collector::default();

    let signals = collector.collect(|| {
        let result = ParserSpec::builder(Format::Delimited { delimiter: ',' })
            .field("order_id", "0")
            .field("order_id", "1")
            .build();
        assert_eq!(
            result.map(|_| ()),
            Err(SpecError::DuplicateField("order_id".into()))
        );
    });

    assert_eq!(
        signals,
        [
            Signal::new(
                "moldette.parser.build",
                Level::INFO,
                None,
                &[
                    ("moldette.parser.format", "delimited"),
                    ("moldette.parser.fields", "2"),
                    ("moldette.parser.bodies", "0"),
                ],
            ),
            Signal::new(
                "moldette.parser.spec_error",
                Level::DEBUG,
                Some("moldette.parser.build"),
                &[(
                    "moldette.error",
                    "field `order_id` is declared more than once"
                )],
            ),
        ]
    );
}
