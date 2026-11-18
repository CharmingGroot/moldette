use std::borrow::Cow;

use moldette_core::{FieldRecord, Malformed, SpecError, validate_field_names};

use crate::delimited::DelimitedColumns;
use crate::json::{self, JsonPaths};
use crate::kv::KvKeys;
use crate::regex::RegexGroups;

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
}

/// A validated description of how to parse raw records. Immutable once
/// built.
#[derive(Debug, Clone)]
pub struct ParserSpec {
    names: Vec<String>,
    paths: Paths,
    empty_as_none: bool,
}

#[derive(Debug, Clone)]
enum Paths {
    Json(JsonPaths),
    Delimited(DelimitedColumns),
    Regex(RegexGroups),
    Kv(KvKeys),
}

impl ParserSpec {
    pub fn builder(format: Format) -> ParserSpecBuilder {
        ParserSpecBuilder {
            format,
            fields: Vec::new(),
            empty_as_none: false,
        }
    }

    /// Parses one raw record. Values borrow from `raw` where possible, and a
    /// missing or null value is left absent.
    pub fn parse<'a>(&'a self, raw: &'a str) -> Result<FieldRecord<'a>, Malformed> {
        let mut record = FieldRecord::with_capacity(self.names.len());
        match &self.paths {
            Paths::Json(paths) => {
                for (name, text) in self.names.iter().zip(paths.find(raw)?) {
                    let value = text.map(json::field_value).transpose()?.flatten();
                    self.insert(&mut record, name, value);
                }
            }
            Paths::Delimited(columns) => self.insert_all(&mut record, columns.find(raw)?),
            Paths::Regex(groups) => self.insert_all(&mut record, groups.find(raw)?),
            Paths::Kv(keys) => self.insert_all(&mut record, keys.find(raw)?),
        }
        Ok(record)
    }

    fn insert_all<'a>(&'a self, record: &mut FieldRecord<'a>, values: Vec<Option<Cow<'a, str>>>) {
        for (name, value) in self.names.iter().zip(values) {
            self.insert(record, name, value);
        }
    }

    fn insert<'a>(&self, record: &mut FieldRecord<'a>, name: &'a str, value: Option<Cow<'a, str>>) {
        if let Some(value) = value.filter(|value| !(self.empty_as_none && value.is_empty())) {
            record.insert(name, value);
        }
    }
}

pub struct ParserSpecBuilder {
    format: Format,
    fields: Vec<(String, String)>,
    empty_as_none: bool,
}

impl ParserSpecBuilder {
    /// Declares an output field read from `path` in the raw record. The path
    /// is checked when the spec is built.
    pub fn field(mut self, name: impl Into<String>, path: impl Into<String>) -> Self {
        self.fields.push((name.into(), path.into()));
        self
    }

    /// Leaves empty strings absent instead of returning them as values (#13).
    pub fn empty_as_none(mut self) -> Self {
        self.empty_as_none = true;
        self
    }

    pub fn build(self) -> Result<ParserSpec, SpecError> {
        validate_field_names(self.fields.iter().map(|(name, _)| name.as_str()))?;
        let (names, paths): (_, Vec<String>) = self.fields.into_iter().unzip();
        let paths = match self.format {
            Format::Json => Paths::Json(JsonPaths::new(&paths)?),
            Format::Delimited { delimiter } => {
                Paths::Delimited(DelimitedColumns::new(delimiter, &paths)?)
            }
            Format::Regex { pattern } => Paths::Regex(RegexGroups::new(&pattern, &paths)?),
            Format::Kv {
                pair_delimiter,
                key_value_delimiter,
            } => Paths::Kv(KvKeys::new(pair_delimiter, key_value_delimiter, &paths)?),
        };
        Ok(ParserSpec {
            names,
            paths,
            empty_as_none: self.empty_as_none,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
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

    #[test]
    fn rejects_empty_and_duplicate_field_names() {
        let duplicate = ParserSpec::builder(Format::Json)
            .field("id", "/order/id")
            .field("id", "/id")
            .build();
        assert_eq!(
            duplicate.map(|_| ()),
            Err(SpecError::DuplicateField("id".into()))
        );

        let empty = ParserSpec::builder(Format::Json).field("", "/id").build();
        assert_eq!(empty.map(|_| ()), Err(SpecError::EmptyFieldName));
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
    fn malformed_when_record_does_not_match() {
        let spec = spec(regex(COMBINED), &[("status", "status")]);
        let unknown_status = ACCESS_LOG.replace(" 200 ", " OK ");
        for raw in ["", "\n", "A-1,paid,12.50", unknown_status.as_str()] {
            assert!(spec.parse(raw).is_err(), "{raw:?}");
        }
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
    fn public_types_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ParserSpec>();
        assert_send_sync::<ParserSpecBuilder>();
    }
}
