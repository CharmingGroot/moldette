use std::fmt;

use moldette_core::SpecError;
use regex::Regex;

/// The default of [`Framer::max_record_len`].
pub const DEFAULT_MAX_RECORD_LEN: usize = 16 << 20;

/// Splits an input stream, fed in chunks of bytes, into raw records (#103).
///
/// A chunk may end anywhere, even inside a UTF-8 sequence; each record is
/// checked as UTF-8 once it is complete. After an error the framer stops and
/// returns that error again.
#[derive(Debug, Clone)]
pub struct Framer {
    mode: Mode,
    max_len: usize,
    pending: Vec<u8>,
    emitted: usize,
    error: Option<FrameError>,
}

#[derive(Debug, Clone)]
enum Mode {
    Lines,
    Multiline {
        start: Regex,
        current: Option<Vec<u8>>,
    },
    Json(JsonScanner),
}

impl Framer {
    /// One record per line, without its `\n` or `\r\n`.
    pub fn lines() -> Self {
        Self::new(Mode::Lines)
    }

    /// A record starts at a line matching `start`, and each following line
    /// that does not match is appended to it after a `\n`. Lines before the
    /// first match start a record of their own. The build fails when `start`
    /// is not a valid regular expression.
    pub fn multiline(start: &str) -> Result<Self, SpecError> {
        let start = Regex::new(start).map_err(|_| SpecError::InvalidArgument {
            name: "record_start".into(),
            value: start.into(),
        })?;
        Ok(Self::new(Mode::Multiline {
            start,
            current: None,
        }))
    }

    /// One record per JSON value, the values separated by whitespace, or one
    /// record per element when the input is a single top-level array. Only the
    /// boundaries of values are found; their content is left to the parser.
    pub fn json() -> Self {
        Self::new(Mode::Json(JsonScanner::default()))
    }

    fn new(mode: Mode) -> Self {
        Self {
            mode,
            max_len: DEFAULT_MAX_RECORD_LEN,
            pending: Vec::new(),
            emitted: 0,
            error: None,
        }
    }

    /// Fails a record longer than `len` bytes instead of buffering it.
    pub fn max_record_len(mut self, len: usize) -> Self {
        self.max_len = len;
        self
    }

    /// Frames `chunk`, calling `emit` with each record completed by it.
    pub fn push(&mut self, chunk: &[u8], emit: &mut impl FnMut(&str)) -> Result<(), FrameError> {
        self.run(|framer| {
            framer.pending.extend_from_slice(chunk);
            framer.frame(false, emit)
        })
    }

    /// Frames what remains at the end of the input.
    pub fn finish(&mut self, emit: &mut impl FnMut(&str)) -> Result<(), FrameError> {
        self.run(|framer| framer.frame(true, emit))
    }

    fn run(
        &mut self,
        step: impl FnOnce(&mut Self) -> Result<(), FrameErrorKind>,
    ) -> Result<(), FrameError> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        step(self).map_err(|kind| {
            let error = FrameError {
                kind,
                record: self.emitted + 1,
            };
            self.pending = Vec::new();
            self.error = Some(error.clone());
            error
        })
    }

    fn frame(&mut self, end: bool, emit: &mut impl FnMut(&str)) -> Result<(), FrameErrorKind> {
        match &mut self.mode {
            Mode::Lines => {
                let consumed = frame_lines(&self.pending, end, self.max_len, &mut |line| {
                    emit_record(line, &mut self.emitted, emit)
                })?;
                self.pending.drain(..consumed);
            }
            Mode::Multiline { start, current } => {
                let max_len = self.max_len;
                let emitted = &mut self.emitted;
                let consumed = frame_lines(&self.pending, end, max_len, &mut |line| {
                    let text =
                        std::str::from_utf8(line).map_err(|_| FrameErrorKind::InvalidUtf8)?;
                    match current {
                        Some(record) if !start.is_match(text) => {
                            if record.len() + 1 + line.len() > max_len {
                                return Err(FrameErrorKind::TooLong(max_len));
                            }
                            record.push(b'\n');
                            record.extend_from_slice(line);
                        }
                        _ => {
                            if let Some(record) = current.replace(line.to_vec()) {
                                emit_record(&record, emitted, emit)?;
                            }
                        }
                    }
                    Ok(())
                })?;
                self.pending.drain(..consumed);
                if end && let Some(record) = current.take() {
                    emit_record(&record, &mut self.emitted, emit)?;
                }
            }
            Mode::Json(scanner) => {
                let consumed = scanner.scan(&self.pending, end, self.max_len, &mut |value| {
                    emit_record(value, &mut self.emitted, emit)
                })?;
                self.pending.drain(..consumed);
            }
        }
        Ok(())
    }
}

