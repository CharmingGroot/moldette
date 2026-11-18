use moldette_core::{Explain, FieldRecord, Mapped, OutputSchema, SpecError};

use crate::Rule;
use crate::recorder::{Discard, ExplainRecorder, Recorder};
use crate::rule::CompiledRule;
use crate::telemetry;

/// Rules for every field of an output schema, validated and immutable.
///
/// A schema field without a rule reads the source field of the same name,
/// unless the spec was built with [`MapperSpecBuilder::explicit_rules`].
#[derive(Debug, Clone, PartialEq)]
pub struct MapperSpec {
    schema: OutputSchema,
    rules: Vec<Option<CompiledRule>>,
}

impl MapperSpec {
    pub fn builder(schema: OutputSchema) -> MapperSpecBuilder {
        MapperSpecBuilder {
            schema,
            rules: Vec::new(),
            explicit_rules: false,
        }
    }

    pub fn schema(&self) -> &OutputSchema {
        &self.schema
    }

    /// Maps one record. Output field names, lookup values and constants
    /// borrow from the spec, and other text values borrow from the record.
    pub fn map<'a>(&'a self, record: &FieldRecord<'a>) -> Mapped<'a> {
        self.run(record, &mut Discard)
    }

    /// Maps one record like [`MapperSpec::map`] and records, for each field,
    /// the sources read, the output of each op and the converted value.
    pub fn explain<'a>(&'a self, record: &FieldRecord<'a>) -> (Mapped<'a>, Explain<'a>) {
        let mut recorder = ExplainRecorder::new(&self.schema);
        let mapped = self.run(record, &mut recorder);
        (mapped, recorder.finish())
    }

    fn run<'a, R: Recorder<'a>>(
        &'a self,
        record: &FieldRecord<'a>,
        recorder: &mut R,
    ) -> Mapped<'a> {
        telemetry::map_span(&self.schema).in_scope(|| {
            let mut mapped = Mapped::new();
            for (field, rule) in self.schema.fields().iter().zip(&self.rules) {
                match rule {
                    Some(rule) => rule.run(field, record, &mut mapped, recorder),
                    None => recorder.field(field.name()),
                }
            }
            mapped
        })
    }
}

pub struct MapperSpecBuilder {
    schema: OutputSchema,
    rules: Vec<Rule>,
    explicit_rules: bool,
}

impl MapperSpecBuilder {
    pub fn rule(mut self, rule: Rule) -> Self {
        self.rules.push(rule);
        self
    }

    /// Leaves a schema field without a rule absent instead of reading the
    /// source field of the same name. The field is not reported as emptied,
    /// and [`MapperSpec::explain`] records it with no reads.
    pub fn explicit_rules(mut self) -> Self {
        self.explicit_rules = true;
        self
    }

    /// Fails when a rule targets a field missing from the schema or a field
    /// that already has a rule, or when an op argument is not valid.
    pub fn build(self) -> Result<MapperSpec, SpecError> {
        let span = telemetry::build_span(&self.schema, self.rules.len(), self.explicit_rules);
        span.in_scope(|| self.compile().inspect_err(telemetry::spec_error))
    }

