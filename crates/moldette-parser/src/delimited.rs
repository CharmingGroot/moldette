use std::borrow::Cow;
use std::iter;

use moldette_core::{Malformed, SpecError};

use crate::text::{array_index, without_line_break};

const QUOTE: char = '"';

#[derive(Debug, Clone)]
pub(crate) struct DelimitedColumns {
    delimiter: char,
    targets: Vec<(usize, usize)>,
}

impl DelimitedColumns {
    pub(crate) fn new(delimiter: char, columns: &[String]) -> Result<Self, SpecError> {
        if matches!(delimiter, QUOTE | '\n' | '\r') {
            return Err(SpecError::InvalidArgument {
                name: "delimiter".into(),
                value: delimiter.into(),
            });
        }
        let mut targets = columns
            .iter()
            .enumerate()
            .map(|(field, column)| {
                array_index(column)
                    .map(|index| (index, field))
                    .ok_or_else(|| SpecError::InvalidPath(column.clone()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        targets.sort_by_key(|&(column, _)| column);
        Ok(Self { delimiter, targets })
    }

    pub(crate) fn find<'a>(
        &self,
        raw: &'a str,
        values: &mut [Option<Cow<'a, str>>],
    ) -> Result<(), Malformed> {
        let mut targets = self.targets.iter().peekable();
        let mut rest = Some(without_line_break(raw));
        for column in 0.. {
            let Some(text) = rest else { break };
            let (value, next) = self.split_column(text)?;
            let mut fields =
                iter::from_fn(|| targets.next_if(|&&(c, _)| c == column).map(|&(_, f)| f));
            if let Some(first) = fields.next() {
                for field in fields {
                    if let Some(slot) = values.get_mut(field) {
                        *slot = Some(value.clone());
                    }
                }
                if let Some(slot) = values.get_mut(first) {
                    *slot = Some(value);
                }
            }
            rest = next;
        }
        Ok(())
    }

    fn split_column<'a>(
        &self,
        text: &'a str,
    ) -> Result<(Cow<'a, str>, Option<&'a str>), Malformed> {
        match text.strip_prefix(QUOTE) {
            Some(quoted) => {
                let (value, after) = unquote(quoted)?;
                if after.is_empty() {
                    return Ok((value, None));
                }
                after
                    .strip_prefix(self.delimiter)
                    .map(|next| (value, Some(next)))
                    .ok_or_else(|| Malformed::new("text follows a closing quote"))
            }
            None => Ok(match text.split_once(self.delimiter) {
                Some((value, next)) => (Cow::Borrowed(value), Some(next)),
                None => (Cow::Borrowed(text), None),
            }),
        }
    }
}

fn unquote(quoted: &str) -> Result<(Cow<'_, str>, &str), Malformed> {
    let mut unescaped: Option<String> = None;
    let mut rest = quoted;
    loop {
        let (text, after) = rest
            .split_once(QUOTE)
            .ok_or_else(|| Malformed::new("quoted field is not closed"))?;
        match after.strip_prefix(QUOTE) {
            Some(next) => {
                let buffer = unescaped.get_or_insert_with(String::new);
                buffer.push_str(text);
                buffer.push(QUOTE);
                rest = next;
            }
            None => {
                let value = match unescaped {
                    Some(mut buffer) => {
                        buffer.push_str(text);
                        Cow::Owned(buffer)
                    }
                    None => Cow::Borrowed(text),
                };
                return Ok((value, after));
            }
        }
    }
}
