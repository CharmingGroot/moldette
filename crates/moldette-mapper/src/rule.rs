use std::borrow::Cow;

use moldette_core::{FieldRecord, SpecError};

use crate::op::{CompiledOp, DeclaredOp};

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
    /// record.
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
        self.via(DeclaredOp::Trim)
    }

    /// Splits the text on `separator` and keeps the part at the zero-based
    /// `index`.
    pub fn split(self, separator: char, index: usize) -> Self {
        self.via(DeclaredOp::Split { separator, index })
    }

    /// Keeps the text matched by the capture group of `pattern`. The build
    /// fails unless `pattern` is a valid regular expression with exactly one
    /// capture group.
    pub fn extract(self, pattern: impl Into<String>) -> Self {
        self.via(DeclaredOp::Extract(pattern.into()))
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
        self.via(DeclaredOp::Lookup(table))
    }

    /// Reads the text with the first of `formats` that matches it and writes
    /// it as RFC 3339. Formats use the version 2 format description syntax of
    /// the `time` crate; a format without an offset reads the text as UTC. The
    /// build fails when a format is not valid.
    pub fn timestamp(self, formats: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let formats = formats.into_iter().map(Into::into).collect();
        self.via(DeclaredOp::Timestamp(formats))
    }

    /// Treats an empty text as absent.
    pub fn empty_as_none(self) -> Self {
        self.via(DeclaredOp::EmptyAsNone)
    }

    fn via(mut self, op: DeclaredOp) -> Self {
        self.via.push(op);
        self
    }

    pub(crate) fn compile(self) -> Result<CompiledRule, SpecError> {
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

impl CompiledRule {
    pub(crate) fn texts<'a>(
        &'a self,
        record: &FieldRecord<'a>,
    ) -> impl Iterator<Item = Cow<'a, str>> {
        let primary = match &self.declared.source {
            Source::Field(name) => record.get_cow(name).cloned(),
            Source::Coalesce(names) => names.iter().find_map(|name| record.get_cow(name)).cloned(),
            Source::Const(value) => Some(Cow::Borrowed(value.as_str())),
        };
        let fallback = self
            .declared
            .fallback
            .iter()
            .filter_map(|name| record.get_cow(name).cloned());
        primary.into_iter().chain(fallback)
    }

    pub(crate) fn apply<'a>(&'a self, text: Cow<'a, str>) -> Option<Cow<'a, str>> {
        self.via.iter().try_fold(text, |text, op| op.apply(text))
    }
}
