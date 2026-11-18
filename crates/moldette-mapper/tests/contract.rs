use std::borrow::Cow;

use moldette_core::{Field, FieldRecord, OutputSchema, SpecError, Timestamp, Type, Value};
use moldette_mapper::{MapperSpec, Rule};

fn schema(fields: impl IntoIterator<Item = Field>) -> OutputSchema {
    fields
        .into_iter()
        .fold(OutputSchema::builder("order", 1), |builder, field| {
            builder.field(field)
        })
        .build()
        .expect("valid schema")
}

fn record<'a>(fields: &[(&'a str, &'a str)]) -> FieldRecord<'a> {
    let mut record = FieldRecord::new();
    for (name, value) in fields {
        record.insert(*name, *value);
    }
    record
}

#[test]
fn mapped_follows_schema_order_and_holds_only_schema_fields() {
    let spec = MapperSpec::builder(schema([
        Field::new("id", Type::String),
        Field::new("amount", Type::Float),
        Field::new("status", Type::Int),
    ]))
    .build()
    .expect("valid spec");
    let record = record(&[
        ("status", "200"),
        ("note", "gift"),
        ("amount", "12.5"),
        ("id", "A-1"),
    ]);

    let mapped = spec.map(&record);
    let fields: Vec<_> = mapped.iter().collect();
    assert_eq!(
        fields,
        [
            ("id", &Value::Str("A-1".into())),
            ("amount", &Value::Float(12.5)),
            ("status", &Value::Int(200)),
        ]
    );
    assert_eq!(mapped.emptied().count(), 0);
}

#[test]
fn converts_text_to_the_schema_type_of_each_field() {
    let spec = MapperSpec::builder(schema([
        Field::new("id", Type::String),
        Field::new("quantity", Type::Int),
        Field::new("amount", Type::Float),
        Field::new("paid", Type::Bool),
        Field::new("placed_at", Type::Timestamp),
        Field::new("client", Type::Ip),
    ]))
    .build()
    .expect("valid spec");
    let record = record(&[
        ("id", "A-1"),
        ("quantity", "-3"),
        ("amount", "12.5"),
        ("paid", "true"),
        ("placed_at", "2023-11-15T07:13:20+09:00"),
        ("client", "192.0.2.1"),
    ]);

    let mapped = spec.map(&record);
    assert_eq!(mapped.get("id"), Some(&Value::Str("A-1".into())));
    assert_eq!(mapped.get("quantity"), Some(&Value::Int(-3)));
    assert_eq!(mapped.get("amount"), Some(&Value::Float(12.5)));
    assert_eq!(mapped.get("paid"), Some(&Value::Bool(true)));
    assert_eq!(
        mapped.get("placed_at"),
        Some(&Value::Timestamp(
            Timestamp::new(1_700_000_000, 0, 9 * 60 * 60).expect("timestamp")
        ))
    );
    assert_eq!(
        mapped.get("client"),
        Some(&Value::Ip("192.0.2.1".parse().expect("ip")))
    );
}

#[test]
fn text_values_borrow_from_the_record() {
    let spec = MapperSpec::builder(schema([Field::new("id", Type::String)]))
        .rule(Rule::new("id", "order_id").trim())
        .build()
        .expect("valid spec");
    let raw = String::from(" A-1 ");
    let mut record = FieldRecord::new();
    record.insert("order_id", raw.as_str());

    let mapped = spec.map(&record);
    let Some(Value::Str(Cow::Borrowed(id))) = mapped.get("id") else {
        panic!("id is borrowed");
    };
    assert_eq!(*id, "A-1");
    assert!(raw.as_bytes().as_ptr_range().contains(&id.as_ptr()));
}

#[test]
fn reports_values_ops_or_conversion_cannot_handle_as_emptied() {
    let spec = MapperSpec::builder(schema([
        Field::new("id", Type::String),
        Field::new("amount", Type::Float),
        Field::new("region", Type::String),
    ]))
    .rule(Rule::new("region", "region").extract(r"^([A-Z]{2})-"))
    .build()
    .expect("valid spec");
    let record = record(&[("id", "A-1"), ("amount", "twelve"), ("region", "north")]);

    let mapped = spec.map(&record);
    assert_eq!(mapped.get("amount"), None);
    assert_eq!(mapped.get("region"), None);
    assert_eq!(mapped.emptied().collect::<Vec<_>>(), ["amount", "region"]);
    assert_eq!(mapped.get("id"), Some(&Value::Str("A-1".into())));
}

#[test]
fn leaves_missing_sources_absent_without_reporting_them_as_emptied() {
    let spec = MapperSpec::builder(schema([
        Field::new("id", Type::String),
        Field::new("amount", Type::Float),
    ]))
    .build()
    .expect("valid spec");

    let mapped = spec.map(&record(&[("id", "A-1")]));
    assert_eq!(mapped.get("amount"), None);
    assert_eq!(mapped.emptied().count(), 0);
}

#[test]
fn explain_maps_as_map_does() {
    let spec = MapperSpec::builder(schema([
        Field::new("id", Type::String),
        Field::new("amount", Type::Float),
    ]))
    .rule(Rule::coalesce("id", ["order_id", "id"]))
    .build()
    .expect("valid spec");
    let record = record(&[("id", "A-1"), ("amount", "twelve")]);

    let (mapped, explain) = spec.explain(&record);
    assert_eq!(mapped, spec.map(&record));
    assert_eq!((explain.schema(), explain.version()), ("order", 1));
}

#[test]
fn rejects_rules_outside_the_schema_and_repeated_rules_at_build() {
    let order = || schema([Field::new("id", Type::String)]);
    assert_eq!(
        MapperSpec::builder(order())
            .rule(Rule::new("discount", "discount"))
            .build()
            .map(drop),
        Err(SpecError::UnknownField("discount".into()))
    );
    assert_eq!(
        MapperSpec::builder(order())
            .rule(Rule::new("id", "id"))
            .rule(Rule::new("id", "order_id"))
            .build()
            .map(drop),
        Err(SpecError::DuplicateRule("id".into()))
    );
}
