use std::borrow::Cow;

use crate::Value;

/// How each output field of one record got its value, with the name and
/// version of the output schema (#39).
#[derive(Debug, Clone, PartialEq)]
pub struct Explain<'a> {
    schema: Cow<'a, str>,
    version: u32,
    fields: Vec<ExplainField<'a>>,
}

impl<'a> Explain<'a> {
    pub fn new(schema: impl Into<Cow<'a, str>>, version: u32) -> Self {
        Self {
            schema: schema.into(),
            version,
            fields: Vec::new(),
        }
    }

    pub fn push(&mut self, field: ExplainField<'a>) {
        self.fields.push(field);
    }

    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    /// Fields in output schema order.
    pub fn fields(&self) -> &[ExplainField<'a>] {
        &self.fields
    }

    pub fn field(&self, name: &str) -> Option<&ExplainField<'a>> {
        self.fields.iter().find(|field| field.name == name)
    }

    pub fn into_owned(self) -> Explain<'static> {
        Explain {
            schema: Cow::Owned(self.schema.into_owned()),
            version: self.version,
            fields: self
                .fields
                .into_iter()
                .map(ExplainField::into_owned)
                .collect(),
        }
    }
}

/// The reads made for one output field, in the order they were tried.
#[derive(Debug, Clone, PartialEq)]
pub struct ExplainField<'a> {
    name: Cow<'a, str>,
    reads: Vec<ExplainRead<'a>>,
}

impl<'a> ExplainField<'a> {
    pub fn new(name: impl Into<Cow<'a, str>>) -> Self {
        Self {
            name: name.into(),
            reads: Vec::new(),
        }
    }

    pub fn push(&mut self, read: ExplainRead<'a>) {
        self.reads.push(read);
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn reads(&self) -> &[ExplainRead<'a>] {
        &self.reads
    }

    pub fn into_owned(self) -> ExplainField<'static> {
        ExplainField {
            name: Cow::Owned(self.name.into_owned()),
            reads: self
                .reads
                .into_iter()
                .map(ExplainRead::into_owned)
                .collect(),
        }
    }
}

/// One source read: the text found, the output of each op applied to it and
/// the converted value.
#[derive(Debug, Clone, PartialEq)]
pub struct ExplainRead<'a> {
    source: ExplainSource<'a>,
    text: Option<Cow<'a, str>>,
    steps: Vec<ExplainStep<'a>>,
    value: Option<Value<'a>>,
}

impl<'a> ExplainRead<'a> {
    pub fn new(source: ExplainSource<'a>, text: Option<Cow<'a, str>>) -> Self {
        Self {
            source,
            text,
            steps: Vec::new(),
            value: None,
        }
    }

    pub fn push(&mut self, step: ExplainStep<'a>) {
        self.steps.push(step);
    }

    pub fn set_value(&mut self, value: Value<'a>) {
        self.value = Some(value);
    }

    pub fn source(&self) -> &ExplainSource<'a> {
        &self.source
    }

    /// The text read, or `None` when the source field is not in the record.
    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    pub fn steps(&self) -> &[ExplainStep<'a>] {
        &self.steps
    }

    /// The value converted to the field's schema type, or `None` when no text
    /// was read, an op yielded no text or the conversion failed.
    pub fn value(&self) -> Option<&Value<'a>> {
        self.value.as_ref()
    }

    pub fn into_owned(self) -> ExplainRead<'static> {
        ExplainRead {
            source: self.source.into_owned(),
            text: self.text.map(|text| Cow::Owned(text.into_owned())),
            steps: self
                .steps
                .into_iter()
                .map(ExplainStep::into_owned)
                .collect(),
            value: self.value.map(Value::into_owned),
        }
    }
}

/// Where a read takes its text from.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExplainSource<'a> {
    Field(Cow<'a, str>),
    Constant,
}

impl ExplainSource<'_> {
    pub fn into_owned(self) -> ExplainSource<'static> {
        match self {
            Self::Field(name) => ExplainSource::Field(Cow::Owned(name.into_owned())),
            Self::Constant => ExplainSource::Constant,
        }
    }
}

/// The output of one op, or `None` when the op could not handle its input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplainStep<'a> {
    op: Cow<'a, str>,
    output: Option<Cow<'a, str>>,
}

impl<'a> ExplainStep<'a> {
    pub fn new(op: impl Into<Cow<'a, str>>, output: Option<Cow<'a, str>>) -> Self {
        Self {
            op: op.into(),
            output,
        }
    }

    pub fn op(&self) -> &str {
        &self.op
    }

    pub fn output(&self) -> Option<&str> {
        self.output.as_deref()
    }

    pub fn into_owned(self) -> ExplainStep<'static> {
        ExplainStep {
            op: Cow::Owned(self.op.into_owned()),
            output: self.output.map(|output| Cow::Owned(output.into_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quantity<'a>(source: &'a str, text: &'a str) -> Explain<'a> {
        let mut read = ExplainRead::new(ExplainSource::Field(source.into()), Some(text.into()));
        read.push(ExplainStep::new("trim", Some(text.trim().into())));
        read.set_value(Value::Str(text.trim().into()));
        let mut field = ExplainField::new("quantity");
        field.push(ExplainRead::new(ExplainSource::Field("count".into()), None));
        field.push(read);
        let mut explain = Explain::new("order", 2);
        explain.push(field);
        explain
    }

    #[test]
    fn finds_fields_by_name() {
        let explain = quantity("qty", " 3");
        assert_eq!((explain.schema(), explain.version()), ("order", 2));
        let field = explain.field("quantity").expect("recorded field");
        assert_eq!(field.reads()[0].text(), None);
        let read = &field.reads()[1];
        assert_eq!(read.source(), &ExplainSource::Field("qty".into()));
        assert_eq!(read.text(), Some(" 3"));
        assert_eq!(read.steps()[0].op(), "trim");
        assert_eq!(read.steps()[0].output(), Some("3"));
        assert_eq!(read.value(), Some(&Value::Str("3".into())));
        assert!(explain.field("amount").is_none());
    }

    #[test]
    fn borrows_until_converted_to_owned() {
        let source = String::from("qty");
        let text = String::from(" 3");
        let owned: Explain<'static> = quantity(&source, &text).into_owned();
        drop((source, text));
        assert_eq!(owned, quantity("qty", " 3"));
    }
}
