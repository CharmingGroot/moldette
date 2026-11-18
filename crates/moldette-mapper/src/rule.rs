use std::borrow::Cow;

use moldette_core::{Field, FieldRecord, Mapped, SpecError, Type, Value};

use crate::coerce::coerce;
use crate::op::{CompiledOp, CustomOp, DeclaredOp, Op, invalid_argument};
use crate::recorder::Recorder;
use crate::telemetry;

/// Where one output field takes its value from, and the ops applied to it.
///
/// Ops run on the text of the source in the order they are added, before the
/// text is converted to the field's schema type. An op that cannot handle its
/// input leaves the value absent (#5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub(crate) target: String,
    source: Source,
    fallback: Option<String>,
    via: Vec<DeclaredOp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Source {
    Field(String),
    Coalesce(Vec<String>),
    Const(String),
}

impl Rule {
    /// Reads the output field `target` from the source field `from`.
    pub fn new(target: impl Into<String>, from: impl Into<String>) -> Self {
        Self::with_source(target, Source::Field(from.into()))
    }

    /// Reads the first of the source fields `from` that is present in the
    /// record. The build fails when `from` is empty.
    pub fn coalesce(
        target: impl Into<String>,
        from: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self::with_source(
            target,
            Source::Coalesce(from.into_iter().map(Into::into).collect()),
        )
    }

    /// Takes `value` without reading the record.
    pub fn constant(target: impl Into<String>, value: impl Into<String>) -> Self {
        Self::with_source(target, Source::Const(value.into()))
    }

    fn with_source(target: impl Into<String>, source: Source) -> Self {
        Self {
            target: target.into(),
            source,
            fallback: None,
            via: Vec::new(),
        }
    }

    /// Reads the source field `from` when the primary source yields no value.
    /// The ops of the rule run on it as well.
    pub fn fallback(mut self, from: impl Into<String>) -> Self {
        self.fallback = Some(from.into());
        self
    }

    /// Removes leading and trailing whitespace.
    pub fn trim(self) -> Self {
        self.push(DeclaredOp::Trim)
    }

    /// Splits the text on `separator` and keeps the part at the zero-based
    /// `index`.
    pub fn split(self, separator: char, index: usize) -> Self {
        self.push(DeclaredOp::Split { separator, index })
    }

    /// Keeps the text matched by the capture group of `pattern`. The build
    /// fails unless `pattern` is a valid regular expression with exactly one
    /// capture group.
    pub fn extract(self, pattern: impl Into<String>) -> Self {
        self.push(DeclaredOp::Extract(pattern.into()))
    }

    /// Replaces the text with its value in `table`. The first entry wins when
    /// a key repeats.
    pub fn lookup(
        self,
        table: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        let table = table
            .into_iter()
            .map(|(key, value)| (key.into(), value.into()))
            .collect();
        self.push(DeclaredOp::Lookup(table))
    }

    /// Reads the text with the first of `formats` that matches it and writes
    /// it as RFC 3339. Formats use the version 2 format description syntax of
    /// the `time` crate; a format without an offset reads the text as UTC. The
    /// build fails when `formats` is empty or a format is not valid.
    pub fn timestamp(self, formats: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let formats = formats.into_iter().map(Into::into).collect();
        self.push(DeclaredOp::Timestamp(formats))
    }

    /// Treats an empty text as absent.
    pub fn empty_as_none(self) -> Self {
        self.push(DeclaredOp::EmptyAsNone)
    }

    /// Applies a caller-defined op, which may be an `Arc<dyn Op>`. Each call
    /// wraps `op` in a new `Arc`, so rules with custom ops are equal only when
    /// one is a clone of the other.
    pub fn via(self, op: impl Op + 'static) -> Self {
        self.push(DeclaredOp::Custom(CustomOp::new(op)))
    }

    fn push(mut self, op: DeclaredOp) -> Self {
        self.via.push(op);
        self
    }

    pub(crate) fn compile(self) -> Result<CompiledRule, SpecError> {
        if matches!(&self.source, Source::Coalesce(from) if from.is_empty()) {
            return Err(invalid_argument("coalesce.from", ""));
        }
        let via = self
            .via
            .iter()
            .map(DeclaredOp::compile)
            .collect::<Result<_, _>>()?;
        Ok(CompiledRule {
            declared: self,
            via,
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CompiledRule {
    declared: Rule,
    via: Vec<CompiledOp>,
}

impl PartialEq for CompiledRule {
    fn eq(&self, other: &Self) -> bool {
        self.declared == other.declared
    }
}

impl Eq for CompiledRule {}

impl CompiledRule {
    pub(crate) fn run<'a, R: Recorder<'a>>(
        &'a self,
        field: &'a Field,
        record: &FieldRecord<'a>,
        mapped: &mut Mapped<'a>,
        recorder: &mut R,
    ) {
        recorder.field(field.name());
        let primary = self.read(record, recorder);
        let mut found = primary.is_some();
        let mut value = primary.and_then(|text| self.convert(text, field.ty(), recorder));
        if value.is_none() {
            let fallback = self
                .declared
                .fallback
                .as_deref()
                .and_then(|name| read_field(name, record, recorder));
            found |= fallback.is_some();
            value = fallback.and_then(|text| self.convert(text, field.ty(), recorder));
        }
        match value {
            Some(value) => mapped.insert(field.name(), value),
            None if found => {
                telemetry::emptied(field.name());
                mapped.mark_emptied(field.name());
            }
            None => {}
        }
    }

    fn read<'a, R: Recorder<'a>>(
        &'a self,
        record: &FieldRecord<'a>,
        recorder: &mut R,
    ) -> Option<Cow<'a, str>> {
        match &self.declared.source {
            Source::Field(name) => read_field(name, record, recorder),
            Source::Coalesce(names) => names
                .iter()
                .find_map(|name| read_field(name, record, recorder)),
            Source::Const(value) => {
                recorder.constant(value);
                Some(Cow::Borrowed(value.as_str()))
            }
        }
    }

    fn convert<'a, R: Recorder<'a>>(
        &'a self,
        text: Cow<'a, str>,
        ty: Type,
        recorder: &mut R,
    ) -> Option<Value<'a>> {
        let text = self.via.iter().try_fold(text, |text, op| {
            let output = op.apply(text);
            recorder.step(op, output.as_ref());
            output
        })?;
        let value = coerce(text, ty);
        recorder.coerced(value.as_ref());
        value
    }
}

fn read_field<'a, R: Recorder<'a>>(
    name: &'a str,
    record: &FieldRecord<'a>,
    recorder: &mut R,
) -> Option<Cow<'a, str>> {
    let text = record.get_cow(name).cloned();
    recorder.read(name, text.as_ref());
    text
}
