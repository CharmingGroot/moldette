#![cfg(feature = "tracing")]

use moldette_core::{Field, Mapped, OutputSchema, Type, Value};
use moldette_resp::{RespSpec, RespSpecBuilder};
use moldette_test_tracing::{Collector, Signal};
use tracing::Level;

fn order_builder() -> RespSpecBuilder {
    let schema = OutputSchema::builder("order", 2)
        .field(Field::new("order_id", Type::String).required())
        .field(Field::new("status", Type::String))
        .build()
        .expect("valid schema");
    RespSpec::builder(schema)
}

fn order(id: Option<&'static str>, emptied_status: bool) -> Mapped<'static> {
    let mut mapped = Mapped::new();
    if let Some(id) = id {
        mapped.insert("order_id", Value::Str(id.into()));
    }
    if emptied_status {
        mapped.mark_emptied("status");
    }
    mapped
}

#[test]
fn batch_span_covers_reading_the_input_and_counts_each_status() {
    let collector = Collector::default();
    let spec = order_builder().strict().build();
    let records = [
        order(Some("A-1"), false),
        order(Some("A-2"), true),
        order(None, false),
        order(Some("A-4"), false),
    ];

    let signals = collector.collect(|| {
        spec.respond_batch(records.into_iter().inspect(|_| {
            tracing::info!(name: "input", "read");
        }));
    });

    let input = Signal::new(
        "input",
        Level::INFO,
        Some("moldette.resp.respond_batch"),
        &[("message", "read")],
    );
    assert_eq!(
        signals,
        [
            Signal::new(
                "moldette.resp.respond_batch",
                Level::INFO,
                None,
                &[
                    ("moldette.schema.name", "order"),
                    ("moldette.schema.version", "2"),
                    ("moldette.resp.strict", "true"),
                    ("moldette.resp.ok", "2"),
                    ("moldette.resp.missing_required", "1"),
                    ("moldette.resp.emptied", "1"),
                ],
            ),
            input.clone(),
            input.clone(),
            input.clone(),
            input,
        ]
    );
}

#[test]
fn batch_span_reports_strict_mode_off_and_counts_emptied_records_as_ok() {
    let collector = Collector::default();
    let spec = order_builder().build();

    let signals = collector.collect(|| {
        spec.respond_batch([order(Some("A-1"), false), order(Some("A-2"), true)]);
    });

    assert_eq!(
        signals,
        [Signal::new(
            "moldette.resp.respond_batch",
            Level::INFO,
            None,
            &[
                ("moldette.schema.name", "order"),
                ("moldette.schema.version", "2"),
                ("moldette.resp.strict", "false"),
                ("moldette.resp.ok", "2"),
                ("moldette.resp.missing_required", "0"),
                ("moldette.resp.emptied", "0"),
            ],
        )]
    );
}

#[test]
fn single_responses_emit_no_signals() {
    let collector = Collector::default();
    let spec = order_builder().strict().build();

    let signals = collector.collect(|| {
        spec.respond(order(Some("A-1"), true));
    });

    assert!(signals.is_empty());
}