    fn compile(self) -> Result<MapperSpec, SpecError> {
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
                    Some(i) => declared.swap_remove(i).compile().map(Some),
                    None if self.explicit_rules => Ok(None),
                    None => Rule::new(field.name(), field.name()).compile().map(Some),
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
    use std::sync::Arc;
    use std::{panic, thread};

    use moldette_core::{
        ExplainField, ExplainRead, ExplainSource, ExplainStep, Field, Timestamp, Type, Value,
    };

    use super::*;
    use crate::Op;
    use crate::op::CompiledOp;

    const LOG_TIME: &str =
        "[day]/[month repr:short]/[year]:[hour]:[minute]:[second] [offset_hour][offset_minute]";
    const LOCAL_TIME: &str = "[year]-[month]-[day] [hour]:[minute]:[second]";
    const UNIX_TIME: &str = "[unix_timestamp]";

    #[derive(Debug)]
    struct StripPrefix(char);

    impl Op for StripPrefix {
        fn apply<'a>(&'a self, text: &'a str) -> Option<Cow<'a, str>> {
            Some(Cow::Borrowed(text.strip_prefix(self.0).unwrap_or(text)))
        }

        fn name(&self) -> &str {
            "strip_prefix"
        }
    }

    #[derive(Debug)]
    struct Cents;

    impl Op for Cents {
        fn apply<'a>(&'a self, text: &'a str) -> Option<Cow<'a, str>> {
            let cents: u64 = text.parse().ok()?;
            Some(Cow::Owned(format!("{}.{:02}", cents / 100, cents % 100)))
        }

        fn name(&self) -> &str {
            "cents"
        }
    }

    #[derive(Debug)]
    struct PanicsOnEmpty;

    impl Op for PanicsOnEmpty {
        fn apply<'a>(&'a self, text: &'a str) -> Option<Cow<'a, str>> {
            assert!(!text.is_empty(), "empty order id");
            Some(Cow::Borrowed(text))
        }

        fn name(&self) -> &str {
            "panics_on_empty"
        }
    }

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
            .rule(Rule::new("amount", "total").via(StripPrefix('$')))
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

        let spec = MapperSpec::builder(order_schema())
            .rule(Rule::new("order_id", "raw").trim().via(StripPrefix('#')))
            .build()
            .expect("valid rule");
        let padded = record(&[("raw", " #A-1 ")]);
        assert!(matches!(
            spec.map(&padded).get("order_id"),
            Some(Value::Str(Cow::Borrowed("A-1")))
        ));
        let shared: Arc<dyn Op> = Arc::new(StripPrefix('#'));
        let shared_spec = MapperSpec::builder(order_schema())
            .rule(Rule::new("order_id", "raw").trim().via(shared))
            .build()
            .expect("valid rule");
        assert!(matches!(
            shared_spec.map(&padded).get("order_id"),
            Some(Value::Str(Cow::Borrowed("A-1")))
        ));
        let mut owned = FieldRecord::new();
        owned.insert("raw", String::from("#A-1"));
        assert_eq!(
            spec.map(&owned).get("order_id"),
            Some(&Value::Str("A-1".into()))
        );
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
            (
                Rule::new("order_id", "raw").via(Cents),
                "1250",
                Some("12.50"),
            ),
            (Rule::new("order_id", "raw").via(Cents), "12.50", None),
            (
                Rule::new("order_id", "raw").via(Arc::new(Cents) as Arc<dyn Op>),
                "1250",
                Some("12.50"),
            ),
            (
                Rule::new("order_id", "raw").via(Cents).split('.', 0),
                "1250",
                Some("12"),
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
    fn rules_are_equal_only_when_they_share_custom_ops() {
        let rule = Rule::new("amount", "total").via(Cents);
        assert_eq!(rule.clone(), rule);
        assert_ne!(Rule::new("amount", "total").via(Cents), rule);

        let shared: Arc<dyn Op> = Arc::new(Cents);
        let rule = Rule::new("amount", "total").via(Arc::clone(&shared));
        assert_eq!(rule.clone(), rule);
        assert_ne!(Rule::new("amount", "total").via(shared), rule);
    }

    #[test]
    fn custom_op_panic_reaches_the_caller_and_leaves_the_spec_usable() {
        let spec = MapperSpec::builder(order_schema())
            .rule(Rule::new("order_id", "id").via(PanicsOnEmpty))
            .build()
            .expect("valid rule");
        let empty = record(&[("id", "")]);
        assert!(panic::catch_unwind(|| spec.map(&empty).into_owned()).is_err());

        let mapped = spec.map(&record(&[("id", "A-1")]));
        assert_eq!(mapped.get("order_id"), Some(&Value::Str("A-1".into())));
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
    fn explicit_rules_leave_fields_without_a_rule_absent() {
        let fields = [("order_id", "A-1"), ("quantity", "3"), ("paid", "true")];
        let builder =
            || MapperSpec::builder(order_schema()).rule(Rule::new("quantity", "quantity").trim());

        let implicit = builder().build().expect("valid rule");
        let mapped = implicit.map(&record(&fields));
        assert_eq!(mapped.get("order_id"), Some(&Value::Str("A-1".into())));
        assert_eq!(mapped.get("paid"), Some(&Value::Bool(true)));

        let explicit = builder().explicit_rules().build().expect("valid rule");
        let record = record(&fields);
        let (mapped, explain) = explicit.explain(&record);
        assert_eq!(mapped, explicit.map(&record));
        assert_eq!(
            mapped.iter().collect::<Vec<_>>(),
            [("quantity", &Value::Int(3))]
        );
        assert_eq!(mapped.emptied().count(), 0);
        let reads: Vec<_> = explain
            .fields()
            .iter()
            .map(|field| (field.name(), field.reads().len()))
            .collect();
        assert_eq!(
            reads,
            [
                ("order_id", 0),
                ("quantity", 1),
                ("amount", 0),
                ("paid", 0),
                ("ordered_at", 0),
            ]
        );
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
    fn shared_spec_maps_and_explains_the_same_from_several_threads() {
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
                record.insert("total", format!("${i}.5"));
                record.insert("paid", if i % 2 == 0 { "true" } else { "false" });
                record.insert("at", (1_700_000_000 + i).to_string());
                record
            })
            .collect();
        let run = || {
            records
                .iter()
                .map(|r| (spec.map(r), spec.explain(r)))
                .collect::<Vec<_>>()
        };
        let expected = run();
        for (mapped, (explained, _)) in &expected {
            assert_eq!(explained, mapped);
        }

        thread::scope(|scope| {
            let handles: Vec<_> = (0..4).map(|_| scope.spawn(run)).collect();
            for handle in handles {
                assert_eq!(handle.join().expect("thread finished"), expected);
            }
        });
    }

    #[derive(Default)]
    struct Events(Vec<String>);

    impl<'a> Recorder<'a> for Events {
        fn field(&mut self, name: &'a str) {
            self.0.push(format!("field {name}"));
        }

        fn read(&mut self, field: &'a str, text: Option<&Cow<'a, str>>) {
            self.0.push(format!("read {field} {text:?}"));
        }

        fn constant(&mut self, value: &'a str) {
            self.0.push(format!("constant {value:?}"));
        }

        fn step(&mut self, op: &'a CompiledOp, output: Option<&Cow<'a, str>>) {
            self.0.push(format!("step {} {output:?}", op.name()));
        }

        fn coerced(&mut self, value: Option<&Value<'a>>) {
            self.0.push(format!("coerced {value:?}"));
        }
    }

    #[test]
    fn recorder_receives_reads_ops_and_conversions_in_order() {
        let schema = OutputSchema::builder("order", 1)
            .field(Field::new("quantity", Type::Int))
            .field(Field::new("order_id", Type::String))
            .build()
            .expect("valid schema");
        let spec = MapperSpec::builder(schema)
            .rule(
                Rule::coalesce("quantity", ["qty", "count"])
                    .trim()
                    .split(' ', 0)
                    .fallback("n"),
            )
            .rule(Rule::constant("order_id", "").empty_as_none())
            .build()
            .expect("valid rules");
        let record = record(&[("count", " x pcs"), ("n", "4")]);

        let mut events = Events::default();
        let mapped = spec.run(&record, &mut events);

        assert_eq!(mapped, spec.map(&record));
        assert_eq!(
            events.0,
            [
                "field quantity",
                "read qty None",
                r#"read count Some(" x pcs")"#,
                r#"step trim Some("x pcs")"#,
                r#"step split Some("x")"#,
                "coerced None",
                r#"read n Some("4")"#,
                r#"step trim Some("4")"#,
                r#"step split Some("4")"#,
                "coerced Some(Int(4))",
                "field order_id",
                r#"constant """#,
                "step empty_as_none None",
            ]
        );
    }

    fn explained<'a>(
        source: ExplainSource<'a>,
        text: Option<&'a str>,
        steps: &[(&'a str, Option<&'a str>)],
        value: Option<Value<'a>>,
    ) -> ExplainRead<'a> {
        let mut read = ExplainRead::new(source, text.map(Cow::Borrowed));
        for &(op, output) in steps {
            read.push(ExplainStep::new(op, output.map(Cow::Borrowed)));
        }
        if let Some(value) = value {
            read.set_value(value);
        }
        read
    }

