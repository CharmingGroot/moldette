use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::panic::RefUnwindSafe;
use std::sync::Arc;

use moldette_core::SpecError;
use regex::Regex;
use time::error::TryFromParsed;
use time::format_description::well_known::Rfc3339;
use time::format_description::{self, OwnedFormatItem};
use time::parsing::Parsed;
use time::{Date, OffsetDateTime, PrimitiveDateTime, UtcOffset};

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

impl<T: Op + ?Sized> Op for Arc<T> {
    fn apply<'a>(&'a self, text: &'a str) -> Option<Cow<'a, str>> {
        (**self).apply(text)
    }

    fn name(&self) -> &str {
        (**self).name()
    }
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
    Split {
        separator: char,
        index: usize,
    },
    Extract(String),
    Lookup {
        table: Vec<(String, String)>,
        unmatched: Unmatched,
    },
    Timestamp {
        formats: Vec<String>,
        defaults: TimestampDefaults,
    },
    EmptyAsNone,
    Custom(CustomOp),
}

/// What the `lookup` op writes for a text that is not in its table (#95).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Unmatched {
    /// No value, so the field is reported as emptied (#5).
    #[default]
    Absent,
    /// This value.
    Value(String),
    /// The text itself.
    Keep,
}

/// Values the `timestamp` op uses for what a format does not read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TimestampDefaults {
    year: Option<i32>,
    offset: Option<String>,
}

impl TimestampDefaults {
    pub fn new() -> Self {
        Self::default()
    }

    /// The year of a text whose format reads no year. The build fails
    /// outside -9999 to 9999.
    pub fn year(mut self, year: i32) -> Self {
        self.year = Some(year);
        self
    }

    /// The UTC offset, written `+HH:MM` or `-HH:MM`, of a text whose format
    /// reads no offset; without it the text is read as UTC. The build fails
    /// on any other form and on an offset of a day or more.
    pub fn offset(mut self, offset: impl Into<String>) -> Self {
        self.offset = Some(offset.into());
        self
    }

    fn compile(&self) -> Result<(Option<i32>, UtcOffset), SpecError> {
        let year = match self.year {
            Some(year) if !(Date::MIN.year()..=Date::MAX.year()).contains(&year) => {
                return Err(invalid_argument("timestamp.year", &year.to_string()));
            }
            year => year,
        };
        let offset = match &self.offset {
            Some(text) => {
                parse_offset(text).ok_or_else(|| invalid_argument("timestamp.offset", text))?
            }
            None => UtcOffset::UTC,
        };
        Ok((year, offset))
    }
}

fn parse_offset(text: &str) -> Option<UtcOffset> {
    let format =
        format_description::parse_borrowed::<2>("[offset_hour sign:mandatory]:[offset_minute]")
            .ok()?;
    UtcOffset::parse(text, &format)
        .ok()
        .filter(|offset| offset.whole_seconds().unsigned_abs() < 24 * 60 * 60)
}

#[derive(Debug, Clone)]
pub(crate) struct TimeReader {
    formats: Vec<OwnedFormatItem>,
    year: Option<i32>,
    offset: UtcOffset,
}

#[derive(Debug, Clone)]
pub(crate) enum CompiledOp {
    Trim,
    Split {
        separator: char,
        index: usize,
    },
    Extract(Regex),
    Lookup {
        table: BTreeMap<String, String>,
        unmatched: Unmatched,
    },
    Timestamp(TimeReader),
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
            Self::Lookup { table, unmatched } => {
                let mut compiled = BTreeMap::new();
                for (key, value) in table {
                    compiled.entry(key.clone()).or_insert_with(|| value.clone());
                }
                CompiledOp::Lookup {
                    table: compiled,
                    unmatched: unmatched.clone(),
                }
            }
            Self::Timestamp { formats, .. } if formats.is_empty() => {
                return Err(invalid_argument("timestamp.formats", ""));
            }
            Self::Timestamp { formats, defaults } => {
                let formats = formats
                    .iter()
                    .map(|format| {
                        format_description::parse_owned::<2>(format)
                            .map_err(|_| invalid_argument("timestamp.formats", format))
                    })
                    .collect::<Result<_, _>>()?;
                let (year, offset) = defaults.compile()?;
                CompiledOp::Timestamp(TimeReader {
                    formats,
                    year,
                    offset,
                })
            }
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
            Self::Lookup { .. } => "lookup",
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
            Self::Lookup { table, unmatched } => match table.get(text.as_ref()) {
                Some(value) => Some(Cow::Borrowed(value.as_str())),
                None => match unmatched {
                    Unmatched::Absent => None,
                    Unmatched::Value(value) => Some(Cow::Borrowed(value.as_str())),
                    Unmatched::Keep => Some(text),
                },
            },
            Self::Timestamp(reader) => reader
                .read(&text)
                .and_then(|time| time.format(&Rfc3339).ok())
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

impl TimeReader {
    pub(crate) fn read_rfc3339(&self, text: &str) -> Option<OffsetDateTime> {
        self.read(text)
            .filter(|time| time.format_into(&mut io::sink(), &Rfc3339).is_ok())
    }

    fn read(&self, text: &str) -> Option<OffsetDateTime> {
        self.formats
            .iter()
            .find_map(|format| self.parse(text, format))
    }

    fn parse(&self, text: &str, format: &OwnedFormatItem) -> Option<OffsetDateTime> {
        let mut parsed = Parsed::new();
        parsed
            .parse_item(text.as_bytes(), format)
            .ok()
            .filter(|rest| rest.is_empty())?;
        if let (None, Some(year)) = (parsed.year(), self.year) {
            parsed.set_year(year)?;
        }
        // A format that reads no time of day reads the start of the day.
        if parsed.hour_24().is_none() && parsed.hour_12().is_none() {
            parsed.set_hour_24(0)?;
            parsed.set_minute(0)?;
            parsed.set_second(0)?;
        }
        match OffsetDateTime::try_from(parsed) {
            Ok(timestamp) => Some(timestamp),
            Err(TryFromParsed::InsufficientInformation) => PrimitiveDateTime::try_from(parsed)
                .ok()
                .map(|time| time.assume_offset(self.offset)),
            Err(_) => None,
        }
    }
}
