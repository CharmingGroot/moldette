use std::borrow::Cow;
use std::fmt;

use crate::Mapped;

/// The outcome for one record. Failures are values, not errors (#3).
#[derive(Debug, Clone, PartialEq)]
pub enum Response<'a> {
    Ok(Mapped<'a>),
    Malformed(Malformed),
    MissingRequired(Vec<Cow<'a, str>>),
    /// Fields emptied by the mapper while strict mode is on (#5).
    Emptied(Vec<Cow<'a, str>>),
}

impl Response<'_> {
    pub fn into_owned(self) -> Response<'static> {
        match self {
            Self::Ok(mapped) => Response::Ok(mapped.into_owned()),
            Self::Malformed(m) => Response::Malformed(m),
            Self::MissingRequired(fields) => Response::MissingRequired(owned(fields)),
            Self::Emptied(fields) => Response::Emptied(owned(fields)),
        }
    }
}

fn owned(fields: Vec<Cow<'_, str>>) -> Vec<Cow<'static, str>> {
    fields
        .into_iter()
        .map(|f| Cow::Owned(f.into_owned()))
        .collect()
}

/// A raw record that does not match the format its parser expects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Malformed {
    reason: Cow<'static, str>,
}

impl Malformed {
    pub fn new(reason: impl Into<Cow<'static, str>>) -> Self {
        Self {
            reason: reason.into(),
        }
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl fmt::Display for Malformed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.reason)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emptied_into_owned_keeps_field_names() {
        let name = String::from("status");
        let response = Response::Emptied(vec![Cow::Borrowed(name.as_str())]);
        assert_eq!(
            response.into_owned(),
            Response::Emptied(vec!["status".into()])
        );
    }
}
