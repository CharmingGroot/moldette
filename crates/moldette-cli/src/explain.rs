use moldette::{Explain, ExplainRead, ExplainSource, ExplainStep};
use serde_json::{Value, json};

use crate::value::to_json_value;

pub(crate) fn to_json(explain: &Explain<'_>) -> Value {
    json!({
        "schema": explain.schema(),
        "version": explain.version(),
        "fields": explain
            .fields()
            .iter()
            .map(|field| json!({
                "name": field.name(),
                "reads": field.reads().iter().map(read).collect::<Vec<_>>(),
            }))
            .collect::<Vec<_>>(),
    })
}

fn read(read: &ExplainRead<'_>) -> Value {
    json!({
        "source": source(read.source()),
        "text": read.text(),
        "steps": read.steps().iter().map(step).collect::<Vec<_>>(),
        "value": read.value().map(to_json_value),
    })
}

fn source(source: &ExplainSource<'_>) -> Value {
    match source {
        ExplainSource::Field(name) => json!({ "field": name }),
        ExplainSource::Constant => json!("constant"),
        _ => Value::Null,
    }
}

fn step(step: &ExplainStep<'_>) -> Value {
    json!({ "op": step.op(), "output": step.output() })
}
