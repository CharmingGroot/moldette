use std::borrow::Cow;

use moldette_core::{FieldRecord, Malformed, SpecError, validate_field_names};

use crate::custom::{CustomFormat, CustomPaths, Parser};
use crate::delimited::DelimitedColumns;
use crate::json::JsonPaths;
use crate::kv::KvKeys;
use crate::regex::RegexGroups;
use crate::slots::with_slots;
use crate::telemetry;

/// The format of the raw records a spec parses.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Format {
    /// One JSON object per record. A field path is a JSON Pointer (RFC 6901),
    /// with `""` for the whole record. Strings are unescaped and other
    /// non-null values keep their JSON text.
    Json,
    /// One line per record, split into columns at `delimiter`. A field path
    /// is a zero-based column index such as `"0"`. A column starting with
    /// `"` is quoted: it may contain the delimiter, and `""` inside it stands
    /// for one quote. A trailing line break is ignored. The delimiter cannot
    /// be `"`, `\n` or `\r`.
    Delimited { delimiter: char },
    /// One record searched with `pattern` in the syntax of the `regex` crate.
    /// A field path is the name of a capture group, and a group that takes
    /// no part in the match leaves its field absent. The pattern may match
    /// anywhere, so `^` and `$` are needed to match the whole record. A
    /// trailing line break is ignored, and a record without a match is
    /// malformed.
    Regex { pattern: String },
    /// One record of key-value pairs such as logfmt, split into pairs at
    /// `pair_delimiter` and each pair into key and value at the first
    /// `key_value_delimiter`. A field path is a key, and the first pair with
    /// that key gives the value. Consecutive pair delimiters count as one. A
    /// value starting with `"` is quoted: it may contain both delimiters and
    /// uses the backslash escapes of JSON strings. A key with no
    /// `key_value_delimiter` leaves its field absent, and a pair with an empty
    /// key is malformed. A trailing line break is ignored. The delimiters must
    /// differ and cannot be `"`, `\n` or `\r`.
    Kv {
        pair_delimiter: char,
        key_value_delimiter: char,
    },
    /// One record read by a [`Parser`] defined by the caller. A field path is
    /// resolved by [`Parser::path`] when the spec is built, and a trailing
    /// line break is removed before [`Parser::read`].
    Custom(CustomFormat),
}

impl Format {
    pub fn custom(parser: impl Parser + 'static) -> Self {
        Self::Custom(CustomFormat::new(parser))
    }

    fn name(&self) -> &str {
        match self {
            Self::Json => "json",
            Self::Delimited { .. } => "delimited",
            Self::Regex { .. } => "regex",
            Self::Kv { .. } => "kv",
            Self::Custom(format) => format.name(),
        }
    }
}

/// A validated description of how to parse raw records. Immutable once
/// built.
///
/// Two specs are equal when they are built from equal formats, fields, bodies
/// and `empty_as_none` settings, so a custom format must share its parser.
#[derive(Debug, Clone)]
pub struct ParserSpec {
    format: Format,
    fields: Vec<(String, String)>,
    paths: Paths,
    bodies: Vec<Body>,
    field_count: usize,
    empty_as_none: bool,
}

#[derive(Debug, Clone)]
enum Paths {
    Json(JsonPaths),
    Delimited(DelimitedColumns),
    Regex(RegexGroups),
    Kv(KvKeys),
    Custom(CustomPaths),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Body {
    path: String,
    spec: ParserSpec,
}

impl ParserSpec {
    pub fn builder(format: Format) -> ParserSpecBuilder {
        ParserSpecBuilder {
            format,
            fields: Vec::new(),
            bodies: Vec::new(),
            empty_as_none: false,
        }
    }

    /// Parses one raw record. Values borrow from `raw` where possible, and a
    /// missing or null value is left absent.
    pub fn parse<'a>(&'a self, raw: &'a str) -> Result<FieldRecord<'a>, Malformed> {
        telemetry::parse_span(|| self.format.name()).in_scope(|| {
            let mut record = FieldRecord::with_capacity(self.field_count);
            self.read(raw, &mut |name, value| record.insert(name, value))?;
            Ok(record)
        })
    }

    fn read<'s, 't>(
        &'s self,
        raw: &'t str,
        sink: &mut dyn FnMut(&'s str, Cow<'t, str>),
    ) -> Result<(), Malformed> {
        with_slots(self.fields.len() + self.bodies.len(), |values| {
            self.paths
                .find(raw, values)
                .inspect_err(|_| telemetry::malformed(|| self.format.name()))?;
            self.emit(values, sink)
        })
    }

    fn emit<'s, 't>(
        &'s self,
        values: &mut [Option<Cow<'t, str>>],
        sink: &mut dyn FnMut(&'s str, Cow<'t, str>),
    ) -> Result<(), Malformed> {
        let mut values = values.iter_mut().map(|value| {
            value
                .take()
                .filter(|value| !(self.empty_as_none && value.is_empty()))
        });
        for ((name, _), value) in self.fields.iter().zip(values.by_ref()) {
            if let Some(value) = value {
                sink(name, value);
            }
        }
        for (body, text) in self.bodies.iter().zip(values) {
            let Some(text) = text else { continue };
            telemetry::body_span(&body.path, || body.spec.format.name())
                .in_scope(|| match text {
                    Cow::Borrowed(text) => body.spec.read(text, sink),
                    Cow::Owned(text) => body.spec.read(&text, &mut |name, value| {
                        sink(name, Cow::Owned(value.into_owned()));
                    }),
                })
                .map_err(|err| Malformed::new(format!("body `{}`: {err}", body.path)))?;
        }
        Ok(())
    }

    fn collect_names<'s>(&'s self, names: &mut Vec<&'s str>) {
        names.extend(self.fields.iter().map(|(name, _)| name.as_str()));
        for body in &self.bodies {
            body.spec.collect_names(names);
        }
    }
}

impl PartialEq for ParserSpec {
    fn eq(&self, other: &Self) -> bool {
        self.format == other.format
            && self.fields == other.fields
            && self.bodies == other.bodies
            && self.empty_as_none == other.empty_as_none
    }
}

impl Eq for ParserSpec {}

impl Paths {
    fn new(format: Format, paths: &[String]) -> Result<Self, SpecError> {
        Ok(match format {
            Format::Json => Self::Json(JsonPaths::new(paths)?),
            Format::Delimited { delimiter } => {
                Self::Delimited(DelimitedColumns::new(delimiter, paths)?)
            }
            Format::Regex { pattern } => Self::Regex(RegexGroups::new(&pattern, paths)?),
            Format::Kv {
                pair_delimiter,
                key_value_delimiter,
            } => Self::Kv(KvKeys::new(pair_delimiter, key_value_delimiter, paths)?),
            Format::Custom(format) => Self::Custom(format.resolve(paths)?),
        })
    }

