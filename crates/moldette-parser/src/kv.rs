use std::borrow::Cow;

use moldette_core::{Malformed, SpecError};

use crate::slots::with_slots;
use crate::text::without_line_break;

const QUOTE: char = '"';
const BACKSLASH: char = '\\';

#[derive(Debug, Clone)]
pub(crate) struct KvKeys {
    pair_delimiter: char,
    key_value_delimiter: char,
    keys: Vec<String>,
}

impl KvKeys {
    pub(crate) fn new(
        pair_delimiter: char,
        key_value_delimiter: char,
        keys: &[String],
    ) -> Result<Self, SpecError> {
        if is_reserved(pair_delimiter) {
            return Err(invalid_argument("pair_delimiter", pair_delimiter));
        }
        if is_reserved(key_value_delimiter) || key_value_delimiter == pair_delimiter {
            return Err(invalid_argument("key_value_delimiter", key_value_delimiter));
        }
        if let Some(key) = keys
            .iter()
            .find(|key| key.is_empty() || key.contains([pair_delimiter, key_value_delimiter]))
        {
            return Err(SpecError::InvalidPath(key.clone()));
        }
        Ok(Self {
            pair_delimiter,
            key_value_delimiter,
            keys: keys.to_vec(),
        })
    }

    pub(crate) fn find<'a>(
        &self,
        raw: &'a str,
        values: &mut [Option<Cow<'a, str>>],
    ) -> Result<(), Malformed> {
        with_slots(self.keys.len(), |decided| {
            self.find_pairs(without_line_break(raw), values, decided)
        })
    }

    fn find_pairs<'a>(
        &self,
        mut rest: &'a str,
        values: &mut [Option<Cow<'a, str>>],
        decided: &mut [Option<()>],
    ) -> Result<(), Malformed> {
        loop {
            rest = rest.trim_start_matches(self.pair_delimiter);
            if rest.is_empty() {
                break;
            }
            let (key, value, next) = self.split_pair(rest)?;
            let mut slots = values
                .iter_mut()
                .zip(decided.iter_mut())
                .zip(&self.keys)
                .filter(|((_, decided), path)| decided.is_none() && *path == key)
                .map(|((slot, decided), _)| {
                    *decided = Some(());
                    slot
                });
            if let Some(first) = slots.next() {
                for slot in slots {
                    *slot = value.clone();
                }
                *first = value;
            }
            rest = next;
        }
        Ok(())
    }

    fn split_pair<'a>(
        &self,
        text: &'a str,
    ) -> Result<(&'a str, Option<Cow<'a, str>>, &'a str), Malformed> {
        let Some((key, delimiter, after)) =
            split_at_first(text, [self.pair_delimiter, self.key_value_delimiter])
        else {
            return Ok((text, None, ""));
        };
        if delimiter == self.pair_delimiter {
            return Ok((key, None, after));
        }
        if key.is_empty() {
            return Err(Malformed::new("pair has no key"));
        }
        let Some(quoted) = after.strip_prefix(QUOTE) else {
            return Ok(match after.split_once(self.pair_delimiter) {
                Some((value, next)) => (key, Some(Cow::Borrowed(value)), next),
                None => (key, Some(Cow::Borrowed(after)), ""),
            });
        };
        let (value, after) = unquote(quoted)?;
        if after.is_empty() {
            return Ok((key, Some(value), ""));
        }
        after
            .strip_prefix(self.pair_delimiter)
            .map(|next| (key, Some(value), next))
            .ok_or_else(|| Malformed::new("text follows a closing quote"))
    }
}

fn is_reserved(delimiter: char) -> bool {
    matches!(delimiter, QUOTE | '\n' | '\r')
}

fn invalid_argument(name: &str, delimiter: char) -> SpecError {
    SpecError::InvalidArgument {
        name: name.into(),
        value: delimiter.into(),
    }
}

fn split_at_first(text: &str, delimiters: [char; 2]) -> Option<(&str, char, &str)> {
    let (at, delimiter) = text.char_indices().find(|(_, c)| delimiters.contains(c))?;
    let (before, after) = text.split_at_checked(at)?;
    let mut after = after.chars();
    after.next();
    Some((before, delimiter, after.as_str()))
}

fn unquote(quoted: &str) -> Result<(Cow<'_, str>, &str), Malformed> {
    let mut unescaped: Option<String> = None;
    let mut rest = quoted;
    loop {
        let (text, special, after) = split_at_first(rest, [QUOTE, BACKSLASH])
            .ok_or_else(|| Malformed::new("quoted value is not closed"))?;
        if special == QUOTE {
            let value = match unescaped {
                Some(mut buffer) => {
                    buffer.push_str(text);
                    Cow::Owned(buffer)
                }
                None => Cow::Borrowed(text),
            };
            return Ok((value, after));
        }
        let (c, next) =
            unescape(after).ok_or_else(|| Malformed::new("invalid escape in quoted value"))?;
        let buffer = unescaped.get_or_insert_with(String::new);
        buffer.push_str(text);
        buffer.push(c);
        rest = next;
    }
}

fn unescape(text: &str) -> Option<(char, &str)> {
    let mut chars = text.chars();
    let c = match chars.next()? {
        QUOTE => QUOTE,
        BACKSLASH => BACKSLASH,
        '/' => '/',
        'b' => '\u{8}',
        'f' => '\u{c}',
        'n' => '\n',
        'r' => '\r',
        't' => '\t',
        'u' => return unescape_unicode(chars.as_str()),
        _ => return None,
    };
    Some((c, chars.as_str()))
}

fn unescape_unicode(text: &str) -> Option<(char, &str)> {
    let (unit, rest) = utf16_unit(text)?;
    if let Some(c) = char::from_u32(unit.into()) {
        return Some((c, rest));
    }
    let (low, rest) = utf16_unit(rest.strip_prefix("\\u")?)?;
    let c = char::decode_utf16([unit, low]).next()?.ok()?;
    Some((c, rest))
}

fn utf16_unit(text: &str) -> Option<(u16, &str)> {
    let (digits, rest) = text.split_at_checked(4)?;
    if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u16::from_str_radix(digits, 16)
        .ok()
        .map(|unit| (unit, rest))
}
