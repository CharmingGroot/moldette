use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::panic::RefUnwindSafe;
use std::sync::Arc;

use moldette_core::SpecError;
use regex::Regex;
use time::error::TryFromParsed;
use time::format_description::well_known::Rfc3339;
use time::format_description::{self, OwnedFormatItem};
use time::parsing::Parsed;
use time::{OffsetDateTime, PrimitiveDateTime};

/// A value conversion defined by the caller and added to a rule with
/// [`Rule::via`](crate::Rule::via).
///
/// An op is a pure function of its text: it returns `None` for text it cannot
/// handle (#5) and does not panic. A panic is not caught and reaches the
/// caller of [`MapperSpec::map`](crate::MapperSpec::map) or
/// [`MapperSpec::explain`](crate::MapperSpec::explain) (#15).
pub trait Op: fmt::Debug + Send + Sync + RefUnwindSafe {
    /// Returns the converted text, borrowed from `text` or from the op where
    /// possible, or `None` when the op cannot handle `text`.
    fn apply<'a>(&'a self, text: &'a str) -> Option<Cow<'a, str>>;

    /// The name recorded for the op by
    /// [`MapperSpec::explain`](crate::MapperSpec::explain).
    fn name(&self) -> &str;
}

#[derive(Debug, Clone)]
pub(crate) struct CustomOp(Arc<dyn Op>);

impl CustomOp {
    pub(crate) fn new(op: impl Op + 'static) -> Self {
        Self(Arc::new(op))
    }

    fn apply<'a>(&'a self, text: Cow<'a, str>) -> Option<Cow<'a, str>> {
        match text {
            Cow::Borrowed(text) => self.0.apply(text),
            Cow::Owned(text) => self
                .0
                .apply(&text)
                .map(|converted| Cow::Owned(converted.into_owned())),
        }
    }

    fn name(&self) -> &str {
        self.0.name()
    }
}

impl PartialEq for CustomOp {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for CustomOp {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DeclaredOp {
    Trim,
    Split { separator: char, index: usize },
    Extract(String),
    Lookup(Vec<(String, String)>),
    Timestamp(Vec<String>),
    EmptyAsNone,
    Custom(CustomOp),
}

#[derive(Debug, Clone)]
pub(crate) enum CompiledOp {
    Trim,
    Split { separator: char, index: usize },
    Extract(Regex),
    Lookup(BTreeMap<String, String>),
    Timestamp(Vec<OwnedFormatItem>),
    EmptyAsNone,
    Custom(CustomOp),
}

impl DeclaredOp {
    pub(crate) fn compile(&self) -> Result<CompiledOp, SpecError> {
        Ok(match self {
            Self::Trim => CompiledOp::Trim,
            Self::Split { separator, index } => CompiledOp::Split {
                separator: *separator,
                index: *index,
            },
            Self::Extract(pattern) => CompiledOp::Extract(
                Regex::new(pattern)
                    .ok()
                    .filter(|regex| regex.captures_len() == 2)
                    .ok_or_else(|| invalid_argument("extract.pattern", pattern))?,
            ),
            Self::Lookup(table) => {
                let mut compiled = BTreeMap::new();
                for (key, value) in table {
                    compiled.entry(key.clone()).or_insert_with(|| value.clone());
                }
                CompiledOp::Lookup(compiled)
            }
            Self::Timestamp(formats) if formats.is_empty() => {
                return Err(invalid_argument("timestamp.formats", ""));
            }
            Self::Timestamp(formats) => CompiledOp::Timestamp(
                formats
                    .iter()
                    .map(|format| {
                        format_description::parse_owned::<2>(format)
                            .map_err(|_| invalid_argument("timestamp.formats", format))
                    })
                    .collect::<Result<_, _>>()?,
            ),
            Self::EmptyAsNone => CompiledOp::EmptyAsNone,
            Self::Custom(op) => CompiledOp::Custom(op.clone()),
        })
    }
}

pub(crate) fn invalid_argument(name: &str, value: &str) -> SpecError {
    SpecError::InvalidArgument {
        name: name.to_owned(),
        value: value.to_owned(),
    }
}

impl CompiledOp {
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::Trim => "trim",
            Self::Split { .. } => "split",
            Self::Extract(_) => "extract",
            Self::Lookup(_) => "lookup",
            Self::Timestamp(_) => "timestamp",
            Self::EmptyAsNone => "empty_as_none",
            Self::Custom(op) => op.name(),
        }
    }

    pub(crate) fn apply<'a>(&'a self, text: Cow<'a, str>) -> Option<Cow<'a, str>> {
        match self {
            Self::Trim => substring(text, |text| Some(text.trim())),
            Self::Split { separator, index } => {
                substring(text, |text| text.split(*separator).nth(*index))
            }
            Self::Extract(regex) => {
                substring(text, |text| Some(regex.captures(text)?.get(1)?.as_str()))
            }
            Self::Lookup(table) => table
                .get(text.as_ref())
                .map(|value| Cow::Borrowed(value.as_str())),
            Self::Timestamp(formats) => formats
                .iter()
                .find_map(|format| parse_timestamp(&text, format))
                .and_then(|timestamp| timestamp.format(&Rfc3339).ok())
                .map(Cow::Owned),
            Self::EmptyAsNone => (!text.is_empty()).then_some(text),
            Self::Custom(op) => op.apply(text),
        }
    }
}

fn substring<'a>(
    text: Cow<'a, str>,
    select: impl FnOnce(&str) -> Option<&str>,
) -> Option<Cow<'a, str>> {
    match text {
        Cow::Borrowed(text) => select(text).map(Cow::Borrowed),
        Cow::Owned(text) => select(&text).map(|part| Cow::Owned(part.to_owned())),
    }
}

fn parse_timestamp(text: &str, format: &OwnedFormatItem) -> Option<OffsetDateTime> {
    let mut parsed = Parsed::new();
    parsed
        .parse_item(text.as_bytes(), format)
        .ok()
        .filter(|rest| rest.is_empty())?;
    match OffsetDateTime::try_from(parsed) {
        Ok(timestamp) => Some(timestamp),
        Err(TryFromParsed::InsufficientInformation) => PrimitiveDateTime::try_from(parsed)
            .ok()
            .map(PrimitiveDateTime::assume_utc),
        Err(_) => None,
    }
}