    fn find<'t>(&self, raw: &'t str, values: &mut [Option<Cow<'t, str>>]) -> Result<(), Malformed> {
        match self {
            Self::Json(paths) => paths.find(raw, values),
            Self::Delimited(columns) => columns.find(raw, values),
            Self::Regex(groups) => groups.find(raw, values),
            Self::Kv(keys) => keys.find(raw, values),
            Self::Custom(paths) => paths.find(raw, values),
        }
    }
}

pub struct ParserSpecBuilder {
    format: Format,
    fields: Vec<(String, String)>,
    bodies: Vec<(String, ParserSpec)>,
    empty_as_none: bool,
}

impl ParserSpecBuilder {
    /// Declares an output field read from `path` in the raw record. The path
    /// is checked when the spec is built.
    pub fn field(mut self, name: impl Into<String>, path: impl Into<String>) -> Self {
        self.fields.push((name.into(), path.into()));
        self
    }

    /// Parses the text at `path`, in this spec's path notation, with `spec`
    /// and appends its fields; a body `spec` cannot parse makes the record malformed.
    pub fn body(mut self, path: impl Into<String>, spec: ParserSpec) -> Self {
        self.bodies.push((path.into(), spec));
        self
    }

    /// Leaves empty strings absent instead of returning them as values (#13).
    /// This also covers the text at a body path, while a body spec applies its
    /// own setting to its fields.
    pub fn empty_as_none(mut self) -> Self {
        self.empty_as_none = true;
        self
    }

    pub fn build(self) -> Result<ParserSpec, SpecError> {
        let span =
            telemetry::build_span(|| self.format.name(), self.fields.len(), self.bodies.len());
        span.in_scope(|| self.compile().inspect_err(telemetry::spec_error))
    }

    fn compile(self) -> Result<ParserSpec, SpecError> {
        let mut names: Vec<&str> = self.fields.iter().map(|(name, _)| name.as_str()).collect();
        for (_, spec) in &self.bodies {
            spec.collect_names(&mut names);
        }
        validate_field_names(names.iter().copied())?;
        let field_count = names.len();
        let paths: Vec<String> = self
            .fields
            .iter()
            .map(|(_, path)| path)
            .chain(self.bodies.iter().map(|(path, _)| path))
            .cloned()
            .collect();
        Ok(ParserSpec {
            fields: self.fields,
            paths: Paths::new(self.format.clone(), &paths)?,
            format: self.format,
            bodies: self
                .bodies
                .into_iter()
                .map(|(path, spec)| Body { path, spec })
                .collect(),
            field_count,
            empty_as_none: self.empty_as_none,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::ops::Range;
    use std::panic::{self, RefUnwindSafe, UnwindSafe};
    use std::thread;

    use super::*;

    const ORDER: &str = r#"{
        "order": {"id": "A-1", "amount": 12.50, "paid": true, "memo": "", "coupon": null},
        "items": [{"sku": "B-7", "qty": 2}, {"sku": "C-3", "qty": 1}],
        "note": "line\nbreak",
        "a/b": "slash",
        "m~n": "tilde",
        "~1": "escaped tilde",
        "": "empty key"
    }"#;

    const CSV: Format = Format::Delimited { delimiter: ',' };

    const ACCESS_LOG: &str = r#"203.0.113.7 - - [18/Nov/2026:10:15:32 +0000] "GET /orders?id=A-1 HTTP/1.1" 200 512 "-" "curl/8.5.0""#;

    const COMBINED: &str = r#"^(?P<client>\S+) \S+ (?P<user>\S+) \[(?P<time>[^\]]+)\] "(?P<method>\S+) (?P<path>\S+) (?P<protocol>[^"]+)" (?P<status>\d{3}) (?P<bytes>\d+|-) "(?P<referer>[^"]*)" "(?P<agent>[^"]*)"$"#;

    const LOGFMT: Format = Format::Kv {
        pair_delimiter: ' ',
        key_value_delimiter: '=',
    };

    const APP_LOG: &str = r#"ts=2026-11-18T10:15:32Z level=info msg="order paid" order_id=A-1 amount=12.50 path=/orders?id=A-1"#;

    const APP_LINE: &str = r#"2026-11-18T10:00:00Z app[42]: {"order_id":"A-1","amount":12.5}"#;

    const APP_HEADER: &str = r"^(?P<time>\S+) (?P<app>[^\[]+)\[(?P<pid>\d+)\]:(?: (?P<body>.*))?$";

    const ORDER_LAYOUT: FixedWidth = FixedWidth(&[
        ("order_id", 0..6),
        ("status", 6..14),
        ("amount", 14..22),
        ("items", 22..30),
    ]);

    const FIXED_ORDER: &str = "A-1   paid    12.50   B-7;C-3 ";

    #[derive(Debug)]
    struct FixedWidth(&'static [(&'static str, Range<usize>)]);

    impl Parser for FixedWidth {
        type Path = Range<usize>;

        fn name(&self) -> &str {
            "fixed_width"
        }

        fn path(&self, path: &str) -> Option<Range<usize>> {
            self.0
                .iter()
                .find(|(name, _)| *name == path)
                .map(|(_, columns)| columns.clone())
        }

        fn read<'a>(
            &self,
            raw: &'a str,
            paths: &[Range<usize>],
            values: &mut [Option<Cow<'a, str>>],
        ) -> Result<(), Malformed> {
            let width = self.0.iter().map(|(_, columns)| columns.end).max();
            if Some(raw.len()) != width {
                return Err(Malformed::new("record width differs from the layout"));
            }
            for (columns, value) in paths.iter().zip(values) {
                *value = raw
                    .get(columns.clone())
                    .map(|text| Cow::Borrowed(text.trim_end()));
            }
            Ok(())
        }
    }

    #[derive(Debug)]
    struct PanicsOnEmpty;

    impl Parser for PanicsOnEmpty {
        type Path = ();

        fn name(&self) -> &str {
            "panics_on_empty"
        }

        fn path(&self, _: &str) -> Option<()> {
            Some(())
        }

        fn read<'a>(
            &self,
            raw: &'a str,
            _: &[()],
            values: &mut [Option<Cow<'a, str>>],
        ) -> Result<(), Malformed> {
            assert!(!raw.is_empty(), "empty record");
            values.fill(Some(Cow::Borrowed(raw)));
            Ok(())
        }
    }

