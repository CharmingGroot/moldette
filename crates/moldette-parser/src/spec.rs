use moldette_core::{FieldRecord, Malformed, SpecError, validate_field_names};

use crate::json::{self, JsonPaths};

/// The format of the raw records a spec parses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Format {
    /// One JSON object per record. A field path is a JSON Pointer (RFC 6901),
    /// with `""` for the whole record. Strings are unescaped and other
    /// non-null values keep their JSON text.
    Json,
}

/// A validated description of how to parse raw records. Immutable once
/// built.
#[derive(Debug, Clone)]
pub struct ParserSpec {
    names: Vec<String>,
    paths: JsonPaths,
    empty_as_none: bool,
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
        let texts = self.paths.find(raw)?;
        let mut record = FieldRecord::with_capacity(self.names.len());
        for (name, text) in self.names.iter().zip(texts) {
            let Some(value) = text.map(json::field_value).transpose()?.flatten() else {
                continue;
            };
            if !(self.empty_as_none && value.is_empty()) {
                record.insert(name.as_str(), value);
            }
        }
        Ok(record)
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
            Format::Json => JsonPaths::new(&paths)?,
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

    fn spec(fields: &[(&str, &str)]) -> ParserSpec {
        fields
            .iter()
            .fold(
                ParserSpec::builder(Format::Json),
                |builder, (name, path)| builder.field(*name, *path),
            )
            .build()
            .expect("valid spec")
    }

    fn fields<'a>(record: &'a FieldRecord<'_>) -> Vec<(&'a str, &'a str)> {
        record.iter().collect()
    }

    #[test]
    fn returns_values_as_text_in_declared_order() {
        let spec = spec(&[
            ("paid", "/order/paid"),
            ("order_id", "/order/id"),
            ("amount", "/order/amount"),
            ("second_sku", "/items/1/sku"),
        ]);
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
        let spec = spec(&[("first_item", "/items/0"), ("whole", "")]);
        let record = spec.parse(ORDER).expect("valid record");
        assert_eq!(
            record.get("first_item"),
            Some(r#"{"sku": "B-7", "qty": 2}"#)
        );
        assert_eq!(record.get("whole"), Some(ORDER.trim()));
    }

    #[test]
    fn missing_and_null_are_absent_and_empty_string_is_a_value() {
        let spec = spec(&[
            ("memo", "/order/memo"),
            ("coupon", "/order/coupon"),
            ("status", "/order/status"),
            ("through_scalar", "/order/id/x"),
            ("out_of_range", "/items/2"),
            ("leading_zero", "/items/01"),
            ("signed", "/items/+1"),
        ]);
        let record = spec.parse(ORDER).expect("valid record");
        assert_eq!(fields(&record), [("memo", "")]);
    }

    #[test]
    fn empty_as_none_leaves_empty_strings_absent() {
        let spec = ParserSpec::builder(Format::Json)
            .field("memo", "/order/memo")
            .field("order_id", "/order/id")
            .empty_as_none()
            .build()
            .expect("valid spec");
        let record = spec.parse(ORDER).expect("valid record");
        assert_eq!(fields(&record), [("order_id", "A-1")]);
    }

    #[test]
    fn pointer_escapes_match_keys_with_slash_and_tilde() {
        let spec = spec(&[
            ("slash", "/a~1b"),
            ("tilde", "/m~0n"),
            ("tilde_one", "/~01"),
            ("empty_key", "/"),
        ]);
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
        let spec = spec(&[
            ("order_id", "/order/id"),
            ("amount", "/order/amount"),
            ("note", "/note"),
        ]);
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
        let spec = spec(&[("id", "/id")]);
        let record = spec
            .parse(r#"{"id": "A-1", "id": "A-2"}"#)
            .expect("valid record");
        assert_eq!(fields(&record), [("id", "A-1")]);
    }

    #[test]
    fn malformed_unless_input_is_one_json_object() {
        let spec = spec(&[("id", "/id")]);
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

    #[test]
    fn one_spec_gives_the_same_records_across_threads() {
        let spec = spec(&[("order_id", "/order/id"), ("first_sku", "/items/0/sku")]);
        let inputs: Vec<String> = (0..64)
            .map(|i| {
                format!(r#"{{"order": {{"id": "A-{i}"}}, "items": [{{"sku": "S\u00{i:02}"}}]}}"#)
            })
            .collect();
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
    fn public_types_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ParserSpec>();
        assert_send_sync::<ParserSpecBuilder>();
    }
}
