use std::borrow::Cow;

use moldette_core::{FieldRecord, Malformed, SpecError, validate_field_names};

use crate::delimited::DelimitedColumns;
use crate::json::{self, JsonPaths};

/// The format of the raw records a spec parses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
            Paths::Delimited(columns) => {
                for (name, value) in self.names.iter().zip(columns.find(raw)?) {
                    self.insert(&mut record, name, value);
                }
            }
        }
        Ok(record)
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