    fn regex(pattern: &str) -> Format {
        Format::Regex {
            pattern: pattern.into(),
        }
    }

    fn spec(format: Format, fields: &[(&str, &str)]) -> ParserSpec {
        fields
            .iter()
            .fold(ParserSpec::builder(format), |builder, (name, path)| {
                builder.field(*name, *path)
            })
            .build()
            .expect("valid spec")
    }

    fn fields<'a>(record: &'a FieldRecord<'_>) -> Vec<(&'a str, &'a str)> {
        record.iter().collect()
    }

    #[test]
    fn returns_values_as_text_in_declared_order() {
        let spec = spec(
            Format::Json,
            &[
                ("paid", "/order/paid"),
                ("order_id", "/order/id"),
                ("amount", "/order/amount"),
                ("second_sku", "/items/1/sku"),
            ],
        );
        let record = spec.parse(ORDER).expect("valid record");
        assert_eq!(
            fields(&record),
            [
                ("paid", "true"),
                ("order_id", "A-1"),
                ("amount", "12.50"),
                ("second_sku", "C-3"),
            ]
        );
    }

    #[test]
    fn objects_and_arrays_keep_their_json_text() {
        let spec = spec(Format::Json, &[("first_item", "/items/0"), ("whole", "")]);
        let record = spec.parse(ORDER).expect("valid record");
        assert_eq!(
            record.get("first_item"),
            Some(r#"{"sku": "B-7", "qty": 2}"#)
        );
        assert_eq!(record.get("whole"), Some(ORDER.trim()));
    }

    #[test]
    fn missing_and_null_are_absent_and_empty_string_is_a_value() {
        let spec = spec(
            Format::Json,
            &[
                ("memo", "/order/memo"),
                ("coupon", "/order/coupon"),
                ("status", "/order/status"),
                ("through_scalar", "/order/id/x"),
                ("out_of_range", "/items/2"),
                ("leading_zero", "/items/01"),
                ("signed", "/items/+1"),
            ],
        );
        let record = spec.parse(ORDER).expect("valid record");
        assert_eq!(fields(&record), [("memo", "")]);
    }

    #[test]
    fn empty_as_none_leaves_empty_strings_absent() {
        for (format, memo, order_id, raw) in [
            (Format::Json, "/order/memo", "/order/id", ORDER),
            (CSV, "1", "0", "A-1,,12.50"),
            (CSV, "1", "0", r#"A-1,"",12.50"#),
            (
                regex("^(?P<id>[^,]*),(?P<memo>[^,]*),"),
                "memo",
                "id",
                "A-1,,12.50",
            ),
            (
                LOGFMT,
                "memo",
                "order_id",
                "order_id=A-1 memo= amount=12.50",
            ),
            (
                LOGFMT,
                "memo",
                "order_id",
                r#"order_id=A-1 memo="" amount=12.50"#,
            ),
        ] {
            let spec = ParserSpec::builder(format)
                .field("memo", memo)
                .field("order_id", order_id)
                .empty_as_none()
                .build()
                .expect("valid spec");
            let record = spec.parse(raw).expect("valid record");
            assert_eq!(fields(&record), [("order_id", "A-1")], "{raw:?}");
        }
    }

    #[test]
    fn pointer_escapes_match_keys_with_slash_and_tilde() {
        let spec = spec(
            Format::Json,
            &[
                ("slash", "/a~1b"),
                ("tilde", "/m~0n"),
                ("tilde_one", "/~01"),
                ("empty_key", "/"),
            ],
        );
        let record = spec.parse(ORDER).expect("valid record");
        assert_eq!(
            fields(&record),
            [
                ("slash", "slash"),
                ("tilde", "tilde"),
                ("tilde_one", "escaped tilde"),
                ("empty_key", "empty key"),
            ]
        );
    }

    #[test]
    fn rejects_invalid_pointers() {
        for path in ["order/id", "#/order", "/order~2", "/order~", "/~a"] {
            let result = ParserSpec::builder(Format::Json)
                .field("order_id", path)
                .build();
            assert_eq!(
                result.map(|_| ()),
                Err(SpecError::InvalidPath(path.into())),
                "{path:?}"
            );
        }
    }

    #[test]
    fn unescapes_strings_and_borrows_escape_free_values() {
        let spec = spec(
            Format::Json,
            &[
                ("order_id", "/order/id"),
                ("amount", "/order/amount"),
                ("note", "/note"),
            ],
        );
        let record = spec.parse(ORDER).expect("valid record");
        assert!(matches!(
            record.get_cow("order_id"),
            Some(Cow::Borrowed("A-1"))
        ));
        assert!(matches!(
            record.get_cow("amount"),
            Some(Cow::Borrowed("12.50"))
        ));
        assert!(matches!(
            record.get_cow("note"),
            Some(Cow::Owned(note)) if note == "line\nbreak"
        ));
    }

    #[test]
    fn first_value_wins_for_repeated_keys() {
        let spec = spec(Format::Json, &[("id", "/id")]);
        let record = spec
            .parse(r#"{"id": "A-1", "id": "A-2"}"#)
            .expect("valid record");
        assert_eq!(fields(&record), [("id", "A-1")]);
    }

    #[test]
    fn malformed_unless_input_is_one_json_object() {
        let spec = spec(Format::Json, &[("id", "/id")]);
        for raw in [
            "",
            "{",
            r#"{"id": 1"#,
            r#"{"id": 1} {}"#,
            "[1]",
            r#""id""#,
            "null",
        ] {
            assert!(spec.parse(raw).is_err(), "{raw:?}");
        }
        assert!(
            spec.parse(r#"{"other": [1, {"x": "y"}]}"#)
                .is_ok_and(|r| r.is_empty())
        );
    }

    fn assert_same_records_across_threads(spec: &ParserSpec, inputs: &[String]) {
        let parse_all = || -> Vec<_> {
            inputs
                .iter()
                .map(|raw| spec.parse(raw).map(FieldRecord::into_owned))
                .collect()
        };
        let expected = parse_all();
        let results: Vec<_> = thread::scope(|scope| {
            let handles: Vec<_> = (0..8).map(|_| scope.spawn(parse_all)).collect();
            handles
                .into_iter()
                .map(|handle| handle.join().expect("thread finished"))
                .collect()
        });
        assert!(results.iter().all(|result| *result == expected));
    }

    #[test]
    fn one_spec_gives_the_same_records_across_threads() {
        let json = spec(
            Format::Json,
            &[("order_id", "/order/id"), ("first_sku", "/items/0/sku")],
        );
        let inputs: Vec<String> = (0..64)
            .map(|i| {
                format!(r#"{{"order": {{"id": "A-{i}"}}, "items": [{{"sku": "S\u00{i:02}"}}]}}"#)
            })
            .collect();
        assert_same_records_across_threads(&json, &inputs);

        let delimited = spec(CSV, &[("order_id", "0"), ("memo", "2")]);
        let inputs: Vec<String> = (0..64)
            .map(|i| format!(r#"A-{i},{i},"say ""{i}""""#))
            .collect();
        assert_same_records_across_threads(&delimited, &inputs);

        let access_log = spec(
            regex(COMBINED),
            &[("path", "path"), ("status", "status"), ("user", "user")],
        );
        let inputs: Vec<String> = (0..64)
            .map(|i| ACCESS_LOG.replace("A-1", &format!("A-{i}")))
            .collect();
        assert_same_records_across_threads(&access_log, &inputs);

        let app_log = spec(LOGFMT, &[("order_id", "order_id"), ("msg", "msg")]);
        let inputs: Vec<String> = (0..64)
            .map(|i| format!(r#"level=info msg="say \"{i}\"" order_id=A-{i}"#))
            .collect();
        assert_same_records_across_threads(&app_log, &inputs);

        let composed = ParserSpec::builder(regex(APP_HEADER))
            .field("pid", "pid")
            .body(
                "body",
                spec(
                    Format::Json,
                    &[("order_id", "/order_id"), ("note", "/note")],
                ),
            )
            .build()
            .expect("valid spec");
        let inputs: Vec<String> = (0..64)
            .map(|i| {
                format!(
                    r#"2026-11-18T10:00:00Z app[{i}]: {{"order_id":"A-{i}","note":"say \"{i}\""}}"#
                )
            })
            .collect();
        assert_same_records_across_threads(&composed, &inputs);

        let custom = ParserSpec::builder(Format::custom(ORDER_LAYOUT))
            .field("order_id", "order_id")
            .field("amount", "amount")
            .body(
                "items",
                spec(Format::Delimited { delimiter: ';' }, &[("sku", "1")]),
            )
            .build()
            .expect("valid spec");
        let inputs: Vec<String> = (0..64)
            .map(|i| format!("A-{i:<4}paid    {i:<8}B-7;C-{i:<2}"))
            .collect();
        assert_same_records_across_threads(&custom, &inputs);
    }

    #[test]
    fn kv_selects_values_by_key_in_declared_order() {
        let spec = spec(
            LOGFMT,
            &[
                ("amount", "amount"),
                ("order_id", "order_id"),
                ("reference", "order_id"),
                ("msg", "msg"),
                ("path", "path"),
                ("status", "status"),
            ],
        );
        for raw in [
            APP_LOG.to_owned(),
            format!("{APP_LOG}\n"),
            format!("{APP_LOG}\r\n"),
            format!("  {}  ", APP_LOG.replace(" order_id", "   order_id")),
        ] {
            let record = spec.parse(&raw).expect("valid record");
            assert_eq!(
                fields(&record),
                [
                    ("amount", "12.50"),
                    ("order_id", "A-1"),
                    ("reference", "A-1"),
                    ("msg", "order paid"),
                    ("path", "/orders?id=A-1"),
                ],
                "{raw:?}"
            );
            assert!(matches!(
                record.get_cow("msg"),
                Some(Cow::Borrowed("order paid"))
            ));
        }
    }

    #[test]
    fn kv_quoted_values_keep_delimiters_and_borrow_unless_escaped() {
        let spec = spec(
            LOGFMT,
            &[
                ("note", "note"),
                ("msg", "msg"),
                ("file", "file"),
                ("customer", "customer"),
                ("label", "label"),
            ],
        );
        let record = spec
            .parse(
                r#"note="a=b c" msg="say \"hi\"\tthen\nleave" file="C:\\orders\/a.csv" customer="Jos\u00e9 \ud83d\ude00" label=5"ruler"#,
            )
            .expect("valid record");
        assert!(matches!(
            record.get_cow("note"),
            Some(Cow::Borrowed("a=b c"))
        ));
        assert!(matches!(
            record.get_cow("msg"),
            Some(Cow::Owned(msg)) if msg == "say \"hi\"\tthen\nleave"
        ));
        assert!(matches!(
            record.get_cow("file"),
            Some(Cow::Owned(file)) if file == r"C:\orders/a.csv"
        ));
        assert!(matches!(
            record.get_cow("customer"),
            Some(Cow::Owned(customer)) if customer == "Jos\u{e9} \u{1f600}"
        ));
        assert!(matches!(
            record.get_cow("label"),
            Some(Cow::Borrowed(r#"5"ruler"#))
        ));
    }

    #[test]
    fn kv_key_without_delimiter_is_absent_and_empty_value_is_a_value() {
        let spec = spec(
            LOGFMT,
            &[
                ("retry", "retry"),
                ("order_id", "order_id"),
                ("memo", "memo"),
                ("coupon", "coupon"),
            ],
        );
        let record = spec
            .parse(r#"retry order_id=A-1 memo= coupon="""#)
            .expect("valid record");
        assert_eq!(
            fields(&record),
            [("order_id", "A-1"), ("memo", ""), ("coupon", "")]
        );

        for raw in ["", "\n", "   "] {
            assert!(spec.parse(raw).is_ok_and(|r| r.is_empty()), "{raw:?}");
        }
    }

    #[test]
    fn kv_first_pair_wins_for_repeated_keys() {
        let spec = spec(LOGFMT, &[("status", "status"), ("retry", "retry")]);
        let record = spec
            .parse("status=pending retry status=paid retry=2")
            .expect("valid record");
        assert_eq!(fields(&record), [("status", "pending")]);
    }

    #[test]
    fn kv_delimiters_are_configurable() {
        let semicolon = spec(
            Format::Kv {
                pair_delimiter: ';',
                key_value_delimiter: ':',
            },
            &[
                ("order_id", "order_id"),
                ("note", "note"),
                ("amount", "amount"),
            ],
        );
        let record = semicolon
            .parse(r#"order_id:A-1;note:"paid; in full";amount:12.50"#)
            .expect("valid record");
        assert_eq!(
            fields(&record),
            [
                ("order_id", "A-1"),
                ("note", "paid; in full"),
                ("amount", "12.50"),
            ]
        );

        let query = spec(
            Format::Kv {
                pair_delimiter: '&',
                key_value_delimiter: '=',
            },
            &[("order_id", "id"), ("page", "page")],
        );
        let record = query.parse("id=A-1&page=2").expect("valid record");
        assert_eq!(fields(&record), [("order_id", "A-1"), ("page", "2")]);
    }

    #[test]
    fn kv_rejects_quote_line_break_and_equal_delimiters() {
        for (pair_delimiter, key_value_delimiter, name, value) in [
            ('"', '=', "pair_delimiter", '"'),
            ('\n', '=', "pair_delimiter", '\n'),
            ('\r', '=', "pair_delimiter", '\r'),
            (' ', '"', "key_value_delimiter", '"'),
            (' ', '\n', "key_value_delimiter", '\n'),
            (' ', '\r', "key_value_delimiter", '\r'),
            ('=', '=', "key_value_delimiter", '='),
        ] {
            let result = ParserSpec::builder(Format::Kv {
                pair_delimiter,
                key_value_delimiter,
            })
            .field("order_id", "order_id")
            .build();
            assert_eq!(
                result.map(|_| ()),
                Err(SpecError::InvalidArgument {
                    name: name.into(),
                    value: value.into(),
                }),
                "{pair_delimiter:?} {key_value_delimiter:?}"
            );
        }
    }

    #[test]
    fn kv_rejects_keys_that_contain_a_delimiter() {
        for path in ["", "order id", "order=id"] {
            let result = ParserSpec::builder(LOGFMT).field("order_id", path).build();
            assert_eq!(
                result.map(|_| ()),
                Err(SpecError::InvalidPath(path.into())),
                "{path:?}"
            );
        }
    }

    #[test]
    fn kv_malformed_when_a_key_quote_or_escape_is_broken_in_any_pair() {
        let spec = spec(LOGFMT, &[("order_id", "order_id")]);
        for raw in [
            "order_id=A-1 =paid",
            r#"order_id=A-1 msg="open"#,
            r#"order_id=A-1 msg="done"x"#,
            r#"order_id=A-1 msg="trailing\"#,
            r#"order_id=A-1 msg="bad \q""#,
            r#"order_id=A-1 msg="\u12""#,
            r#"order_id=A-1 msg="\u+123""#,
            r#"order_id=A-1 msg="\ud83d""#,
            r#"order_id=A-1 msg="\ude00\ude00""#,
        ] {
            assert!(spec.parse(raw).is_err(), "{raw:?}");
        }
    }

    #[test]
    fn regex_selects_named_groups_in_declared_order() {
        let spec = spec(
            regex(COMBINED),
            &[
                ("status", "status"),
                ("client", "client"),
                ("path", "path"),
                ("request_path", "path"),
                ("time", "time"),
                ("referer", "referer"),
            ],
        );
        for raw in [
            ACCESS_LOG.to_owned(),
            format!("{ACCESS_LOG}\n"),
            format!("{ACCESS_LOG}\r\n"),
        ] {
            let record = spec.parse(&raw).expect("valid record");
            assert_eq!(
                fields(&record),
                [
                    ("status", "200"),
                    ("client", "203.0.113.7"),
                    ("path", "/orders?id=A-1"),
                    ("request_path", "/orders?id=A-1"),
                    ("time", "18/Nov/2026:10:15:32 +0000"),
                    ("referer", "-"),
                ],
                "{raw:?}"
            );
            assert!(matches!(
                record.get_cow("path"),
                Some(Cow::Borrowed("/orders?id=A-1"))
            ));
        }
    }

    #[test]
    fn regex_group_outside_the_match_is_absent_and_empty_group_is_a_value() {
        let spec = spec(
            regex(r"^(?P<method>[A-Z]+) (?P<path>[^?\s]*)(?:\?(?P<query>\S*))?$"),
            &[("path", "path"), ("query", "query")],
        );
        let record = spec.parse("GET /orders").expect("valid record");
        assert_eq!(fields(&record), [("path", "/orders")]);

        let record = spec.parse("GET /orders?").expect("valid record");
        assert_eq!(fields(&record), [("path", "/orders"), ("query", "")]);
    }

    #[test]
    fn regex_matches_anywhere_unless_anchored() {
        let unanchored = spec(regex(r"(?P<status>\d{3})"), &[("status", "status")]);
        let record = unanchored.parse(ACCESS_LOG).expect("valid record");
        assert_eq!(fields(&record), [("status", "203")]);

        let anchored = spec(regex(r"^(?P<status>\d{3})$"), &[("status", "status")]);
        assert!(anchored.parse(ACCESS_LOG).is_err());
        assert!(anchored.parse("200\n").is_ok());
    }

    #[test]
    fn rejects_invalid_patterns() {
        for pattern in ["(", "[a-", r"\p{Unknown}", "(?P<id>a)(?P<id>b)"] {
            let result = ParserSpec::builder(regex(pattern))
                .field("order_id", "id")
                .build();
            assert_eq!(
                result.map(|_| ()),
                Err(SpecError::InvalidArgument {
                    name: "pattern".into(),
                    value: pattern.into(),
                }),
                "{pattern:?}"
            );
        }
    }

    #[test]
    fn rejects_paths_that_are_not_group_names() {
        for path in ["", "0", "1", "status ", "missing"] {
            let result = ParserSpec::builder(regex(r"^(?P<status>\d{3}) (\S+)$"))
                .field("status", path)
                .build();
            assert_eq!(
                result.map(|_| ()),
                Err(SpecError::InvalidPath(path.into())),
                "{path:?}"
            );
        }
    }

    #[test]
    fn delimited_selects_columns_by_position_in_declared_order() {
        let spec = spec(
            CSV,
            &[
                ("amount", "2"),
                ("order_id", "0"),
                ("reference", "0"),
                ("memo", "3"),
                ("coupon", "4"),
            ],
        );
        for raw in [
            "A-1,paid,12.50,,",
            "A-1,paid,12.50,,\n",
            "A-1,paid,12.50,,\r\n",
        ] {
            let record = spec.parse(raw).expect("valid record");
            assert_eq!(
                fields(&record),
                [
                    ("amount", "12.50"),
                    ("order_id", "A-1"),
                    ("reference", "A-1"),
                    ("memo", ""),
                    ("coupon", ""),
                ],
                "{raw:?}"
            );
        }
    }

    #[test]
    fn delimited_missing_column_is_absent_and_empty_column_is_a_value() {
        let spec = spec(CSV, &[("order_id", "0"), ("memo", "1"), ("coupon", "2")]);
        let record = spec.parse("A-1,").expect("valid record");
        assert_eq!(fields(&record), [("order_id", "A-1"), ("memo", "")]);

        let record = spec.parse("").expect("valid record");
        assert_eq!(fields(&record), [("order_id", "")]);
    }

    #[test]
    fn wide_records_keep_every_field_in_declared_order() {
        let names: Vec<String> = (0..40).rev().map(|i| format!("c{i}")).collect();
        let indices: Vec<String> = (0..40).rev().map(|i: usize| i.to_string()).collect();
        let expected: Vec<(&str, &str)> = names.iter().map(|n| (n.as_str(), n.as_str())).collect();
        let columns: Vec<(&str, &str)> = names
            .iter()
            .zip(&indices)
            .map(|(name, index)| (name.as_str(), index.as_str()))
            .collect();

        let raw = (0..40)
            .map(|i| format!("c{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let record_spec = spec(CSV, &columns);
        let record = record_spec.parse(&raw).expect("valid record");
        assert_eq!(fields(&record), expected);

        let raw = (0..40)
            .map(|i| format!("c{i}=c{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let record_spec = spec(LOGFMT, &expected);
        let record = record_spec.parse(&raw).expect("valid record");
        assert_eq!(fields(&record), expected);
    }

    #[test]
    fn quoted_columns_keep_delimiters_and_borrow_unless_quotes_are_doubled() {
        let spec = spec(
            CSV,
            &[
                ("order_id", "0"),
                ("memo", "1"),
                ("note", "2"),
                ("label", "3"),
            ],
        );
        let record = spec
            .parse(r#""A-1","paid, in full","say ""hi""",5" ruler"#)
            .expect("valid record");
        assert!(matches!(
            record.get_cow("order_id"),
            Some(Cow::Borrowed("A-1"))
        ));
        assert!(matches!(
            record.get_cow("memo"),
            Some(Cow::Borrowed("paid, in full"))
        ));
        assert!(matches!(
            record.get_cow("note"),
            Some(Cow::Owned(note)) if note == r#"say "hi""#
        ));
        assert!(matches!(
            record.get_cow("label"),
            Some(Cow::Borrowed(r#"5" ruler"#))
        ));
    }

    #[test]
    fn delimiter_is_configurable() {
        let line = r#"127.0.0.1 - "GET /orders?id=A-1 HTTP/1.1" 200"#;
        let space = spec(
            Format::Delimited { delimiter: ' ' },
            &[("client", "0"), ("request", "2"), ("status", "3")],
        );
        let record = space.parse(line).expect("valid record");
        assert_eq!(
            fields(&record),
            [
                ("client", "127.0.0.1"),
                ("request", "GET /orders?id=A-1 HTTP/1.1"),
                ("status", "200"),
            ]
        );

        let tab = spec(
            Format::Delimited { delimiter: '\t' },
            &[("order_id", "0"), ("memo", "1")],
        );
        let record = tab.parse("A-1\tpaid, in full").expect("valid record");
        assert_eq!(
            fields(&record),
            [("order_id", "A-1"), ("memo", "paid, in full")]
        );
    }

    #[test]
    fn rejects_quote_and_line_break_delimiters() {
        for delimiter in ['"', '\n', '\r'] {
            let result = ParserSpec::builder(Format::Delimited { delimiter })
                .field("order_id", "0")
                .build();
            assert_eq!(
                result.map(|_| ()),
                Err(SpecError::InvalidArgument {
                    name: "delimiter".into(),
                    value: delimiter.into(),
                }),
                "{delimiter:?}"
            );
        }
    }

    #[test]
    fn malformed_when_quoting_is_broken_in_any_column() {
        let spec = spec(CSV, &[("order_id", "0")]);
        for raw in [
            r#""A-1"#,
            r#""A-1"x,paid"#,
            r#"A-1,"paid"#,
            r#"A-1,"paid" ,12.50"#,
        ] {
            assert!(spec.parse(raw).is_err(), "{raw:?}");
        }
    }

    #[test]
    fn rejects_invalid_columns() {
        for column in [
            "",
            "-1",
            "+1",
            "01",
            "a",
            "/0",
            " 1",
            "99999999999999999999999",
        ] {
            let result = ParserSpec::builder(CSV).field("order_id", column).build();
            assert_eq!(
                result.map(|_| ()),
                Err(SpecError::InvalidPath(column.into())),
                "{column:?}"
            );
        }
    }

    #[test]
    fn compose_appends_body_fields_after_header_fields() {
        let body = spec(
            Format::Json,
            &[("order_id", "/order_id"), ("amount", "/amount")],
        );
        let spec = ParserSpec::builder(regex(APP_HEADER))
            .field("time", "time")
            .field("app", "app")
            .field("message", "body")
            .body("body", body)
            .build()
            .expect("valid spec");
        for raw in [APP_LINE.to_owned(), format!("{APP_LINE}\n")] {
            let record = spec.parse(&raw).expect("valid record");
            assert_eq!(
                fields(&record),
                [
                    ("time", "2026-11-18T10:00:00Z"),
                    ("app", "app"),
                    ("message", r#"{"order_id":"A-1","amount":12.5}"#),
                    ("order_id", "A-1"),
                    ("amount", "12.5"),
                ],
                "{raw:?}"
            );
            assert!(matches!(
                record.get_cow("order_id"),
                Some(Cow::Borrowed("A-1"))
            ));
        }
    }

    #[test]
    fn compose_nests_bodies_and_owns_values_read_from_unescaped_text() {
        let items = spec(
            Format::Delimited { delimiter: ';' },
            &[("first_sku", "0"), ("second_sku", "1")],
        );
        let message = ParserSpec::builder(LOGFMT)
            .field("order_id", "order_id")
            .body("items", items)
            .build()
            .expect("valid spec");

        let regex_header = ParserSpec::builder(regex(APP_HEADER))
            .field("time", "time")
            .body("body", message.clone())
            .build()
            .expect("valid spec");
        let record = regex_header
            .parse(r#"2026-11-18T10:00:00Z app[42]: order_id=A-1 items="B-7;C-3""#)
            .expect("valid record");
        let expected = [
            ("time", "2026-11-18T10:00:00Z"),
            ("order_id", "A-1"),
            ("first_sku", "B-7"),
            ("second_sku", "C-3"),
        ];
        assert_eq!(fields(&record), expected);
        assert!(matches!(
            record.get_cow("second_sku"),
            Some(Cow::Borrowed("C-3"))
        ));

        let json_header = ParserSpec::builder(Format::Json)
            .field("time", "/time")
            .body("/message", message)
            .build()
            .expect("valid spec");
        let record = json_header
            .parse(
                r#"{"time": "2026-11-18T10:00:00Z", "message": "order_id=A-1 items=\"B-7;C-3\""}"#,
            )
            .expect("valid record");
        assert_eq!(fields(&record), expected);
        assert!(matches!(
            record.get_cow("time"),
            Some(Cow::Borrowed("2026-11-18T10:00:00Z"))
        ));
        assert!(matches!(
            record.get_cow("order_id"),
            Some(Cow::Owned(order_id)) if order_id == "A-1"
        ));
        assert!(matches!(
            record.get_cow("second_sku"),
            Some(Cow::Owned(sku)) if sku == "C-3"
        ));
    }

    #[test]
    fn compose_reads_one_body_per_call() {
        let spec = ParserSpec::builder(CSV)
            .field("order_id", "0")
            .body("1", spec(Format::Json, &[("amount", "/amount")]))
            .body(
                "2",
                spec(Format::Delimited { delimiter: ';' }, &[("first_sku", "0")]),
            )
            .build()
            .expect("valid spec");
        let record = spec
            .parse(r#"A-1,"{""amount"": 12.5}",B-7;C-3"#)
            .expect("valid record");
        assert_eq!(
            fields(&record),
            [
                ("order_id", "A-1"),
                ("amount", "12.5"),
                ("first_sku", "B-7")
            ]
        );
    }

    #[test]
    fn compose_body_without_text_leaves_body_fields_absent() {
        let body = || {
            spec(
                Format::Json,
                &[("order_id", "/order_id"), ("memo", "/memo")],
            )
        };
        let strict = ParserSpec::builder(regex(APP_HEADER))
            .field("pid", "pid")
            .body("body", body())
            .build()
            .expect("valid spec");
        let lenient = ParserSpec::builder(regex(APP_HEADER))
            .field("pid", "pid")
            .body("body", body())
            .empty_as_none()
            .build()
            .expect("valid spec");

        for spec in [&strict, &lenient] {
            let record = spec
                .parse("2026-11-18T10:00:00Z app[42]:")
                .expect("valid record");
            assert_eq!(fields(&record), [("pid", "42")]);
        }

        let empty_body = "2026-11-18T10:00:00Z app[42]: ";
        assert!(strict.parse(empty_body).is_err());
        let record = lenient.parse(empty_body).expect("valid record");
        assert_eq!(fields(&record), [("pid", "42")]);

        let record = lenient
            .parse(r#"2026-11-18T10:00:00Z app[42]: {"order_id":"A-1","memo":""}"#)
            .expect("valid record");
        assert_eq!(
            fields(&record),
            [("pid", "42"), ("order_id", "A-1"), ("memo", "")]
        );
    }

    #[test]
    fn compose_malformed_when_header_or_body_does_not_match() {
        let spec = ParserSpec::builder(regex(APP_HEADER))
            .field("pid", "pid")
            .body("body", spec(Format::Json, &[("order_id", "/order_id")]))
            .build()
            .expect("valid spec");
        assert!(spec.parse(r#"app: {"order_id":"A-1"}"#).is_err());
        for raw in [
            r#"2026-11-18T10:00:00Z app[42]: {"order_id":"#,
            "2026-11-18T10:00:00Z app[42]: order_id=A-1",
        ] {
            let err = spec.parse(raw).expect_err("malformed body");
            assert!(err.reason().starts_with("body `body`: "), "{raw:?}");
        }
    }

    #[test]
    fn compose_rejects_names_repeated_across_header_and_bodies() {
        let items = spec(Format::Delimited { delimiter: ';' }, &[("sku", "0")]);
        let message = ParserSpec::builder(LOGFMT)
            .field("order_id", "order_id")
            .body("items", items)
            .build()
            .expect("valid spec");
        for repeated in ["order_id", "sku"] {
            let result = ParserSpec::builder(regex(APP_HEADER))
                .field(repeated, "pid")
                .body("body", message.clone())
                .build();
            assert_eq!(
                result.map(|_| ()),
                Err(SpecError::DuplicateField(repeated.into())),
                "{repeated:?}"
            );
        }
    }

    #[test]
    fn compose_rejects_body_paths_invalid_for_the_header_format() {
        let body = spec(Format::Json, &[("order_id", "/order_id")]);
        for (format, path) in [
            (regex(APP_HEADER), "payload"),
            (Format::Json, "message"),
            (CSV, "a"),
            (LOGFMT, ""),
            (Format::custom(ORDER_LAYOUT), "message"),
        ] {
            let result = ParserSpec::builder(format).body(path, body.clone()).build();
            assert_eq!(
                result.map(|_| ()),
                Err(SpecError::InvalidPath(path.into())),
                "{path:?}"
            );
        }
    }

    #[test]
    fn custom_reads_records_without_their_trailing_line_break() {
        let spec = spec(
            Format::custom(ORDER_LAYOUT),
            &[("order_id", "order_id"), ("reference", "order_id")],
        );
        for raw in [format!("{FIXED_ORDER}\n"), format!("{FIXED_ORDER}\r\n")] {
            let record = spec.parse(&raw).expect("valid record");
            assert_eq!(
                fields(&record),
                [("order_id", "A-1"), ("reference", "A-1")],
                "{raw:?}"
            );
        }
    }

    #[test]
    fn custom_empty_as_none_leaves_blank_columns_absent() {
        let raw = FIXED_ORDER.replace("paid", "    ");
        let strict = spec(
            Format::custom(ORDER_LAYOUT),
            &[("order_id", "order_id"), ("status", "status")],
        );
        let record = strict.parse(&raw).expect("valid record");
        assert_eq!(fields(&record), [("order_id", "A-1"), ("status", "")]);

        let lenient = ParserSpec::builder(Format::custom(ORDER_LAYOUT))
            .field("order_id", "order_id")
            .field("status", "status")
            .empty_as_none()
            .build()
            .expect("valid spec");
        let record = lenient.parse(&raw).expect("valid record");
        assert_eq!(fields(&record), [("order_id", "A-1")]);
    }

    #[test]
    fn custom_formats_are_equal_only_when_they_share_the_parser() {
        let format = Format::custom(ORDER_LAYOUT);
        assert_eq!(format.clone(), format);
        assert_ne!(Format::custom(ORDER_LAYOUT), format);
    }

    fn app_line_spec(header: Format, body: ParserSpec) -> ParserSpecBuilder {
        ParserSpec::builder(header)
            .field("time", "time")
            .field("app", "app")
            .body("body", body)
    }

    fn order_body() -> ParserSpec {
        ParserSpec::builder(Format::Json)
            .field("order_id", "/order_id")
            .body("/items", spec(CSV, &[("first_item", "0")]))
            .build()
            .expect("valid spec")
    }

    #[test]
    fn specs_built_by_the_same_calls_are_equal() {
        let build = || {
            app_line_spec(regex(APP_HEADER), order_body())
                .empty_as_none()
                .build()
                .expect("valid spec")
        };
        assert_eq!(build(), build());
        assert_eq!(build().clone(), build());
        let kv = || spec(LOGFMT, &[("order_id", "order_id"), ("amount", "amount")]);
        assert_eq!(kv(), kv());
    }

    #[test]
    fn specs_differ_in_any_declared_input() {
        let base = spec(CSV, &[("order_id", "0"), ("amount", "1")]);
        let others = [
            spec(
                Format::Delimited { delimiter: ';' },
                &[("order_id", "0"), ("amount", "1")],
            ),
            spec(CSV, &[("amount", "1"), ("order_id", "0")]),
            spec(CSV, &[("order_id", "0"), ("total", "1")]),
            spec(CSV, &[("order_id", "0"), ("amount", "2")]),
            spec(CSV, &[("order_id", "0")]),
            ParserSpec::builder(CSV)
                .field("order_id", "0")
                .field("amount", "1")
                .empty_as_none()
                .build()
                .expect("valid spec"),
            ParserSpec::builder(CSV)
                .field("order_id", "0")
                .body("1", spec(Format::Json, &[("amount", "/amount")]))
                .build()
                .expect("valid spec"),
        ];
        for other in &others {
            assert_ne!(&base, other);
        }
    }

    #[test]
    fn regex_specs_compare_patterns_as_written() {
        let digits = spec(regex(r"^(?P<id>\d+)$"), &[("id", "id")]);
        let range = spec(regex(r"^(?P<id>[0-9]+)$"), &[("id", "id")]);
        assert_eq!(digits, spec(regex(r"^(?P<id>\d+)$"), &[("id", "id")]));
        assert_ne!(digits, range);
    }

    #[test]
    fn bodies_are_compared_recursively() {
        let base = app_line_spec(regex(APP_HEADER), order_body())
            .build()
            .expect("valid spec");
        let other_body_path = ParserSpec::builder(regex(APP_HEADER))
            .field("time", "time")
            .field("app", "app")
            .body("pid", order_body())
            .build()
            .expect("valid spec");
        let nested_empty_as_none = ParserSpec::builder(Format::Json)
            .field("order_id", "/order_id")
            .body(
                "/items",
                ParserSpec::builder(CSV)
                    .field("first_item", "0")
                    .empty_as_none()
                    .build()
                    .expect("valid spec"),
            )
            .build()
            .expect("valid spec");
        let other_nested = app_line_spec(regex(APP_HEADER), nested_empty_as_none)
            .build()
            .expect("valid spec");
        assert_ne!(base, other_body_path);
        assert_ne!(base, other_nested);
    }

    #[test]
    fn custom_specs_are_equal_only_when_they_share_the_parser() {
        let format = Format::custom(ORDER_LAYOUT);
        let fields = [("order_id", "order_id"), ("status", "status")];
        assert_eq!(spec(format.clone(), &fields), spec(format.clone(), &fields));
        assert_ne!(
            spec(format.clone(), &fields),
            spec(Format::custom(ORDER_LAYOUT), &fields)
        );
        let header = |body: Format| {
            app_line_spec(regex(APP_HEADER), spec(body, &fields))
                .build()
                .expect("valid spec")
        };
        assert_eq!(header(format.clone()), header(format));
        assert_ne!(
            header(Format::custom(ORDER_LAYOUT)),
            header(Format::custom(ORDER_LAYOUT))
        );
    }

    #[test]
    fn compose_reads_custom_header_and_custom_body() {
        let header = ParserSpec::builder(Format::custom(ORDER_LAYOUT))
            .field("order_id", "order_id")
            .body(
                "items",
                spec(
                    Format::Delimited { delimiter: ';' },
                    &[("first_sku", "0"), ("second_sku", "1")],
                ),
            )
            .build()
            .expect("valid spec");
        let record = header.parse(FIXED_ORDER).expect("valid record");
        assert_eq!(
            fields(&record),
            [
                ("order_id", "A-1"),
                ("first_sku", "B-7"),
                ("second_sku", "C-3")
            ]
        );
        assert!(matches!(
            record.get_cow("second_sku"),
            Some(Cow::Borrowed("C-3"))
        ));

        let body = ParserSpec::builder(Format::Json)
            .field("time", "/time")
            .body(
                "/record",
                spec(
                    Format::custom(ORDER_LAYOUT),
                    &[("order_id", "order_id"), ("status", "status")],
                ),
            )
            .build()
            .expect("valid spec");
        let raw = format!(r#"{{"time": "2026-11-18T10:00:00Z", "record": "{FIXED_ORDER}"}}"#);
        let record = body.parse(&raw).expect("valid record");
        assert_eq!(
            fields(&record),
            [
                ("time", "2026-11-18T10:00:00Z"),
                ("order_id", "A-1"),
                ("status", "paid")
            ]
        );
        assert!(matches!(
            record.get_cow("status"),
            Some(Cow::Borrowed("paid"))
        ));

        let escaped = raw.replace("paid", r"\u0070aid");
        let record = body.parse(&escaped).expect("valid record");
        assert!(matches!(
            record.get_cow("status"),
            Some(Cow::Owned(status)) if status == "paid"
        ));

        let err = body
            .parse(r#"{"record": "A-1   paid"}"#)
            .expect_err("malformed body");
        assert!(err.reason().starts_with("body `/record`: "));
    }

    #[test]
    fn custom_parser_panic_reaches_the_caller_and_leaves_the_spec_usable() {
        let spec = spec(Format::custom(PanicsOnEmpty), &[("order_id", "any")]);
        assert!(panic::catch_unwind(|| spec.parse("").map(FieldRecord::into_owned)).is_err());

        let record = spec.parse("A-1\n").expect("valid record");
        assert_eq!(fields(&record), [("order_id", "A-1")]);
    }

    #[test]
    fn public_types_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ParserSpec>();
        assert_send_sync::<ParserSpecBuilder>();
        assert_send_sync::<Format>();
    }

    #[test]
    fn spec_stays_unwind_safe_with_custom_parsers() {
        fn assert_unwind_safe<T: UnwindSafe + RefUnwindSafe>() {}
        assert_unwind_safe::<ParserSpec>();
        assert_unwind_safe::<Format>();
    }
}
