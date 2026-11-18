use moldette_core::{Field, Mapped, OutputSchema, Response, Type, Value};
use moldette_resp::RespSpec;

fn order() -> OutputSchema {
    OutputSchema::builder("order", 1)
        .field(Field::new("id", Type::String).required())
        .field(Field::new("coupon", Type::String).required().deprecated())
        .field(Field::new("amount", Type::Float).required())
        .field(Field::new("note", Type::String))
        .build()
        .expect("valid schema")
}

fn complete() -> Mapped<'static> {
    let mut mapped = Mapped::new();
    mapped.insert("id", Value::Str("A-1".into()));
    mapped.insert("amount", Value::Float(12.5));
    mapped
}

fn with_emptied(mut mapped: Mapped<'static>, names: &[&'static str]) -> Mapped<'static> {
    for name in names {
        mapped.mark_emptied(*name);
    }
    mapped
}

#[test]
fn ok_returns_mapped_unchanged() {
    let spec = RespSpec::builder(order()).build();
    let mapped = with_emptied(complete(), &["note"]);
    assert_eq!(spec.respond(mapped.clone()), Response::Ok(mapped));

    let mut empty_id = Mapped::new();
    empty_id.insert("amount", Value::Float(12.5));
    empty_id.insert("id", Value::Str("".into()));
    empty_id.insert("coupon", Value::Str("WELCOME".into()));
    empty_id.insert("channel", Value::Str("web".into()));
    assert_eq!(spec.respond(empty_id.clone()), Response::Ok(empty_id));
}

#[test]
fn missing_required_lists_absent_required_fields_in_schema_order() {
    let spec = RespSpec::builder(order()).build();
    assert_eq!(
        spec.respond(Mapped::new()),
        Response::MissingRequired(vec!["id".into(), "amount".into()])
    );
    let mut without_amount = Mapped::new();
    without_amount.insert("id", Value::Str("A-1".into()));
    assert_eq!(
        spec.respond(without_amount),
        Response::MissingRequired(vec!["amount".into()])
    );
    let mut emptied_amount = Mapped::new();
    emptied_amount.mark_emptied("amount");
    emptied_amount.insert("coupon", Value::Str("WELCOME".into()));
    assert_eq!(
        spec.respond(emptied_amount),
        Response::MissingRequired(vec!["id".into(), "amount".into()])
    );
}

#[test]
fn deprecated_required_fields_are_not_required() {
    let spec = RespSpec::builder(order()).strict().build();
    assert!(matches!(spec.respond(complete()), Response::Ok(_)));
}

#[test]
fn strict_fails_records_with_emptied_fields() {
    let lenient = RespSpec::builder(order()).build();
    let strict = RespSpec::builder(order()).strict().build();
    let mapped = || with_emptied(complete(), &["discount", "note"]);

    let mut extra = complete();
    extra.insert("channel", Value::Str("web".into()));
    assert_eq!(strict.respond(extra.clone()), Response::Ok(extra));
    assert!(matches!(lenient.respond(mapped()), Response::Ok(_)));
    assert_eq!(
        strict.respond(mapped()),
        Response::Emptied(vec!["note".into(), "discount".into()])
    );
    assert_eq!(
        strict.respond(with_emptied(Mapped::new(), &["note"])),
        Response::MissingRequired(vec!["id".into(), "amount".into()])
    );
}

#[test]
fn batch_responds_in_input_order_and_counts_each_status() {
    let spec = RespSpec::builder(order()).strict().build();
    let records = [
        complete(),
        Mapped::new(),
        with_emptied(complete(), &["note"]),
        complete(),
    ];
    let expected: Vec<_> = records
        .iter()
        .map(|mapped| spec.respond(mapped.clone()))
        .collect();

    let (responses, counts) = spec.respond_batch(records);
    assert_eq!(responses, expected);
    assert_eq!(
        (
            counts.ok(),
            counts.malformed(),
            counts.missing_required(),
            counts.emptied()
        ),
        (2, 0, 1, 1)
    );
}
