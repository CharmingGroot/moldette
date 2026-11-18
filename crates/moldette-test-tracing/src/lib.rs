//! A `tracing` subscriber for tests that records each span and event with its
//! name, level, entered parent span and fields.

use std::fmt;
use std::sync::{Arc, Mutex};

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Level, Metadata, Subscriber};

/// A span or event as recorded by a [`Collector`].
#[derive(Debug, Clone, PartialEq)]
pub struct Signal {
    name: &'static str,
    level: Level,
    parent: Option<&'static str>,
    fields: Vec<(&'static str, String)>,
}

impl Signal {
    pub fn new(
        name: &'static str,
        level: Level,
        parent: Option<&'static str>,
        fields: &[(&'static str, &str)],
    ) -> Self {
        Self {
            name,
            level,
            parent,
            fields: fields
                .iter()
                .map(|&(name, value)| (name, value.to_owned()))
                .collect(),
        }
    }
}

/// Records every signal emitted while [`Collector::collect`] runs.
#[derive(Clone, Default)]
pub struct Collector {
    signals: Arc<Mutex<Vec<Signal>>>,
    entered: Arc<Mutex<Vec<usize>>>,
}

struct Fields<'a>(&'a mut Vec<(&'static str, String)>);

impl Visit for Fields<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.push((field.name(), value.to_owned()));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.0.push((field.name(), format!("{value:?}")));
    }
}

impl Collector {
    /// Runs `run` with this collector as the default subscriber and returns
    /// the signals in the order they were emitted.
    pub fn collect(&self, run: impl FnOnce()) -> Vec<Signal> {
        tracing::subscriber::with_default(self.clone(), run);
        std::mem::take(&mut *self.signals.lock().expect("collector lock"))
    }

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