fn emit_record(
    record: &[u8],
    emitted: &mut usize,
    emit: &mut impl FnMut(&str),
) -> Result<(), FrameErrorKind> {
    let text = std::str::from_utf8(record).map_err(|_| FrameErrorKind::InvalidUtf8)?;
    emit(text);
    *emitted += 1;
    Ok(())
}

/// Calls `line` with each complete line of `bytes`, and with the rest at the
/// end of the input, and returns the number of bytes consumed.
fn frame_lines(
    bytes: &[u8],
    end: bool,
    max_len: usize,
    line: &mut impl FnMut(&[u8]) -> Result<(), FrameErrorKind>,
) -> Result<usize, FrameErrorKind> {
    let mut start = 0;
    while let Some(offset) = bytes[start..].iter().position(|&byte| byte == b'\n') {
        let text = &bytes[start..start + offset];
        let text = text.strip_suffix(b"\r").unwrap_or(text);
        if text.len() > max_len {
            return Err(FrameErrorKind::TooLong(max_len));
        }
        line(text)?;
        start += offset + 1;
    }
    let rest = &bytes[start..];
    if rest.len() > max_len {
        return Err(FrameErrorKind::TooLong(max_len));
    }
    if end && !rest.is_empty() {
        line(rest.strip_suffix(b"\r").unwrap_or(rest))?;
        start = bytes.len();
    }
    Ok(start)
}

#[derive(Debug, Clone, Default)]
struct JsonScanner {
    array: Array,
    value: Option<Value>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Array {
    #[default]
    Unknown,
    None,
    ElementOrEnd,
    Element,
    CommaOrEnd,
    Closed,
}

#[derive(Debug, Clone, Copy)]
struct Value {
    start: usize,
    scanned: usize,
    depth: usize,
    in_string: bool,
    escaped: bool,
    scalar: bool,
}

impl JsonScanner {
    fn scan(
        &mut self,
        bytes: &[u8],
        end: bool,
        max_len: usize,
        value: &mut impl FnMut(&[u8]) -> Result<(), FrameErrorKind>,
    ) -> Result<usize, FrameErrorKind> {
        let mut position = self.value.map_or(0, |value| value.scanned);
        loop {
            if let Some(current) = &mut self.value {
                match scan_value(bytes, position, current) {
                    Some(close) => {
                        let start = current.start;
                        if close - start > max_len {
                            return Err(FrameErrorKind::TooLong(max_len));
                        }
                        self.value = None;
                        value(&bytes[start..close])?;
                        self.array = match self.array {
                            Array::None => Array::None,
                            _ => Array::CommaOrEnd,
                        };
                        position = close;
                    }
                    None => {
                        if bytes.len() - current.start > max_len {
                            return Err(FrameErrorKind::TooLong(max_len));
                        }
                        if !end {
                            let start = current.start;
                            current.scanned = bytes.len() - start;
                            current.start = 0;
                            return Ok(start);
                        }
                        if !current.scalar || current.depth > 0 || current.in_string {
                            return Err(FrameErrorKind::UnterminatedJson);
                        }
                        let start = current.start;
                        self.value = None;
                        value(&bytes[start..])?;
                        return self.close(true).map(|()| bytes.len());
                    }
                }
                continue;
            }
            let Some(offset) = bytes[position..]
                .iter()
                .position(|byte| !byte.is_ascii_whitespace())
            else {
                return self.close(end).map(|()| bytes.len());
            };
            position += offset;
            let byte = bytes[position];
            match (self.array, byte) {
                (Array::Unknown, b'[') => {
                    self.array = Array::ElementOrEnd;
                    position += 1;
                }
                (Array::ElementOrEnd | Array::CommaOrEnd, b']') => {
                    self.array = Array::Closed;
                    position += 1;
                }
                (Array::CommaOrEnd, b',') => {
                    self.array = Array::Element;
                    position += 1;
                }
                (Array::Closed | Array::CommaOrEnd, _) | (Array::Element, b']') => {
                    return Err(FrameErrorKind::UnexpectedJson(byte));
                }
                (_, b']' | b'}' | b',' | b':') => return Err(FrameErrorKind::UnexpectedJson(byte)),
                (array, _) => {
                    if array == Array::Unknown {
                        self.array = Array::None;
                    }
                    self.value = Some(Value {
                        start: position,
                        scanned: position,
                        depth: 0,
                        in_string: false,
                        escaped: false,
                        scalar: !matches!(byte, b'{' | b'[' | b'"'),
                    });
                }
            }
        }
    }

