use moldette_core::{Field, OutputSchema, SpecError};
use moldette_mapper::{MapperSpec, Rule};
use moldette_parser::{Format, ParserSpec};
use moldette_resp::RespSpec;

use crate::error::{LoadError, LoadErrorKind};
use crate::model::{
    Config, FieldEntry, FormatEntry, MapperSection, OpEntry, ParserSection, RespSection, RuleEntry,
    SchemaEntry, SchemaRef, SourceEntry,
};
use crate::node::{index_path, key_path};
use crate::registry::Registry;

/// The specs built from a [`Config`]: every schema of `schemas`, and the spec
/// of each layer section present.
#[derive(Debug, Clone)]
pub struct Specs {
    schemas: Vec<OutputSchema>,
    parser: Option<ParserSpec>,
    mapper: Option<MapperSpec>,
    resp: Option<RespSpec>,
}

impl Specs {
    pub fn schemas(&self) -> &[OutputSchema] {
        &self.schemas
    }

    pub fn parser(&self) -> Option<&ParserSpec> {
        self.parser.as_ref()
    }

    pub fn mapper(&self) -> Option<&MapperSpec> {
        self.mapper.as_ref()
    }

    pub fn resp(&self) -> Option<&RespSpec> {
        self.resp.as_ref()
    }
}

impl Config {
    /// Calls the layer builders, one builder method per key (#3), with the
    /// caller-defined ops and formats of `registry`. An error of a layer
    /// builder is reported as [`LoadErrorKind::Spec`] at the most specific key
    /// path it can be traced to, without a position.
    pub fn build(&self, registry: &Registry) -> Result<Specs, LoadError> {
        let schemas = output_schemas(&self.schemas)?;
        Ok(Specs {
            parser: self
                .parser
                .as_ref()
                .map(|section| parser_spec(section, "parser", registry))
                .transpose()?,
            mapper: self
                .mapper
                .as_ref()
                .map(|section| mapper_spec(section, &schemas, registry))
                .transpose()?,
            resp: self
                .resp
                .as_ref()
                .map(|section| resp_spec(section, &schemas))
                .transpose()?,
            schemas,
        })
    }
}

fn spec_error(error: SpecError, path: String) -> LoadError {
    LoadError::new(LoadErrorKind::Spec(error), path, None)
}

fn output_schemas(entries: &[SchemaEntry]) -> Result<Vec<OutputSchema>, LoadError> {
    let mut schemas: Vec<OutputSchema> = Vec::with_capacity(entries.len());
    for (i, entry) in entries.iter().enumerate() {
        let path = index_path("schemas", i);
        if schemas
            .iter()
            .any(|schema| schema.name() == entry.name && schema.version() == entry.version)
        {
            return Err(LoadError::new(
                LoadErrorKind::DuplicateSchema {
                    name: entry.name.clone(),
                    version: entry.version,
                },
                path,
                entry.position,
            ));
        }
        let schema = entry
            .fields
            .iter()
            .fold(
                OutputSchema::builder(&entry.name, entry.version),
                |builder, field| builder.field(output_field(field)),
            )
            .build()
            .map_err(|err| {
                let fields_path = key_path(&path, "fields");
                let names = entry.fields.iter().enumerate().map(|(k, field)| {
                    (
                        field.name.as_str(),
                        key_path(&index_path(&fields_path, k), "name"),
                    )
                });
                let located = name_error_path(&err, names).unwrap_or(path);
                spec_error(err, located)
            })?;
        schemas.push(schema);
    }
    Ok(schemas)
}

fn output_field(entry: &FieldEntry) -> Field {
    let field = Field::new(&entry.name, entry.ty);
    let field = if entry.required {
        field.required()
    } else {
        field
    };
    if entry.deprecated {
        field.deprecated()
    } else {
        field
    }
}

fn name_error_path<'n>(
    err: &SpecError,
    names: impl IntoIterator<Item = (&'n str, String)>,
) -> Option<String> {
    let (target, occurrence) = match err {
        SpecError::EmptyFieldName => ("", 0),
        SpecError::DuplicateField(name) => (name.as_str(), 1),
        _ => return None,
    };
    names
        .into_iter()
        .filter(|(name, _)| *name == target)
        .nth(occurrence)
        .map(|(_, path)| path)
}

fn resolve<'s>(
    reference: &SchemaRef,
    path: &str,
    schemas: &'s [OutputSchema],
) -> Result<&'s OutputSchema, LoadError> {
    schemas
        .iter()
        .find(|schema| schema.name() == reference.name && schema.version() == reference.version)
        .ok_or_else(|| {
            LoadError::new(
                LoadErrorKind::UnknownSchema {
                    name: reference.name.clone(),
                    version: reference.version,
                },
                path.to_owned(),
                reference.position,
            )
        })
}

fn parser_spec(
    section: &ParserSection,
    path: &str,
    registry: &Registry,
) -> Result<ParserSpec, LoadError> {
    let format = match &section.format {
        FormatEntry::Json => Format::Json,
        FormatEntry::Delimited { delimiter } => Format::Delimited {
            delimiter: *delimiter,
        },
        FormatEntry::Regex { pattern } => Format::Regex {
            pattern: pattern.clone(),
        },
        FormatEntry::Kv {
            pair_delimiter,
            key_value_delimiter,
        } => Format::Kv {
            pair_delimiter: *pair_delimiter,
            key_value_delimiter: *key_value_delimiter,
        },
        FormatEntry::Custom { name, arguments } => registry.build_format(name, arguments)?,
    };
    let mut builder = section.fields.iter().fold(
        ParserSpec::builder(format),
        |builder, (name, field_path)| builder.field(name, field_path),
    );
    for (i, body) in section.bodies.iter().enumerate() {
        let body_spec = parser_spec(&body.parser, &body_path(path, i), registry)?;
        builder = builder.body(&body.path, body_spec);
    }
    if section.empty_as_none {
        builder = builder.empty_as_none();
    }
    builder.build().map_err(|err| {
        let located = parser_error_path(section, path, &err);
        spec_error(err, located)
    })
}

