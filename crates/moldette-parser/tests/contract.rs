use std::borrow::Cow;
use std::ops::Range;

use moldette_core::{FieldRecord, Malformed, SpecError};
use moldette_parser::{Format, Parser, ParserSpec};

#[derive(Debug)]
struct Columns;

impl Parser for Columns {
    type Path = Range<usize>;

    fn name(&self) -> &str {
        "columns"
    }

    fn path(&self, path: &str) -> Option<Range<usize>> {
        let (start, end) = path.split_once("..")?;
        Some(start.parse().ok()?..end.parse().ok()?)
    }

    fn read<'a>(
        &self,
        raw: &'a str,
        paths: &[Range<usize>],
        values: &mut [Option<Cow<'a, str>>],
    ) -> Result<(), Malformed> {
        for (path, value) in paths.iter().zip(values) {
            let column = raw
                .get(path.clone())
                .ok_or_else(|| Malformed::new("record is shorter than its columns"))?;
            *value = Some(Cow::Borrowed(column.trim_end()));
        }
        Ok(())
    }
}

struct Case {
    format: Format,
    paths: [&'static str; 2],
    raw: &'static str,
    unreadable: &'static str,
    invalid_path: &'static str,
}

fn cases() -> [Case; 5] {
    [
        Case {
            format: Format::Json,
            paths: ["/amount", "/id"],
            raw: r#"{"id": "A-1", "amount": 12.50, "note": "gift"}"#,
            unreadable: r#"["A-1", 12.50]"#,
            invalid_path: "id",
        },
        Case {
            format: Format::Delimited { delimiter: ',' },
            paths: ["1", "0"],
            raw: "A-1,12.50,gift",
            unreadable: r#"A-1,"12.50,gift"#,
            invalid_path: "first",
        },
        Case {
            format: Format::Regex {
                pattern: r"^(?P<id>\S+) (?P<amount>\S+) (?P<note>\S+)$".into(),
            },
            paths: ["amount", "id"],
            raw: "A-1 12.50 gift",
            unreadable: "A-1",
            invalid_path: "total",
        },
        Case {
            format: Format::Kv {
                pair_delimiter: ' ',
                key_value_delimiter: '=',
            },
            paths: ["amount", "id"],
            raw: "id=A-1 amount=12.50 note=gift",
            unreadable: "id=A-1 =12.50",
            invalid_path: "order id",
        },
        Case {
            format: Format::custom(Columns),
            paths: ["4..10", "0..4"],
            raw: "A-1 12.50 gift",
            unreadable: "A-1",
            invalid_path: "0",
        },
    ]
}

fn build(case: &Case) -> ParserSpec {
    ParserSpec::builder(case.format.clone())
        .field("amount", case.paths[0])
        .field("id", case.paths[1])
        .build()
        .expect("valid spec")
}

fn is_borrowed_from(record: &FieldRecord<'_>, name: &str, raw: &str) -> bool {
    let raw = raw.as_bytes().as_ptr_range();
    matches!(
        record.get_cow(name),
        Some(Cow::Borrowed(value)) if raw.contains(&value.as_ptr())
    )
}

#[test]
fn field_record_holds_only_declared_fields_in_declared_order() {
    for case in cases() {
        let spec = build(&case);
        let record = spec.parse(case.raw).expect("record is parsed");
        let fields: Vec<_> = record.iter().collect();
        assert_eq!(
            fields,
            [("amount", "12.50"), ("id", "A-1")],
            "{:?}",
            case.format
        );
        assert_eq!(record.get("note"), None);
    }
}

#[test]
fn values_borrow_from_the_raw_record() {
    for case in cases() {
        let spec = build(&case);
        let record = spec.parse(case.raw).expect("record is parsed");
        for name in ["amount", "id"] {
            assert!(
                is_borrowed_from(&record, name, case.raw),
                "{name} of {:?}",
                case.format
            );
        }
    }
}

#[test]
fn unreadable_records_are_malformed_with_a_reason() {
    for case in cases() {
        let spec = build(&case);
        let malformed = spec
            .parse(case.unreadable)
            .expect_err("record is malformed");
        assert!(!malformed.reason().is_empty(), "{:?}", case.format);
        assert!(spec.parse(case.raw).is_ok());
    }
}

#[test]
fn rejects_invalid_paths_and_empty_or_repeated_names_at_build() {
    for case in cases() {
        let invalid = ParserSpec::builder(case.format.clone())
            .field("id", case.invalid_path)
            .build();
        assert_eq!(
            invalid.map(drop),
            Err(SpecError::InvalidPath(case.invalid_path.into())),
            "{:?}",
            case.format
        );
        let repeated = ParserSpec::builder(case.format.clone())
            .field("id", case.paths[1])
            .field("id", case.paths[0])
            .build();
        assert_eq!(
            repeated.map(drop),
            Err(SpecError::DuplicateField("id".into())),
            "{:?}",
            case.format
        );
        let empty = ParserSpec::builder(case.format.clone())
            .field("", case.paths[1])
            .build();
        assert_eq!(
            empty.map(drop),
            Err(SpecError::EmptyFieldName),
            "{:?}",
            case.format
        );
    }
}
