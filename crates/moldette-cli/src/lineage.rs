use moldette::config::{Lineage, LineageField, LineagePath, LineageRead, LineageSource};
use serde_json::{Value, json};

pub(crate) fn to_json(lineage: &Lineage) -> Value {
    json!({
        "schema": lineage.schema(),
        "version": lineage.version(),
        "fields": lineage.fields().iter().map(field).collect::<Vec<_>>(),
    })
}

fn field(field: &LineageField) -> Value {
    json!({
        "name": field.name(),
        "source": source(field.source()),
        "reads": field.reads().iter().map(read).collect::<Vec<_>>(),
        "ops": field.ops(),
        "default": field.default(),
    })
}

fn source(source: &LineageSource) -> Value {
    match source {
        LineageSource::From => json!("from"),
        LineageSource::Coalesce => json!("coalesce"),
        LineageSource::Constant(value) => json!({ "constant": value }),
        LineageSource::SameName => json!("same_name"),
        LineageSource::Absent => json!("absent"),
        _ => Value::Null,
    }
}

fn read(read: &LineageRead) -> Value {
    json!({
        "field": read.field(),
        "fallback": read.is_fallback(),
        "origin": read.origin().iter().map(path).collect::<Vec<_>>(),
    })
}

fn path(path: &LineagePath) -> Value {
    json!({ "format": path.format(), "path": path.path() })
}
