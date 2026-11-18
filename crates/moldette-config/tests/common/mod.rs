#![allow(dead_code)]

use std::borrow::Cow;
use std::ops::Range;
use std::sync::Arc;

use moldette_config::{Arguments, Config, LoadError, LoadErrorKind, Registry, Specs};
use moldette_core::{Malformed, SpecError, Value};
use moldette_mapper::Op;
use moldette_parser::Parser;

pub const ORDER_SCHEMA: &str = r#"
schemas:
  - name: order
    version: 1
    fields:
      - {name: id, type: string}
      - {name: amount, type: float}
      - {name: placed_at, type: timestamp}
"#;

pub const NO_RULES: &str = r#"schemas:
  - name: order
    version: 1
    fields:
      - {name: id, type: string, required: true}
      - {name: note, type: string}
      - {name: amount, type: float}
parser:
  format: kv
  pair_delimiter: " "
  key_value_delimiter: "="
  fields: {amount: amount}
mapper:
  schema: {name: order, version: 1}
"#;

pub const DEPRECATED_FIELDS: &str = r#"schemas:
  - name: order
    version: 1
    fields:
      - {name: id, type: string, required: true}
      - {name: coupon, type: string, required: true, deprecated: true}
      - {name: note, type: string, deprecated: true}
      - {name: channel, type: string, deprecated: true}
      - {name: amount, type: float}
parser:
  format: kv
  pair_delimiter: " "
  key_value_delimiter: "="
  fields: {id: id, note: note, total: total}
mapper:
  schema: {name: order, version: 1}
  rules:
    - {target: amount, from: total}
    - {target: channel, constant: web}
resp:
  schema: {name: order, version: 1}
"#;

pub const FIXED_ORDERS: &str = r#"
schemas:
  - name: order
    version: 1
    fields:
      - {name: order_id, type: string, required: true}
      - {name: status, type: string}
      - {name: amount, type: float}
parser:
  format: fixed_width
  padding: "."
  widths: [6, 8, 8]
  fields: {order_id: "0", status: "1", amount: "2"}
mapper:
  schema: {name: order, version: 1}
  rules:
    - target: order_id
      from: order_id
      via:
        - pad: {width: 8, fill: "0"}
    - target: status
      from: status
      via:
        - replace: {pairs: {paid: settled, open: pending}}
        - upper
resp:
  schema: {name: order, version: 1}
"#;

pub fn build_yaml(text: &str, registry: &Registry) -> Result<Specs, LoadError> {
    Config::from_yaml(text).expect("valid YAML").build(registry)
}

pub fn build_error(text: &str, registry: &Registry) -> LoadError {
    build_yaml(text, registry).expect_err("configuration is rejected")
}

pub fn location(err: &LoadError) -> (&LoadErrorKind, &str, Option<(usize, usize)>) {
    (
        err.kind(),
        err.path(),
        err.position().map(|p| (p.line(), p.column())),
    )
}

pub fn spec(error: SpecError) -> LoadErrorKind {
    LoadErrorKind::Spec(error)
}

pub fn invalid_argument(name: &str, value: &str) -> SpecError {
    SpecError::InvalidArgument {
        name: name.into(),
        value: value.into(),
    }
}

pub fn text(value: &str) -> Value<'static> {
    Value::Str(Cow::Owned(value.to_owned()))
}

#[derive(Debug)]
pub struct Pad {
    width: usize,
    fill: char,
}

impl Op for Pad {
    fn apply<'a>(&'a self, text: &'a str) -> Option<Cow<'a, str>> {
        let missing = self.width.saturating_sub(text.chars().count());
        if missing == 0 {
            return Some(Cow::Borrowed(text));
        }
        Some(Cow::Owned(
            std::iter::repeat_n(self.fill, missing)
                .chain(text.chars())
                .collect(),
        ))
    }

    fn name(&self) -> &str {
        "pad"
    }
}

pub fn pad(arguments: &mut Arguments) -> Result<Arc<dyn Op>, LoadError> {
    let width = arguments.integer("width")?;
    if width == 0 {
        return Err(arguments.spec_error("width", invalid_argument("pad.width", "0")));
    }
    let fill = if arguments.contains_key("fill") {
        arguments.character("fill")?
    } else {
        ' '
    };
    Ok(Arc::new(Pad { width, fill }))
}

#[derive(Debug)]
pub struct Upper;

impl Op for Upper {
    fn apply<'a>(&'a self, text: &'a str) -> Option<Cow<'a, str>> {
        Some(Cow::Owned(text.to_uppercase()))
    }

    fn name(&self) -> &str {
        "upper"
    }
}

#[derive(Debug)]
pub struct Replace(Vec<(String, String)>);

impl Op for Replace {
    fn apply<'a>(&'a self, text: &'a str) -> Option<Cow<'a, str>> {
        let replaced = self.0.iter().find(|(from, _)| from == text);
        Some(Cow::Borrowed(replaced.map_or(text, |(_, to)| to.as_str())))
    }

    fn name(&self) -> &str {
        "replace"
    }
}

pub fn replace(arguments: &mut Arguments) -> Result<Arc<dyn Op>, LoadError> {
    let pairs = arguments
        .map("pairs")?
        .into_iter()
        .map(|(from, to)| Ok((from.to_owned(), to.string()?)))
        .collect::<Result<_, LoadError>>()?;
    Ok(Arc::new(Replace(pairs)))
}

#[derive(Debug)]
pub struct FixedWidth {
    columns: Vec<Range<usize>>,
    padding: char,
}

impl Parser for FixedWidth {
    type Path = Range<usize>;

    fn name(&self) -> &str {
        "fixed_width"
    }

    fn path(&self, path: &str) -> Option<Range<usize>> {
        self.columns.get(path.parse::<usize>().ok()?).cloned()
    }

    fn read<'a>(
        &self,
        raw: &'a str,
        paths: &[Range<usize>],
        values: &mut [Option<Cow<'a, str>>],
    ) -> Result<(), Malformed> {
        for (path, value) in paths.iter().zip(values) {
            let column = raw
                .get(path.clone())
                .ok_or_else(|| Malformed::new("record is shorter than its columns"))?;
            *value = Some(Cow::Borrowed(column.trim_end_matches(self.padding)));
        }
        Ok(())
    }
}

pub fn fixed_width(arguments: &mut Arguments) -> Result<FixedWidth, LoadError> {
    let padding = if arguments.contains_key("padding") {
        arguments.character("padding")?
    } else {
        ' '
    };
    let mut columns = Vec::new();
    let mut start = 0;
    for width in arguments.list("widths")? {
        let size: usize = width.integer()?;
        if size == 0 {
            return Err(width.spec_error(invalid_argument("fixed_width.widths", "0")));
        }
        columns.push(start..start + size);
        start += size;
    }
    Ok(FixedWidth { columns, padding })
}

pub fn registry() -> Registry {
    Registry::new()
        .op("pad", pad)
        .and_then(|registry| registry.op("replace", replace))
        .and_then(|registry| {
            registry.op("upper", |_: &mut Arguments| {
                Ok(Arc::new(Upper) as Arc<dyn Op>)
            })
        })
        .and_then(|registry| registry.format("fixed_width", fixed_width))
        .expect("names are free")
}
