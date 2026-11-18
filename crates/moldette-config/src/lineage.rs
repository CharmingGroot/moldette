use std::slice;

use crate::model::{Config, FormatEntry, OpEntry, ParserSection, RuleEntry, SourceEntry};

/// Where each field of the mapper's output schema comes from, computed from
/// the configuration model when the specs are built (#4, #14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lineage {
    schema: String,
    version: u32,
    fields: Vec<LineageField>,
}

impl Lineage {
    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    /// Fields in output schema order.
    pub fn fields(&self) -> &[LineageField] {
        &self.fields
    }

    pub fn field(&self, name: &str) -> Option<&LineageField> {
        self.fields.iter().find(|field| field.name == name)
    }
}

/// The source of one output field, the parser fields it reads in the order
/// they are tried and the names of the ops applied to each of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineageField {
    name: String,
    source: LineageSource,
    reads: Vec<LineageRead>,
    ops: Vec<String>,
}

impl LineageField {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn source(&self) -> &LineageSource {
        &self.source
    }

    /// The fields of `from` or `coalesce`, or the field of the same name,
    /// followed by the `fallback`.
    pub fn reads(&self) -> &[LineageRead] {
        &self.reads
    }

    /// The op names of `via` as written in the configuration.
    pub fn ops(&self) -> &[String] {
        &self.ops
    }
}

/// How an output field takes its value.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LineageSource {
    From,
    Coalesce,
    /// The value of `constant`, which reads no record.
    Constant(String),
    /// No rule, so the parser field of the same name is read.
    SameName,
    /// No rule and `explicit_rules`, so the field is always absent.
    Absent,
}

/// One parser field read for an output field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineageRead {
    field: String,
    fallback: bool,
    origin: Vec<LineagePath>,
}

impl LineageRead {
    pub fn field(&self) -> &str {
        &self.field
    }

    pub fn is_fallback(&self) -> bool {
        self.fallback
    }

    /// The paths that lead from the raw record to the field: the path of
    /// each enclosing body, then the field path. Empty when the parser
    /// section is absent or declares no such field.
    pub fn origin(&self) -> &[LineagePath] {
        &self.origin
    }
}

/// A path in the notation of one parser format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineagePath {
    format: String,
    path: String,
}

impl LineagePath {
    /// `json`, `delimited`, `regex`, `kv` or the name of a caller-defined
    /// format.
    pub fn format(&self) -> &str {
        &self.format
    }

    /// A JSON Pointer, column index, capture group name, key or
    /// caller-defined path, as written in the configuration.
    pub fn path(&self) -> &str {
        &self.path
    }
}

type Origins<'c> = Vec<(&'c str, Vec<LineagePath>)>;

pub(crate) fn lineage(config: &Config) -> Option<Lineage> {
    let section = config.mapper.as_ref()?;
    let schema = config.schemas.iter().find(|entry| {
        entry.name == section.schema.name && entry.version == section.schema.version
    })?;
    let origins = config
        .parser
        .as_ref()
        .map(|parser| parser_origins(parser, &[]))
        .unwrap_or_default();
    let fields = schema
        .fields
        .iter()
        .map(
            |field| match section.rules.iter().find(|rule| rule.target == field.name) {
                Some(rule) => rule_lineage(rule, &origins),
                None if section.explicit_rules => LineageField {
                    name: field.name.clone(),
                    source: LineageSource::Absent,
                    reads: Vec::new(),
                    ops: Vec::new(),
                },
                None => LineageField {
                    name: field.name.clone(),
                    source: LineageSource::SameName,
                    reads: vec![read(&field.name, false, &origins)],
                    ops: Vec::new(),
                },
            },
        )
        .collect();
    Some(Lineage {
        schema: schema.name.clone(),
        version: schema.version,
        fields,
    })
}

fn parser_origins<'c>(section: &'c ParserSection, enclosing: &[LineagePath]) -> Origins<'c> {
    let format = format_name(&section.format);
    let origin = |path: &str| {
        let mut origin = enclosing.to_vec();
        origin.push(LineagePath {
            format: format.to_owned(),
            path: path.to_owned(),
        });
        origin
    };
    let mut origins: Origins<'c> = section
        .fields
        .iter()
        .map(|(name, path)| (name.as_str(), origin(path)))
        .collect();
    for body in &section.bodies {
        origins.extend(parser_origins(&body.parser, &origin(&body.path)));
    }
    origins
}

fn rule_lineage(rule: &RuleEntry, origins: &Origins<'_>) -> LineageField {
    let (source, primary) = match &rule.source {
        SourceEntry::From(name) => (LineageSource::From, slice::from_ref(name)),
        SourceEntry::Coalesce(names) => (LineageSource::Coalesce, names.as_slice()),
        SourceEntry::Constant(value) => (LineageSource::Constant(value.clone()), &[][..]),
    };
    let reads = primary
        .iter()
        .map(|name| read(name, false, origins))
        .chain(rule.fallback.iter().map(|name| read(name, true, origins)))
        .collect();
    LineageField {
        name: rule.target.clone(),
        source,
        reads,
        ops: rule.via.iter().map(|op| op_name(op).to_owned()).collect(),
    }
}

fn read(field: &str, fallback: bool, origins: &Origins<'_>) -> LineageRead {
    LineageRead {
        field: field.to_owned(),
        fallback,
        origin: origins
            .iter()
            .find(|(name, _)| *name == field)
            .map(|(_, origin)| origin.clone())
            .unwrap_or_default(),
    }
}

fn format_name(format: &FormatEntry) -> &str {
    match format {
        FormatEntry::Json => "json",
        FormatEntry::Delimited { .. } => "delimited",
        FormatEntry::Regex { .. } => "regex",
        FormatEntry::Kv { .. } => "kv",
        FormatEntry::Custom { name, .. } => name,
    }
}

fn op_name(op: &OpEntry) -> &str {
    match op {
        OpEntry::Trim => "trim",
        OpEntry::Split { .. } => "split",
        OpEntry::Extract { .. } => "extract",
        OpEntry::Lookup { .. } => "lookup",
        OpEntry::Timestamp { .. } => "timestamp",
        OpEntry::EmptyAsNone => "empty_as_none",
        OpEntry::Custom { name, .. } => name,
    }
}
