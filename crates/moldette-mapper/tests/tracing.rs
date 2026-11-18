#![cfg(feature = "tracing")]

use moldette_core::{Field, FieldRecord, OutputSchema, SpecError, Type};
use moldette_mapper::{MapperSpec, Rule};
use moldette_test_tracing::{Collector, Signal};
use tracing::Level;

fn order_schema() -> OutputSchema {
    OutputSchema::builder("order", 3)
        .field(Field::new("order_id", Type::String))
        .field(Field::new("quantity", Type::Int))
        .build()
        .expect("valid schema")
}

#[test]
fn build_and_map_emit_spans_and_emptied_events_without_record_values() {
    let collector = Collector::default();
    let schema_fields = [
        ("moldette.schema.name", "order"),
        ("moldette.schema.version", "3"),
    ];
    let mut record = FieldRecord::new();
    record.insert("id", "A-1");
    record.insert("qty", "three");

    let signals = collector.collect(|| {
        let spec = MapperSpec::builder(order_schema())
            .rule(Rule::new("order_id", "id"))
            .rule(Rule::new("quantity", "qty").trim())
            .build()
            .expect("valid rules");
        spec.map(&record);
    });

    assert_eq!(
        signals,
        [
            Signal::new(
                "moldette.mapper.build",
                Level::INFO,
                None,
                &[
                    schema_fields[0],
                    schema_fields[1],
                    ("moldette.mapper.rules", "2"),
                    ("moldette.mapper.explicit_rules", "false"),
                ],
            ),
            signal("moldette.mapper.map", Level::TRACE, None, &schema_fields),
            signal(
                "moldette.mapper.emptied",
                Level::TRACE,
                Some("moldette.mapper.map"),
                &[("moldette.field", "quantity")],
            ),
        ]
    );
}

#[test]
fn explicit_rules_are_reported_at_build_and_fields_without_a_rule_emit_no_event() {
    let collector = Collector::default();
    let mut record = FieldRecord::new();
    record.insert("order_id", "A-1");
    record.insert("qty", "three");

    let signals = collector.collect(|| {
        let spec = MapperSpec::builder(order_schema())
            .rule(Rule::new("quantity", "qty"))
            .explicit_rules()
            .build()
            .expect("valid rule");
        spec.map(&record);
    });

    let schema_fields = [
        ("moldette.schema.name", "order"),
        ("moldette.schema.version", "3"),
    ];
    assert_eq!(
        signals,
        [
            signal(
                "moldette.mapper.build",
                Level::INFO,
                None,
                &[
                    schema_fields[0],
                    schema_fields[1],
                    ("moldette.mapper.rules", "1"),
                    ("moldette.mapper.explicit_rules", "true"),
                ],
            ),
            Signal::new("moldette.mapper.map", Level::TRACE, None, &schema_fields),
            Signal::new(
                "moldette.mapper.emptied",
                Level::TRACE,
                Some("moldette.mapper.map"),
                &[("moldette.field", "quantity")],
            ),
        ]
    );
}

#[test]
fn failed_build_emits_spec_error_inside_the_build_span() {
    let collector = Collector::default();

    let signals = collector.collect(|| {
        let result = MapperSpec::builder(order_schema())
            .rule(Rule::new("total", "amount"))
            .build();
        assert_eq!(result, Err(SpecError::UnknownField("total".into())));
    });

    assert_eq!(
        signals,
        [
            Signal::new(
                "moldette.mapper.build",
                Level::INFO,
                None,
                &[
                    ("moldette.schema.name", "order"),
                    ("moldette.schema.version", "3"),
                    ("moldette.mapper.rules", "1"),
                    ("moldette.mapper.explicit_rules", "false"),
                ],
            ),
            Signal::new(
                "moldette.mapper.spec_error",
                Level::DEBUG,
                Some("moldette.mapper.build"),
                &[(
                    "moldette.error",
                    "field `total` is not in the output schema"
                )],
            ),
        ]
    );
}