    fn explained_field<'a>(
        name: &'a str,
        reads: impl IntoIterator<Item = ExplainRead<'a>>,
    ) -> ExplainField<'a> {
        let mut field = ExplainField::new(name);
        for read in reads {
            field.push(read);
        }
        field
    }

    #[test]
    fn explain_records_reads_op_outputs_and_conversions() {
        let schema = OutputSchema::builder("order", 3)
            .field(Field::new("quantity", Type::Int))
            .field(Field::new("amount", Type::Float))
            .field(Field::new("currency", Type::String))
            .field(Field::new("paid", Type::Bool))
            .build()
            .expect("valid schema");
        let spec = MapperSpec::builder(schema)
            .rule(
                Rule::coalesce("quantity", ["qty", "count"])
                    .trim()
                    .split(' ', 0)
                    .fallback("n"),
            )
            .rule(
                Rule::new("amount", "total")
                    .via(Arc::new(Cents) as Arc<dyn Op>)
                    .fallback("subtotal"),
            )
            .rule(Rule::constant("currency", "usd").lookup([("usd", "USD")]))
            .build()
            .expect("valid rules");
        let record = record(&[
            ("count", " x pcs"),
            ("n", "4"),
            ("total", "12.50"),
            ("paid", "yes"),
        ]);

        let (mapped, explain) = spec.explain(&record);

        assert_eq!(mapped, spec.map(&record));
        assert_eq!((explain.schema(), explain.version()), ("order", 3));
        let field = |name| ExplainSource::Field(Cow::Borrowed(name));
        assert_eq!(
            explain.fields(),
            [
                explained_field(
                    "quantity",
                    [
                        explained(field("qty"), None, &[], None),
                        explained(
                            field("count"),
                            Some(" x pcs"),
                            &[("trim", Some("x pcs")), ("split", Some("x"))],
                            None,
                        ),
                        explained(
                            field("n"),
                            Some("4"),
                            &[("trim", Some("4")), ("split", Some("4"))],
                            Some(Value::Int(4)),
                        ),
                    ],
                ),
                explained_field(
                    "amount",
                    [
                        explained(field("total"), Some("12.50"), &[("cents", None)], None),
                        explained(field("subtotal"), None, &[], None),
                    ],
                ),
                explained_field(
                    "currency",
                    [explained(
                        ExplainSource::Constant,
                        Some("usd"),
                        &[("lookup", Some("USD"))],
                        Some(Value::Str("USD".into())),
                    )],
                ),
                explained_field("paid", [explained(field("paid"), Some("yes"), &[], None)],),
            ]
        );
    }
}
