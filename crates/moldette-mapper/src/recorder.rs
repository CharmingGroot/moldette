use std::borrow::Cow;

use moldette_core::{
    Explain, ExplainField, ExplainRead, ExplainSource, ExplainStep, OutputSchema, Value,
};

use crate::op::CompiledOp;

// One execution path for map and explain; see #39.
pub(crate) trait Recorder<'a> {
    fn field(&mut self, name: &'a str);

    fn read(&mut self, field: &'a str, text: Option<&Cow<'a, str>>);

    fn constant(&mut self, value: &'a str);

    fn step(&mut self, op: &'a CompiledOp, output: Option<&Cow<'a, str>>);

    fn coerced(&mut self, value: Option<&Value<'a>>);
}

pub(crate) struct Discard;

impl<'a> Recorder<'a> for Discard {
    fn field(&mut self, _name: &'a str) {}

    fn read(&mut self, _field: &'a str, _text: Option<&Cow<'a, str>>) {}

    fn constant(&mut self, _value: &'a str) {}

    fn step(&mut self, _op: &'a CompiledOp, _output: Option<&Cow<'a, str>>) {}

    fn coerced(&mut self, _value: Option<&Value<'a>>) {}
}

pub(crate) struct ExplainRecorder<'a> {
    explain: Explain<'a>,
    field: Option<ExplainField<'a>>,
    read: Option<ExplainRead<'a>>,
}

impl<'a> ExplainRecorder<'a> {
    pub(crate) fn new(schema: &'a OutputSchema) -> Self {
        Self {
            explain: Explain::new(schema.name(), schema.version()),
            field: None,
            read: None,
        }
    }

    pub(crate) fn finish(mut self) -> Explain<'a> {
        self.close_field();
        self.explain
    }

    fn open_read(&mut self, read: ExplainRead<'a>) {
        self.close_read();
        self.read = Some(read);
    }

    fn close_read(&mut self) {
        if let (Some(field), Some(read)) = (&mut self.field, self.read.take()) {
            field.push(read);
        }
    }

    fn close_field(&mut self) {
        self.close_read();
        if let Some(field) = self.field.take() {
            self.explain.push(field);
        }
    }
}

impl<'a> Recorder<'a> for ExplainRecorder<'a> {
    fn field(&mut self, name: &'a str) {
        self.close_field();
        self.field = Some(ExplainField::new(name));
    }

    fn read(&mut self, field: &'a str, text: Option<&Cow<'a, str>>) {
        self.open_read(ExplainRead::new(
            ExplainSource::Field(Cow::Borrowed(field)),
            text.cloned(),
        ));
    }

    fn constant(&mut self, value: &'a str) {
        self.open_read(ExplainRead::new(
            ExplainSource::Constant,
            Some(Cow::Borrowed(value)),
        ));
    }

    fn step(&mut self, op: &'a CompiledOp, output: Option<&Cow<'a, str>>) {
        if let Some(read) = &mut self.read {
            read.push(ExplainStep::new(op.name(), output.cloned()));
        }
    }

    fn coerced(&mut self, value: Option<&Value<'a>>) {
        if let (Some(read), Some(value)) = (&mut self.read, value) {
            read.set_value(value.clone());
        }
    }
}
