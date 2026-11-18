use std::borrow::Cow;

use moldette_core::{Mapped, OutputSchema, Response};

/// Turns [`Mapped`] records into a [`Response`] for one output schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RespSpec {
    schema: OutputSchema,
    strict: bool,
}

impl RespSpec {
    pub fn builder(schema: OutputSchema) -> RespSpecBuilder {
        RespSpecBuilder {
            schema,
            strict: false,
        }
    }

    /// Returns [`Response::MissingRequired`] with every required, non-deprecated
    /// field absent from `mapped`, in schema order. Otherwise, in strict mode,
    /// returns [`Response::Emptied`] with the emptied fields in schema order,
    /// followed by those outside the schema in reported order. Otherwise
    /// returns [`Response::Ok`] with `mapped` unchanged, emptied fields
    /// included.
    pub fn respond<'a>(&'a self, mapped: Mapped<'a>) -> Response<'a> {
        let missing: Vec<Cow<'a, str>> = self
            .schema
            .fields()
            .iter()
            .filter(|field| field.is_required() && !field.is_deprecated())
            .filter(|field| mapped.get(field.name()).is_none())
            .map(|field| Cow::Borrowed(field.name()))
            .collect();
        if !missing.is_empty() {
            return Response::MissingRequired(missing);
        }
        if self.strict {
            let emptied = self.emptied_fields(&mapped);
            if !emptied.is_empty() {
                return Response::Emptied(emptied);
            }
        }
        Response::Ok(mapped)
    }

    fn emptied_fields<'a>(&'a self, mapped: &Mapped<'a>) -> Vec<Cow<'a, str>> {
        let mut emptied: Vec<Cow<'a, str>> = self
            .schema
            .fields()
            .iter()
            .filter(|field| mapped.emptied().any(|name| name == field.name()))
            .map(|field| Cow::Borrowed(field.name()))
            .collect();
        for name in mapped.emptied() {
            if self.schema.field(name).is_none() && !emptied.iter().any(|listed| listed == name) {
                emptied.push(Cow::Owned(name.to_owned()));
            }
        }
        emptied
    }
}

pub struct RespSpecBuilder {
    schema: OutputSchema,
    strict: bool,
}

impl RespSpecBuilder {
    /// Fails records with emptied fields as [`Response::Emptied`]. Meant for
    /// tests (#5).
    pub fn strict(mut self) -> Self {
        self.strict = true;
        self
    }

    pub fn build(self) -> RespSpec {
        RespSpec {
            schema: self.schema,
            strict: self.strict,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use moldette_core::{Field, Timestamp, Type, Value};

    use super::*;

    fn order_builder() -> RespSpecBuilder {
        let schema = OutputSchema::builder("order", 1)
            .field(Field::new("order_id", Type::String).required())
            .field(Field::new("amount", Type::Float).required())
            .field(Field::new("coupon", Type::String).required().deprecated())
            .field(Field::new("status", Type::String))
            .field(Field::new("ordered_at", Type::Timestamp).required())
            .build()
            .expect("valid schema");
        RespSpec::builder(schema)
    }

    fn ordered_at() -> Value<'static> {
        Value::Timestamp(Timestamp::new(1_700_000_000, 0, 0).expect("valid"))
    }

    fn complete_order() -> Mapped<'static> {
        let mut mapped = Mapped::new();
        mapped.insert("order_id", Value::Str("A-1".into()));
        mapped.insert("amount", Value::Float(12.5));
        mapped.insert("ordered_at", ordered_at());
        mapped
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

        assert_eq!(
            order_builder().build().respond(mapped.clone()),
            Response::Ok(mapped)
        );
    }

    #[test]
    fn missing_required_lists_absent_fields_in_schema_order() {
        let mut mapped = Mapped::new();
        mapped.mark_emptied("ordered_at");
        mapped.insert("amount", Value::Float(12.5));
        mapped.insert("coupon", Value::Str("WELCOME".into()));

        assert_eq!(
            order_builder().build().respond(mapped),
            Response::MissingRequired(vec!["order_id".into(), "ordered_at".into()])
        );
    }

    #[test]
    fn deprecated_fields_are_not_required() {
        let mapped = complete_order();

        assert_eq!(
            order_builder().build().respond(mapped.clone()),
            Response::Ok(mapped)
        );
    }

    #[test]
    fn strict_accepts_records_without_emptied_fields() {
        let mut mapped = complete_order();
        mapped.insert("channel", Value::Str("web".into()));

        assert_eq!(
            order_builder().strict().build().respond(mapped.clone()),
            Response::Ok(mapped)
        );
    }

    #[test]
    fn strict_lists_emptied_fields_in_schema_order_then_unknown_ones() {
        let mut mapped = complete_order();
        mapped.mark_emptied("channel");
        mapped.mark_emptied("status");
        mapped.mark_emptied("channel");
        mapped.mark_emptied("coupon");

        assert_eq!(
            order_builder().strict().build().respond(mapped),
            Response::Emptied(vec!["coupon".into(), "status".into(), "channel".into()])
        );
    }

    #[test]
    fn strict_reports_missing_required_before_emptied_fields() {
        let mut mapped = Mapped::new();
        mapped.insert("amount", Value::Float(12.5));
        mapped.mark_emptied("status");
        mapped.mark_emptied("ordered_at");

        assert_eq!(
            order_builder().strict().build().respond(mapped),
            Response::MissingRequired(vec!["order_id".into(), "ordered_at".into()])
        );
    }

    #[test]
    fn shared_spec_matches_single_threaded_responses() {
        let records: Vec<Mapped<'static>> = (0..64)
            .map(|i| {
                let mut mapped = Mapped::new();
                mapped.insert("amount", Value::Float(f64::from(i)));
                if i % 2 == 0 {
                    mapped.insert("order_id", Value::Str(format!("A-{i}").into()));
                }
                if i % 3 == 0 {
                    mapped.insert("ordered_at", ordered_at());
                } else if i % 5 == 0 {
                    mapped.mark_emptied("ordered_at");
                }
                if i % 7 == 0 {
                    mapped.mark_emptied("status");
                }
                mapped
            })
            .collect();

        for spec in [order_builder().build(), order_builder().strict().build()] {
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
}
