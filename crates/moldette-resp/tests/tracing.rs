#![cfg(feature = "tracing")]

use std::fmt;
use std::sync::{Arc, Mutex};

use moldette_core::{Field, Mapped, OutputSchema, Type, Value};
use moldette_resp::RespSpec;
use tracing::field::{Field as SignalField, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Level, Metadata, Subscriber};

#[derive(Debug, Clone, PartialEq)]
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

fn order_spec() -> RespSpec {
    let schema = OutputSchema::builder("order", 2)
        .field(Field::new("order_id", Type::String).required())
        .field(Field::new("status", Type::String))
        .build()
        .expect("valid schema");
    RespSpec::builder(schema).strict().build()
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
    let spec = order_spec();
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

    let input = signal(
        "input",
        Level::INFO,
        Some("moldette.resp.respond_batch"),
        &[("message", "read")],
    );
    assert_eq!(
        signals,
        [
            signal(
                "moldette.resp.respond_batch",
                Level::INFO,
                None,
                &[
                    ("moldette.schema.name", "order"),
                    ("moldette.schema.version", "2"),
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
fn single_responses_emit_no_signals() {
    let collector = Collector::default();
    let spec = order_spec();

    let signals = collector.collect(|| {
        spec.respond(order(Some("A-1"), true));
    });

    assert!(signals.is_empty());
}
