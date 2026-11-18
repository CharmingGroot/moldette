use moldette_core::{FieldRecord, Mapped, OutputSchema, SpecError};

use crate::Rule;
use crate::coerce::coerce;
use crate::rule::CompiledRule;

/// Rules for every field of an output schema, validated and immutable.
///
/// A schema field without a rule reads the source field of the same name.
#[derive(Debug, Clone, PartialEq)]
pub struct MapperSpec {
    schema: OutputSchema,
    rules: Vec<CompiledRule>,
}

impl MapperSpec {
    pub fn builder(schema: OutputSchema) -> MapperSpecBuilder {
        MapperSpecBuilder {
            schema,
            rules: Vec::new(),
        }
    }

    pub fn schema(&self) -> &OutputSchema {
        &self.schema
    }

    /// Maps one record. Output field names, lookup values and constants
    /// borrow from the spec, and other text values borrow from the record.
    pub fn map<'a>(&'a self, record: &FieldRecord<'a>) -> Mapped<'a> {
        let mut mapped = Mapped::new();
        for (field, rule) in self.schema.fields().iter().zip(&self.rules) {
            let mut found = false;
            let value = rule.texts(record).find_map(|text| {
                found = true;
                rule.apply(text).and_then(|text| coerce(text, field.ty()))
            });
            match value {
                Some(value) => mapped.insert(field.name(), value),
                None if found => mapped.mark_emptied(field.name()),
                None => {}
            }
        }
        mapped
    }
}

pub struct MapperSpecBuilder {
    schema: OutputSchema,
    rules: Vec<Rule>,
}

impl MapperSpecBuilder {
    pub fn rule(mut self, rule: Rule) -> Self {
        self.rules.push(rule);
        self
    }

