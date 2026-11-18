use moldette_core::{FieldRecord, Mapped, OutputSchema, SpecError};

use crate::Rule;
use crate::coerce::coerce;

/// Rules for every field of an output schema, validated and immutable.
///
/// A schema field without a rule reads the source field of the same name.
#[derive(Debug, Clone, PartialEq)]
pub struct MapperSpec {
    schema: OutputSchema,
    rules: Vec<Rule>,
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

    /// Maps one record. Output field names borrow from the spec, and text
    /// values borrow from the record.
    pub fn map<'a>(&'a self, record: &FieldRecord<'a>) -> Mapped<'a> {
        let mut mapped = Mapped::new();
        for (field, rule) in self.schema.fields().iter().zip(&self.rules) {
            let mut found = false;
            let value = rule
                .sources()
                .filter_map(|source| record.get_cow(source))
                .find_map(|text| {
                    found = true;
                    coerce(text, field.ty())
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
    /// that already has a rule.
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
                    Some(i) => declared.swap_remove(i),
                    None => Rule::new(field.name(), field.name()),
                },
            )
            .collect();
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

    use moldette_core::{Field, Type, Value};

    use super::*;

    fn order_schema() -> OutputSchema {
        OutputSchema::builder("order", 1)
            .field(Field::new("order_id", Type::String).required())
            .field(Field::new("quantity", Type::Int))
            .field(Field::new("amount", Type::Float))
            .field(Field::new("paid", Type::Bool))
            .build()
            .expect("valid schema")
    }

    fn order_spec() -> MapperSpec {
        MapperSpec::builder(order_schema())
            .rule(Rule::new("order_id", "id").fallback("order_no"))
            .rule(Rule::new("quantity", "qty").fallback("count"))
            .rule(Rule::new("amount", "total"))
            .build()
            .expect("valid rules")
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
        let record = record(&[("id", "A-1")]);
        let mapped = spec.map(&record);
        assert!(matches!(
            mapped.get("order_id"),
            Some(Value::Str(Cow::Borrowed("A-1")))
        ));
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
    fn shared_spec_maps_the_same_from_several_threads() {
        let spec = order_spec();
        let records: Vec<_> = (0..64)
            .map(|i| {
                let quantity = if i % 3 == 0 {
                    "x".into()
                } else {
                    i.to_string()
                };
                let mut record = FieldRecord::new();
                record.insert("id", format!("A-{i}"));
                record.insert("qty", quantity);
                record.insert("total", format!("{i}.5"));
                record.insert("paid", if i % 2 == 0 { "true" } else { "false" });
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
