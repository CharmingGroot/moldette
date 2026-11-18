use std::borrow::Cow;

use moldette_core::{Malformed, SpecError};
use regex::Regex;

use crate::text::without_line_break;

#[derive(Debug, Clone)]
pub(crate) struct RegexGroups {
    regex: Regex,
    groups: Vec<usize>,
}

impl RegexGroups {
    pub(crate) fn new(pattern: &str, names: &[String]) -> Result<Self, SpecError> {
        let regex = Regex::new(pattern).map_err(|_| SpecError::InvalidArgument {
            name: "pattern".into(),
            value: pattern.into(),
        })?;
        let groups = names
            .iter()
            .map(|name| {
                regex
                    .capture_names()
                    .position(|group| group == Some(name.as_str()))
                    .ok_or_else(|| SpecError::InvalidPath(name.clone()))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { regex, groups })
    }

    pub(crate) fn find<'a>(&self, raw: &'a str) -> Result<Vec<Option<Cow<'a, str>>>, Malformed> {
        let captures = self
            .regex
            .captures(without_line_break(raw))
            .ok_or_else(|| Malformed::new("record does not match the pattern"))?;
        Ok(self
            .groups
            .iter()
            .map(|&group| {
                captures
                    .get(group)
                    .map(|found| Cow::Borrowed(found.as_str()))
            })
            .collect())
    }
}