    fn close(&self, end: bool) -> Result<(), FrameErrorKind> {
        match (end, self.array) {
            (true, Array::ElementOrEnd | Array::Element | Array::CommaOrEnd) => {
                Err(FrameErrorKind::UnterminatedJson)
            }
            _ => Ok(()),
        }
    }
}

/// Scans the value from `from` and returns the end of its text once it is
/// complete. A scalar ends before the first byte that cannot continue it.
fn scan_value(bytes: &[u8], from: usize, value: &mut Value) -> Option<usize> {
    for (index, &byte) in bytes.iter().enumerate().skip(from) {
        if value.scalar {
            if byte.is_ascii_whitespace() || matches!(byte, b',' | b']' | b'}') {
                return Some(index);
            }
            continue;
        }
        if value.in_string {
            match (value.escaped, byte) {
                (true, _) => value.escaped = false,
                (false, b'\\') => value.escaped = true,
                (false, b'"') => {
                    value.in_string = false;
                    if value.depth == 0 {
                        return Some(index + 1);
                    }
                }
                _ => {}
            }
            continue;
        }
        match byte {
            b'"' => value.in_string = true,
            b'{' | b'[' => value.depth += 1,
            b'}' | b']' => {
                value.depth = value.depth.saturating_sub(1);
                if value.depth == 0 {
                    return Some(index + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// Why framing stopped, and at which record, counted from 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameError {
    kind: FrameErrorKind,
    record: usize,
}

impl FrameError {
    pub fn kind(&self) -> &FrameErrorKind {
        &self.kind
    }

    pub fn record(&self) -> usize {
        self.record
    }
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "record {}: {}", self.record, self.kind)
    }
}

impl std::error::Error for FrameError {}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FrameErrorKind {
    InvalidUtf8,
    /// The record is longer than this limit in bytes.
    TooLong(usize),
    /// A byte that cannot appear here between JSON values.
    UnexpectedJson(u8),
    /// The input ends inside a JSON value or array.
    UnterminatedJson,
}

impl fmt::Display for FrameErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUtf8 => f.write_str("not valid UTF-8"),
            Self::TooLong(limit) => write!(f, "longer than {limit} bytes"),
            Self::UnexpectedJson(byte) => {
                write!(f, "unexpected `{}` between JSON values", char::from(*byte))
            }
            Self::UnterminatedJson => f.write_str("the input ends inside a JSON value"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_in(framer: &Framer, input: &[u8], chunk: usize) -> (Vec<String>, Option<FrameError>) {
        let mut framer = framer.clone();
        let mut records = Vec::new();
        let mut emit = |record: &str| records.push(record.to_owned());
        let mut result = Ok(());
        for part in input.chunks(chunk.max(1)) {
            result = framer.push(part, &mut emit);
            if result.is_err() {
                break;
            }
        }
        if result.is_ok() {
            result = framer.finish(&mut emit);
        }
        (records, result.err())
    }

    fn frame(framer: &Framer, input: &[u8]) -> (Vec<String>, Option<FrameError>) {
        let whole = frame_in(framer, input, input.len());
        for chunk in 1..input.len() {
            assert_eq!(frame_in(framer, input, chunk), whole, "chunks of {chunk}");
        }
        whole
    }

    fn records(framer: &Framer, input: &str) -> Vec<String> {
        let (records, error) = frame(framer, input.as_bytes());
        assert_eq!(error, None, "{input:?}");
        records
    }

    fn error(framer: &Framer, input: &[u8]) -> (Vec<String>, FrameErrorKind, usize) {
        let (records, error) = frame(framer, input);
        let error = error.expect("framing fails");
        (records, error.kind().clone(), error.record())
    }

    #[test]
    fn lines_split_on_line_breaks_in_any_chunks() {
        let lines = Framer::lines();
        assert_eq!(records(&lines, "a\nb\r\n\nc"), ["a", "b", "", "c"]);
        assert_eq!(records(&lines, "a\n"), ["a"]);
        assert_eq!(records(&lines, "é\n가나\n"), ["é", "가나"]);
        assert!(records(&lines, "").is_empty());
    }

    #[test]
    fn lines_stop_at_a_record_that_is_not_utf8() {
        let lines = Framer::lines();
        assert_eq!(
            error(&lines, b"a\n\xff\nb\n"),
            (vec!["a".to_owned()], FrameErrorKind::InvalidUtf8, 2)
        );
    }

    #[test]
    fn multiline_appends_lines_until_the_next_record_start() {
        let multiline = Framer::multiline(r"^\d{4}-\d{2}-\d{2} ").expect("valid pattern");
        assert_eq!(
            records(
                &multiline,
                "2026-11-22 failed\n  at a\r\n  at b\n2026-11-22 next\ntrailing"
            ),
            [
                "2026-11-22 failed\n  at a\n  at b",
                "2026-11-22 next\ntrailing"
            ]
        );
        assert_eq!(
            records(&multiline, "orphan\n  more\n2026-11-22 x\n"),
            ["orphan\n  more", "2026-11-22 x"]
        );
    }

    #[test]
    fn multiline_rejects_an_invalid_start_pattern() {
        assert_eq!(
            Framer::multiline("(").err(),
            Some(SpecError::InvalidArgument {
                name: "record_start".into(),
                value: "(".into(),
            })
        );
    }

    #[test]
    fn json_takes_each_value_of_a_stream() {
        let json = Framer::json();
        assert_eq!(
            records(
                &json,
                "{\"a\":1} {\"b\":\"x}\\\"\"}\n[1,[2]]\t\"s\" 3 true\n"
            ),
            [
                "{\"a\":1}",
                "{\"b\":\"x}\\\"\"}",
                "[1,[2]]",
                "\"s\"",
                "3",
                "true"
            ]
        );
        assert!(records(&json, " \n ").is_empty());
    }

    #[test]
    fn json_takes_each_element_of_a_top_level_array() {
        let json = Framer::json();
        assert_eq!(
            records(&json, " [ {\"a\":[1,2]}, \"x\\\"]\" ,3 , null ]\n"),
            ["{\"a\":[1,2]}", "\"x\\\"]\"", "3", "null"]
        );
        assert!(records(&json, "[ ]").is_empty());
    }

    #[test]
    fn json_stops_at_a_broken_structure() {
        let json = Framer::json();
        let one = || vec!["1".to_owned()];
        assert_eq!(
            error(&json, b"[1,]"),
            (one(), FrameErrorKind::UnexpectedJson(b']'), 2)
        );
        assert_eq!(
            error(&json, b"[1 2]"),
            (one(), FrameErrorKind::UnexpectedJson(b'2'), 2)
        );
        assert_eq!(
            error(&json, b"[1] 2"),
            (one(), FrameErrorKind::UnexpectedJson(b'2'), 2)
        );
        assert_eq!(
            error(&json, b"{\"a\":1"),
            (vec![], FrameErrorKind::UnterminatedJson, 1)
        );
        assert_eq!(
            error(&json, b"[1,"),
            (one(), FrameErrorKind::UnterminatedJson, 2)
        );
        assert_eq!(
            error(&json, b"}"),
            (vec![], FrameErrorKind::UnexpectedJson(b'}'), 1)
        );
    }

    #[test]
    fn records_longer_than_the_limit_fail() {
        assert_eq!(
            error(&Framer::lines().max_record_len(3), b"abc\nabcd\n"),
            (vec!["abc".to_owned()], FrameErrorKind::TooLong(3), 2)
        );
        let multiline = Framer::multiline("^S")
            .expect("valid pattern")
            .max_record_len(5);
        assert_eq!(
            error(&multiline, b"S1\n 2\n 3\n"),
            (vec![], FrameErrorKind::TooLong(5), 1)
        );
        assert_eq!(
            error(&Framer::json().max_record_len(5), b"{\"a\":\"long\"}"),
            (vec![], FrameErrorKind::TooLong(5), 1)
        );
    }

    #[test]
    fn public_types_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Framer>();
        assert_send_sync::<FrameError>();
    }

    #[test]
    fn a_framer_that_failed_returns_its_error_again() {
        let mut framer = Framer::lines();
        let mut ignore = |_: &str| {};
        let first = framer
            .push(b"\xff\n", &mut ignore)
            .expect_err("invalid UTF-8");
        assert_eq!(framer.push(b"a\n", &mut ignore), Err(first.clone()));
        assert_eq!(framer.finish(&mut ignore), Err(first));
    }
}
