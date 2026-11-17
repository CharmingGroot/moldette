use std::borrow::Cow;

use moldette_core::{Mapped, OutputSchema, Response};

/// Turns [`Mapped`] records into a [`Response`] for one output schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RespSpec {
    schema: OutputSchema,
}

impl RespSpec {
    pub fn new(schema: OutputSchema) -> Self {
        Self { schema }
    }

    /// Returns [`Response::MissingRequired`] with every required, non-deprecated
    /// field absent from `mapped`, in schema order, or [`Response::Ok`] with
    /// `mapped` unchanged.
    pub fn respond<'a>(&'a self, mapped: Mapped<'a>) -> Response<'a> {
        let missing: Vec<Cow<'a, str>> = self
            .schema
            .fields()
            .iter()
            .filter(|field| field.is_required() && !field.is_deprecated())
            .filter(|field| mapped.get(field.name()).is_none())
            .map(|field| Cow::Borrowed(field.name()))
            .collect();
        if missing.is_empty() {
            Response::Ok(mapped)
        } else {
            Response::MissingRequired(missing)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use moldette_core::{Field, Timestamp, Type, Value};

    use super::*;

    fn order_spec() -> RespSpec {
        let schema = OutputSchema::builder("order", 1)
            .field(Field::new("order_id", Type::String).required())
            .field(Field::new("amount", Type::Float).required())
            .field(Field::new("coupon", Type::String).required().deprecated())
            .field(Field::new("status", Type::String))
            .field(Field::new("ordered_at", Type::Timestamp).required())
            .build()
            .expect("valid schema");
        RespSpec::new(schema)
    }

    fn ordered_at() -> Value<'static> {
        Value::Timestamp(Timestamp::new(1_700_000_000, 0, 0).expect("valid"))
    }

    #[test]
    fn ok_keeps_mapped_unchanged_when_required_fields_are_present() {
        let mut mapped = Mapped::new();
        mapped.insert("ordered_at", ordered_at());
        mapped.insert("order_id", Value::Str("".into()));
        mapped.insert("amount", Value::Float(12.5));
        mapped.insert("coupon", Value::Str("WELCOME".into()));
        mapped.insert("channel", Value::Str("web".into()));
        mapped.mark_emptied("status");

        assert_eq!(order_spec().respond(mapped.clone()), Response::Ok(mapped));
    }

    #[test]
    fn missing_required_lists_absent_fields_in_schema_order() {
        let mut mapped = Mapped::new();
        mapped.mark_emptied("ordered_at");
        mapped.insert("amount", Value::Float(12.5));
        mapped.insert("coupon", Value::Str("WELCOME".into()));

        assert_eq!(
            order_spec().respond(mapped),
            Response::MissingRequired(vec!["order_id".into(), "ordered_at".into()])
        );
    }

    #[test]
    fn deprecated_fields_are_not_required() {
        let mut mapped = Mapped::new();
        mapped.insert("order_id", Value::Str("A-1".into()));
        mapped.insert("amount", Value::Float(12.5));
        mapped.insert("ordered_at", ordered_at());

        assert_eq!(order_spec().respond(mapped.clone()), Response::Ok(mapped));
    }

    #[test]
    fn shared_spec_matches_single_threaded_responses() {
        let spec = order_spec();
        let records: Vec<Mapped<'static>> = (0..64)
            .map(|i| {
                let mut mapped = Mapped::new();
                mapped.insert("amount", Value::Float(f64::from(i)));
                if i % 2 == 0 {
                    mapped.insert("order_id", Value::Str(format!("A-{i}").into()));
                }
                if i % 3 == 0 {
                    mapped.insert("ordered_at", ordered_at());
                }
                mapped
            })
            .collect();
        let expected: Vec<_> = records
            .iter()
            .map(|mapped| spec.respond(mapped.clone()))
            .collect();

        thread::scope(|scope| {
            let handles: Vec<_> = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        records
                            .iter()
                            .map(|mapped| spec.respond(mapped.clone()))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            for handle in handles {
                assert_eq!(handle.join().expect("thread completes"), expected);
            }
        });
    }
}
