use std::borrow::Cow;
use std::collections::BTreeMap;

use moldette_core::SpecError;
use regex::Regex;
use time::error::TryFromParsed;
use time::format_description::well_known::Rfc3339;
use time::format_description::{self, OwnedFormatItem};
use time::parsing::Parsed;
use time::{OffsetDateTime, PrimitiveDateTime};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DeclaredOp {
    Trim,
    Split { separator: char, index: usize },
    Extract(String),
    Lookup(Vec<(String, String)>),
    Timestamp(Vec<String>),
    EmptyAsNone,
}

#[derive(Debug, Clone)]
pub(crate) enum CompiledOp {
    Trim,
    Split { separator: char, index: usize },
    Extract(Regex),
    Lookup(BTreeMap<String, String>),
    Timestamp(Vec<OwnedFormatItem>),
    EmptyAsNone,
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
