#![cfg(feature = "tracing")]

use std::fmt;
use std::sync::{Arc, Mutex};

use moldette_core::{Field, FieldRecord, OutputSchema, SpecError, Type};
use moldette_mapper::{MapperSpec, Rule};
use tracing::field::{Field as SignalField, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Level, Metadata, Subscriber};

#[derive(Debug, PartialEq)]
struct Signal {
    name: &'static str,
    level: Level,
    parent: Option<&'static str>,
    fields: Vec<(&'static str, String)>,
}

#[derive(Clone, Default)]
struct Collector {
    signals: Arc<Mutex<Vec<Signal>>>,
    entered: Arc<Mutex<Vec<usize>>>,
}

struct Fields<'a>(&'a mut Vec<(&'static str, String)>);

impl Visit for Fields<'_> {
    fn record_str(&mut self, field: &SignalField, value: &str) {
        self.0.push((field.name(), value.to_owned()));
    }

    fn record_debug(&mut self, field: &SignalField, value: &dyn fmt::Debug) {
        self.0.push((field.name(), format!("{value:?}")));
    }
}

impl Collector {
    fn push(
        &self,
        metadata: &'static Metadata<'static>,
        record: impl FnOnce(&mut Fields),
    ) -> usize {
        let mut signals = self.signals.lock().expect("collector lock");
        let parent = self
            .entered
            .lock()
            .expect("collector lock")
            .last()
            .map(|&i| signals[i].name);
        let mut fields = Vec::new();
        record(&mut Fields(&mut fields));
        signals.push(Signal {
            name: metadata.name(),
            level: *metadata.level(),
            parent,
            fields,
        });
        signals.len() - 1
    }

    fn collect(&self, run: impl FnOnce()) -> Vec<Signal> {
        tracing::subscriber::with_default(self.clone(), run);
        std::mem::take(&mut *self.signals.lock().expect("collector lock"))
    }
}

impl Subscriber for Collector {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, attributes: &Attributes<'_>) -> Id {
        let index = self.push(attributes.metadata(), |fields| attributes.record(fields));
        Id::from_u64(index as u64 + 1)
    }

    fn record(&self, span: &Id, values: &Record<'_>) {
        let mut signals = self.signals.lock().expect("collector lock");
        values.record(&mut Fields(
            &mut signals[span.into_u64() as usize - 1].fields,
        ));
    }

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        self.push(event.metadata(), |fields| event.record(fields));
    }

    fn enter(&self, span: &Id) {
        let index = span.into_u64() as usize - 1;
        self.entered.lock().expect("collector lock").push(index);
    }

    fn exit(&self, _span: &Id) {
        self.entered.lock().expect("collector lock").pop();
    }
}

fn signal(
    name: &'static str,
    level: Level,
    parent: Option<&'static str>,
    fields: &[(&'static str, &str)],
) -> Signal {
    Signal {
        name,
        level,
        parent,
        fields: fields
            .iter()
            .map(|&(name, value)| (name, value.to_owned()))
            .collect(),
    }
}

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
            signal(
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
            signal(
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
            signal(
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