fn body_path(path: &str, index: usize) -> String {
    index_path(&key_path(path, "bodies"), index)
}

fn parser_error_path(section: &ParserSection, path: &str, err: &SpecError) -> String {
    let located = match err {
        SpecError::InvalidPath(invalid) => section
            .fields
            .iter()
            .find(|(_, field_path)| field_path == invalid)
            .map(|(name, _)| key_path(&key_path(path, "fields"), name))
            .or_else(|| {
                section
                    .bodies
                    .iter()
                    .position(|body| &body.path == invalid)
                    .map(|i| key_path(&body_path(path, i), "path"))
            }),
        SpecError::InvalidArgument { name, .. } => Some(key_path(path, name)),
        _ => name_error_path(err, parser_field_names(section, path)),
    };
    located.unwrap_or_else(|| path.to_owned())
}

fn parser_field_names<'s>(section: &'s ParserSection, path: &str) -> Vec<(&'s str, String)> {
    let fields_path = key_path(path, "fields");
    let mut names: Vec<_> = section
        .fields
        .iter()
        .map(|(name, _)| (name.as_str(), key_path(&fields_path, name)))
        .collect();
    for (i, body) in section.bodies.iter().enumerate() {
        names.extend(parser_field_names(&body.parser, &body_path(path, i)));
    }
    names
}

fn mapper_spec(
    section: &MapperSection,
    schemas: &[OutputSchema],
    registry: &Registry,
) -> Result<MapperSpec, LoadError> {
    let schema = resolve(&section.schema, "mapper.schema", schemas)?;
    let mut builder = MapperSpec::builder(schema.clone());
    for entry in &section.rules {
        builder = builder.rule(rule(entry, registry)?);
    }
    if section.explicit_rules {
        builder = builder.explicit_rules();
    }
    builder.build().map_err(|err| {
        let located = rule_error_path(&section.rules, schema, &err);
        spec_error(err, located)
    })
}

fn rule(entry: &RuleEntry, registry: &Registry) -> Result<Rule, LoadError> {
    let rule = match &entry.source {
        SourceEntry::From(from) => Rule::new(&entry.target, from),
        SourceEntry::Coalesce(from) => Rule::coalesce(&entry.target, from),
        SourceEntry::Constant(value) => Rule::constant(&entry.target, value),
    };
    let rule = match &entry.fallback {
        Some(from) => rule.fallback(from),
        None => rule,
    };
    entry.via.iter().try_fold(rule, |rule, op| {
        Ok(match op {
            OpEntry::Trim => rule.trim(),
            OpEntry::Split { separator, index } => rule.split(*separator, *index),
            OpEntry::Extract { pattern } => rule.extract(pattern),
            OpEntry::Lookup { table } => rule.lookup(table.iter().cloned()),
            OpEntry::Timestamp { formats } => rule.timestamp(formats),
            OpEntry::EmptyAsNone => rule.empty_as_none(),
            OpEntry::Custom { name, arguments } => rule.via(registry.build_op(name, arguments)?),
        })
    })
}

fn rule_error_path(rules: &[RuleEntry], schema: &OutputSchema, err: &SpecError) -> String {
    let rule_path = |i: usize, suffix: &str| key_path(&index_path("mapper.rules", i), suffix);
    let target_path = |target: &str, occurrence: usize| {
        rules
            .iter()
            .enumerate()
            .filter(|(_, rule)| rule.target == target)
            .nth(occurrence)
            .map(|(i, _)| rule_path(i, "target"))
    };
    let located = match err {
        SpecError::UnknownField(target) => target_path(target, 0),
        SpecError::DuplicateRule(target) => target_path(target, 1),
        SpecError::InvalidArgument { name, value } => rules_in_schema_order(rules, schema)
            .find_map(|i| {
                let suffix = rules
                    .get(i)
                    .and_then(|rule| argument_path(rule, name, value))?;
                Some(rule_path(i, &suffix))
            }),
        _ => None,
    };
    located.unwrap_or_else(|| "mapper".to_owned())
}

fn rules_in_schema_order<'r>(
    rules: &'r [RuleEntry],
    schema: &'r OutputSchema,
) -> impl Iterator<Item = usize> + 'r {
    schema
        .fields()
        .iter()
        .filter_map(|field| rules.iter().position(|rule| rule.target == field.name()))
}

fn argument_path(rule: &RuleEntry, name: &str, value: &str) -> Option<String> {
    if name == "coalesce.from" {
        return matches!(&rule.source, SourceEntry::Coalesce(from) if from.is_empty())
            .then(|| "coalesce".to_owned());
    }
    rule.via.iter().enumerate().find_map(|(j, op)| {
        let argument = key_path(&index_path("via", j), name);
        match op {
            OpEntry::Extract { pattern } if name == "extract.pattern" && pattern == value => {
                Some(argument)
            }
            OpEntry::Timestamp { formats } if name == "timestamp.formats" => {
                if formats.is_empty() {
                    Some(argument)
                } else {
                    formats
                        .iter()
                        .position(|format| format == value)
                        .map(|k| index_path(&argument, k))
                }
            }
            _ => None,
        }
    })
}

fn resp_spec(section: &RespSection, schemas: &[OutputSchema]) -> Result<RespSpec, LoadError> {
    let builder = RespSpec::builder(resolve(&section.schema, "resp.schema", schemas)?.clone());
    Ok(if section.strict {
        builder.strict()
    } else {
        builder
    }
    .build())
}