    /// Fails when a rule targets a field missing from the schema or a field
    /// that already has a rule, or when an op argument is not valid.
    pub fn build(self) -> Result<MapperSpec, SpecError> {
        for (i, rule) in self.rules.iter().enumerate() {
            if self.schema.field(&rule.target).is_none() {
                return Err(SpecError::UnknownField(rule.target.clone()));
            }
            if self.rules[..i].iter().any(|r| r.target == rule.target) {
                return Err(SpecError::DuplicateRule(rule.target.clone()));
            }
        }
        let mut declared = self.rules;
        let rules = self
            .schema
            .fields()
            .iter()
            .map(
                |field| match declared.iter().position(|r| r.target == field.name()) {
                    Some(i) => declared.swap_remove(i).compile(),
                    None => Rule::new(field.name(), field.name()).compile(),
                },
            )
            .collect::<Result<_, _>>()?;
        Ok(MapperSpec {
            schema: self.schema,
            rules,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::thread;

    use moldette_core::{Field, Timestamp, Type, Value};

    use super::*;

    const LOG_TIME: &str =
        "[day]/[month repr:short]/[year]:[hour]:[minute]:[second] [offset_hour][offset_minute]";
    const LOCAL_TIME: &str = "[year]-[month]-[day] [hour]:[minute]:[second]";
    const UNIX_TIME: &str = "[unix_timestamp]";

    fn order_schema() -> OutputSchema {
        OutputSchema::builder("order", 1)
            .field(Field::new("order_id", Type::String).required())
            .field(Field::new("quantity", Type::Int))
            .field(Field::new("amount", Type::Float))
            .field(Field::new("paid", Type::Bool))
            .field(Field::new("ordered_at", Type::Timestamp))
            .build()
            .expect("valid schema")
    }

    fn order_spec() -> MapperSpec {
        MapperSpec::builder(order_schema())
            .rule(Rule::new("order_id", "id").fallback("order_no"))
            .rule(Rule::new("quantity", "qty").trim().fallback("count"))
            .rule(Rule::new("amount", "total"))
            .rule(Rule::new("ordered_at", "at").timestamp([LOG_TIME, LOCAL_TIME, UNIX_TIME]))
            .build()
            .expect("valid rules")
    }

    fn map_with(rule: Rule, fields: &[(&str, &str)]) -> Mapped<'static> {
        let spec = MapperSpec::builder(order_schema())
            .rule(rule)
            .build()
            .expect("valid rule");
        spec.map(&record(fields)).into_owned()
    }

    fn record<'a>(fields: &[(&'a str, &'a str)]) -> FieldRecord<'a> {
        let mut record = FieldRecord::new();
        for &(name, value) in fields {
            record.insert(name, value);
        }
        record
    }

    #[test]
    fn maps_from_source_fields_in_schema_order() {
        let spec = order_spec();
        let record = record(&[("paid", "true"), ("total", "12.5"), ("id", "A-1")]);
        let mapped = spec.map(&record);
        let names: Vec<_> = mapped.iter().map(|(name, _)| name).collect();
        assert_eq!(names, ["order_id", "amount", "paid"]);
        assert_eq!(mapped.get("order_id"), Some(&Value::Str("A-1".into())));
        assert_eq!(mapped.get("amount"), Some(&Value::Float(12.5)));
        assert_eq!(mapped.get("paid"), Some(&Value::Bool(true)));
        assert_eq!(mapped.emptied().count(), 0);
    }

    #[test]
    fn text_values_borrow_from_the_record() {
        let spec = order_spec();
        let plain = record(&[("id", "A-1")]);
        let mapped = spec.map(&plain);
        assert!(matches!(
            mapped.get("order_id"),
            Some(Value::Str(Cow::Borrowed("A-1")))
        ));

        let spec = MapperSpec::builder(order_schema())
            .rule(Rule::new("order_id", "raw").trim().split('/', 1))
            .build()
            .expect("valid rule");
        let padded = record(&[("raw", " shop/A-1 ")]);
        assert!(matches!(
            spec.map(&padded).get("order_id"),
            Some(Value::Str(Cow::Borrowed("A-1")))
        ));
    }

    #[test]
    fn lookup_values_and_constants_borrow_from_the_spec() {
        let spec = MapperSpec::builder(order_schema())
            .rule(Rule::new("order_id", "channel").lookup([("w", "WEB"), ("s", "STORE")]))
            .rule(Rule::constant("quantity", "1"))
            .build()
            .expect("valid rules");
        let mapped = spec.map(&record(&[("channel", "s")]));
        assert!(matches!(
            mapped.get("order_id"),
            Some(Value::Str(Cow::Borrowed("STORE")))
        ));
        assert_eq!(mapped.get("quantity"), Some(&Value::Int(1)));

        let spec = MapperSpec::builder(order_schema())
            .rule(Rule::constant("order_id", "WEB"))
            .build()
            .expect("valid rule");
        assert!(matches!(
            spec.map(&FieldRecord::new()).get("order_id"),
            Some(Value::Str(Cow::Borrowed("WEB")))
        ));
    }

    #[test]
    fn each_op_rewrites_text_or_yields_no_value() {
        let cases = [
            (Rule::new("order_id", "raw").trim(), " A-1\t", Some("A-1")),
            (
                Rule::new("order_id", "raw").split('/', 1),
                "shop/A-1/x",
                Some("A-1"),
            ),
            (
                Rule::new("order_id", "raw").split('/', 3),
                "shop/A-1/x",
                None,
            ),
            (
                Rule::new("order_id", "raw").extract(r"id=(\w+)"),
                "?id=A1&x=2",
                Some("A1"),
            ),
            (
                Rule::new("order_id", "raw").extract(r"id=(\w+)"),
                "?x=2",
                None,
            ),
            (
                Rule::new("order_id", "raw").extract(r"id=(\d+)?"),
                "id=x",
                None,
            ),
            (
                Rule::new("order_id", "raw").lookup([("1", "paid"), ("1", "refunded")]),
                "1",
                Some("paid"),
            ),
            (
                Rule::new("order_id", "raw").lookup([("1", "paid")]),
                "2",
                None,
            ),
            (Rule::new("order_id", "raw").empty_as_none(), "", None),
            (Rule::new("order_id", "raw").empty_as_none(), " ", Some(" ")),
            (
                Rule::new("order_id", "raw").timestamp([LOCAL_TIME]),
                "2023-11-14 22:13:20",
                Some("2023-11-14T22:13:20Z"),
            ),
        ];
        for (rule, text, expected) in cases {
            let mapped = map_with(rule.clone(), &[("raw", text)]);
            assert_eq!(
                mapped.get("order_id"),
                expected.map(|text| Value::Str(text.into())).as_ref(),
                "{rule:?} on {text:?}"
            );
            assert_eq!(mapped.emptied().count(), usize::from(expected.is_none()));
        }
    }

    #[test]
    fn ops_run_in_order_before_conversion() {
        let fields = [("qty", " 3 pcs")];
        let trimmed_first = map_with(Rule::new("quantity", "qty").trim().split(' ', 0), &fields);
        assert_eq!(trimmed_first.get("quantity"), Some(&Value::Int(3)));

        let split_first = map_with(Rule::new("quantity", "qty").split(' ', 0).trim(), &fields);
        assert_eq!(split_first.get("quantity"), None);
        assert_eq!(split_first.emptied().collect::<Vec<_>>(), ["quantity"]);
    }

    #[test]
    fn timestamp_tries_formats_in_order_and_keeps_the_offset() {
        let spec = order_spec();
        let cases = [
            (
                "14/Nov/2023:22:13:20 +0900",
                Timestamp::new(1_699_967_600, 0, 9 * 60 * 60),
            ),
            ("2023-11-14 22:13:20", Timestamp::new(1_700_000_000, 0, 0)),
            ("1700000000", Timestamp::new(1_700_000_000, 0, 0)),
            ("2023-11-14T22:13:20Z", None),
            ("2023-02-30 00:00:00", None),
        ];
        for (text, expected) in cases {
            let record = record(&[("at", text)]);
            assert_eq!(
                spec.map(&record).get("ordered_at"),
                expected.map(Value::Timestamp).as_ref(),
                "{text:?}"
            );
        }
    }

    #[test]
    fn coalesce_takes_the_first_present_source_before_ops() {
        let rule = Rule::coalesce("quantity", ["qty", "count", "n"]).trim();

        let mapped = map_with(rule.clone(), &[("n", "4"), ("count", " 3")]);
        assert_eq!(mapped.get("quantity"), Some(&Value::Int(3)));

        let mapped = map_with(rule.clone(), &[("count", "x"), ("n", "4")]);
        assert_eq!(mapped.get("quantity"), None);
        assert_eq!(mapped.emptied().collect::<Vec<_>>(), ["quantity"]);

        let mapped = map_with(rule.fallback("n"), &[("count", "x"), ("n", "4")]);
        assert_eq!(mapped.get("quantity"), Some(&Value::Int(4)));

        let mapped = map_with(Rule::coalesce("quantity", ["qty"]), &[]);
        assert_eq!(mapped.iter().count(), 0);
        assert_eq!(mapped.emptied().count(), 0);
    }

    #[test]
    fn absent_op_result_falls_back_and_is_emptied_when_nothing_remains() {
        let rule = Rule::new("order_id", "id")
            .empty_as_none()
            .fallback("order_no");

        let mapped = map_with(rule.clone(), &[("id", ""), ("order_no", "B-2")]);
        assert_eq!(mapped.get("order_id"), Some(&Value::Str("B-2".into())));
        assert_eq!(mapped.emptied().count(), 0);

        let mapped = map_with(rule, &[("id", ""), ("order_no", "")]);
        assert_eq!(mapped.get("order_id"), None);
        assert_eq!(mapped.emptied().collect::<Vec<_>>(), ["order_id"]);
    }

    #[test]
    fn fallback_is_read_when_the_primary_source_yields_no_value() {
        let spec = order_spec();

        let missing = record(&[("order_no", "B-2"), ("count", "3")]);
        let mapped = spec.map(&missing);
        assert_eq!(mapped.get("order_id"), Some(&Value::Str("B-2".into())));
        assert_eq!(mapped.get("quantity"), Some(&Value::Int(3)));

        let unconvertible = record(&[("qty", "three"), ("count", "3")]);
        let mapped = spec.map(&unconvertible);
        assert_eq!(mapped.get("quantity"), Some(&Value::Int(3)));
        assert_eq!(mapped.emptied().count(), 0);

        let empty = record(&[("id", ""), ("order_no", "B-2")]);
        assert_eq!(
            spec.map(&empty).get("order_id"),
            Some(&Value::Str("".into()))
        );
    }

    #[test]
    fn failed_conversion_is_absent_and_reported_as_emptied() {
        let spec = order_spec();
        let record = record(&[("qty", "three"), ("count", "3.0"), ("paid", "yes")]);
        let mapped = spec.map(&record);
        assert_eq!(mapped.get("quantity"), None);
        assert_eq!(mapped.get("paid"), None);
        assert_eq!(mapped.emptied().collect::<Vec<_>>(), ["quantity", "paid"]);
    }

    #[test]
    fn missing_sources_are_absent_without_being_emptied() {
        let spec = order_spec();
        let mapped = spec.map(&FieldRecord::new());
        assert_eq!(mapped.iter().count(), 0);
        assert_eq!(mapped.emptied().count(), 0);
    }

    #[test]
    fn rejects_rule_for_field_outside_schema() {
        let result = MapperSpec::builder(order_schema())
            .rule(Rule::new("discount", "discount"))
            .build();
        assert_eq!(result, Err(SpecError::UnknownField("discount".into())));
    }

    #[test]
    fn rejects_two_rules_for_one_field() {
        let result = MapperSpec::builder(order_schema())
            .rule(Rule::new("amount", "total"))
            .rule(Rule::new("amount", "subtotal"))
            .build();
        assert_eq!(result, Err(SpecError::DuplicateRule("amount".into())));
    }

    #[test]
    fn rejects_invalid_or_empty_op_arguments() {
        let empty: [&str; 0] = [];
        let cases = [
            (
                Rule::new("order_id", "raw").extract("(A-"),
                "extract.pattern",
                "(A-",
            ),
            (
                Rule::new("order_id", "raw").extract("A-1"),
                "extract.pattern",
                "A-1",
            ),
            (
                Rule::new("order_id", "raw").extract("(A)-(1)"),
                "extract.pattern",
                "(A)-(1)",
            ),
            (
                Rule::new("ordered_at", "at").timestamp([LOCAL_TIME, "[year"]),
                "timestamp.formats",
                "[year",
            ),
            (
                Rule::new("ordered_at", "at").timestamp(empty),
                "timestamp.formats",
                "",
            ),
            (Rule::coalesce("quantity", empty), "coalesce.from", ""),
        ];
        for (rule, name, value) in cases {
            let result = MapperSpec::builder(order_schema()).rule(rule).build();
            assert_eq!(
                result,
                Err(SpecError::InvalidArgument {
                    name: name.into(),
                    value: value.into(),
                })
            );
        }
    }

    #[test]
    fn shared_spec_maps_the_same_from_several_threads() {
        let spec = order_spec();
        let records: Vec<_> = (0..64)
            .map(|i| {
                let quantity = if i % 3 == 0 {
                    "x".into()
                } else {
                    format!(" {i} ")
                };
                let mut record = FieldRecord::new();
                record.insert("id", format!("A-{i}"));
                record.insert("qty", quantity);
                record.insert("total", format!("{i}.5"));
                record.insert("paid", if i % 2 == 0 { "true" } else { "false" });
                record.insert("at", (1_700_000_000 + i).to_string());
                record
            })
            .collect();
        let expected: Vec<_> = records.iter().map(|r| spec.map(r)).collect();

        thread::scope(|scope| {
            let handles: Vec<_> = (0..4)
                .map(|_| scope.spawn(|| records.iter().map(|r| spec.map(r)).collect::<Vec<_>>()))
                .collect();
            for handle in handles {
                assert_eq!(handle.join().expect("thread finished"), expected);
            }
        });
    }
}
