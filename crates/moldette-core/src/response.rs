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

/// The number of responses per status. The resp layer never returns
/// [`Response::Malformed`]; parser results are counted with the same type.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StatusCounts {
    ok: u64,
    malformed: u64,
    missing_required: u64,
    emptied: u64,
}

impl StatusCounts {
    pub fn ok(&self) -> u64 {
        self.ok
    }

    pub fn malformed(&self) -> u64 {
        self.malformed
    }

    pub fn missing_required(&self) -> u64 {
        self.missing_required
    }

    pub fn emptied(&self) -> u64 {
        self.emptied
    }
}

impl<'r, 'a: 'r> FromIterator<&'r Response<'a>> for StatusCounts {
    fn from_iter<I: IntoIterator<Item = &'r Response<'a>>>(responses: I) -> Self {
        let mut counts = Self::default();
        for response in responses {
            match response {
                Response::Ok(_) => counts.ok += 1,
                Response::Malformed(_) => counts.malformed += 1,
                Response::MissingRequired(_) => counts.missing_required += 1,
                Response::Emptied(_) => counts.emptied += 1,
            }
        }
        counts
    }
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

    #[test]
    fn status_counts_count_each_variant() {
        let responses = [
            Response::Ok(Mapped::new()),
            Response::Malformed(Malformed::new("unterminated quote")),
            Response::MissingRequired(vec!["order_id".into()]),
            Response::Ok(Mapped::new()),
            Response::Emptied(vec!["status".into()]),
            Response::MissingRequired(vec!["amount".into()]),
            Response::Ok(Mapped::new()),
        ];

        let counts: StatusCounts = responses.iter().collect();

        assert_eq!(
            (
                counts.ok(),
                counts.malformed(),
                counts.missing_required(),
                counts.emptied()
            ),
            (3, 1, 2, 1)
        );
    }

    #[test]
    fn status_counts_are_zero_without_responses() {
        let counts: StatusCounts = std::iter::empty::<&Response<'_>>().collect();
        assert_eq!(counts, StatusCounts::default());
    }
}
