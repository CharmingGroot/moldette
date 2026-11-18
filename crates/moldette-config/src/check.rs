use std::collections::HashSet;
use std::fmt;

use crate::error::{LoadError, LoadErrorKind, Position, write_located};
use crate::lower::parser_field_names;
use crate::model::{Config, MapperSection, RuleEntry, SchemaEntry, SourceEntry};
use crate::node::{index_path, key_path};

/// The diagnostics of [`Config::check`], in the order of the sections
/// `schemas`, `parser`, `mapper` and `resp`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    diagnostics: Vec<Diagnostic>,
}

impl Report {
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn has_errors(&self) -> bool {
        self.first_error().is_some()
    }

    pub(crate) fn first_error(&self) -> Option<&Diagnostic> {
        self.diagnostics
            .iter()
            .find(|diagnostic| diagnostic.severity() == Severity::Error)
    }
}

/// A problem between sections, with the key path and, for YAML, the position
/// of the node it concerns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    kind: DiagnosticKind,
    path: String,
    position: Option<Position>,
}

impl Diagnostic {
    pub fn kind(&self) -> &DiagnosticKind {
        &self.kind
    }

    pub fn severity(&self) -> Severity {
        match self.kind {
            DiagnosticKind::UnknownSourceField(_)
            | DiagnosticKind::AbsentRequiredField(_)
            | DiagnosticKind::SchemaMismatch { .. } => Severity::Error,
            DiagnosticKind::AbsentField(_)
            | DiagnosticKind::UnreadField(_)
            | DiagnosticKind::DeprecatedField(_) => Severity::Warning,
        }
    }

    /// The key path of the node, such as `mapper.rules[0].coalesce[1]`.
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn position(&self) -> Option<Position> {
        self.position
    }

    pub(crate) fn to_load_error(&self) -> LoadError {
        LoadError::new(
            LoadErrorKind::Diagnostic(self.kind.clone()),
            self.path.clone(),
            self.position,
        )
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_located(f, &self.path, &self.kind, self.position)
    }
}

/// Whether a diagnostic fails [`Config::build`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// What is wrong between sections.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DiagnosticKind {
    /// A `from`, `coalesce` or `fallback` names a field the parser never
    /// outputs. An error.
    UnknownSourceField(String),
    /// A required schema field has no rule and no parser field of the same
    /// name to read, so every record misses it. An error.
    AbsentRequiredField(String),
    /// An optional schema field has no rule and no parser field of the same
    /// name to read. A warning. Deprecated fields are reported as neither
    /// absent nor required.
    AbsentField(String),
    /// No rule reads the parser field. A warning.
    UnreadField(String),
    /// A deprecated schema field is the target of a rule, or is mapped from
    /// the parser field of the same name. A warning.
    DeprecatedField(String),
    /// The resp section references another schema than the one the mapper
    /// section maps to, given here. An error.
    SchemaMismatch { name: String, version: u32 },
}

impl fmt::Display for DiagnosticKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownSourceField(name) => write!(f, "the parser outputs no field `{name}`"),
            Self::AbsentRequiredField(name) => {
                write!(f, "required field `{name}` has no source in any record")
            }
            Self::AbsentField(name) => write!(f, "field `{name}` has no source in any record"),
            Self::UnreadField(name) => write!(f, "no rule reads field `{name}`"),
            Self::DeprecatedField(name) => write!(f, "deprecated field `{name}` is mapped"),
            Self::SchemaMismatch { name, version } => {
                write!(f, "the mapper maps to schema `{name}` version {version}")
            }
        }
    }
}

type Declared<'c> = (&'c str, String, Option<Position>);

impl Config {
    /// Checks the sections against each other on the model alone, without a
    /// [`Registry`](crate::Registry). A check runs only when every section it
    /// reads is present (#14); a mapper section referencing an undefined
    /// schema is left to [`Config::build`].
    pub fn check(&self) -> Report {
        let mut diagnostics = Vec::new();
        let outputs = self
            .parser
            .as_ref()
            .map(|section| parser_field_names(section, "parser"));
        let mapper = self.mapper.as_ref().and_then(|section| {
            self.schemas
                .iter()
                .enumerate()
                .find(|(_, entry)| {
                    entry.name == section.schema.name && entry.version == section.schema.version
                })
                .map(|(i, entry)| (section, i, entry))
        });
        if let Some((section, i, entry)) = mapper {
            fields_without_rules(section, i, entry, outputs.as_deref(), &mut diagnostics);
        }
        if let (Some(outputs), Some((section, _, entry))) = (&outputs, mapper) {
            unread_fields(section, entry, outputs, &mut diagnostics);
        }
        if let Some((section, _, entry)) = mapper {
            rule_targets_and_sources(section, entry, outputs.as_deref(), &mut diagnostics);
        }
        let schemas = self.mapper.as_ref().zip(self.resp.as_ref());
        if let Some((mapper, resp)) = schemas.filter(|(mapper, resp)| mapper.schema != resp.schema)
        {
            diagnostics.push(Diagnostic {
                kind: DiagnosticKind::SchemaMismatch {
                    name: mapper.schema.name.clone(),
                    version: mapper.schema.version,
                },
                path: "resp.schema".to_owned(),
                position: resp.schema.position,
            });
        }
        Report { diagnostics }
    }
}

fn outputs_contain(outputs: &[Declared<'_>], name: &str) -> bool {
    outputs.iter().any(|(output, _, _)| *output == name)
}

fn has_rule(section: &MapperSection, name: &str) -> bool {
    section.rules.iter().any(|rule| rule.target == name)
}

fn fields_without_rules(
    section: &MapperSection,
    schema_index: usize,
    entry: &SchemaEntry,
    outputs: Option<&[Declared<'_>]>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let fields_path = key_path(&index_path("schemas", schema_index), "fields");
    for (k, field) in entry.fields.iter().enumerate() {
        if has_rule(section, &field.name) {
            continue;
        }
        let produced = outputs.map(|outputs| outputs_contain(outputs, &field.name));
        let kind = match (field.deprecated, section.explicit_rules, produced) {
            (true, false, Some(true)) => DiagnosticKind::DeprecatedField,
            (true, ..) => continue,
            (false, true, _) | (false, false, Some(false)) if field.required => {
                DiagnosticKind::AbsentRequiredField
            }
            (false, false, Some(false)) => DiagnosticKind::AbsentField,
            _ => continue,
        };
        diagnostics.push(Diagnostic {
            kind: kind(field.name.clone()),
            path: index_path(&fields_path, k),
            position: field.position,
        });
    }
}

fn unread_fields(
    section: &MapperSection,
    entry: &SchemaEntry,
    outputs: &[Declared<'_>],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut read: HashSet<&str> = section
        .rules
        .iter()
        .enumerate()
        .flat_map(|(i, rule)| rule_sources(rule, i))
        .map(|(name, _, _)| name)
        .collect();
    if !section.explicit_rules {
        read.extend(
            entry
                .fields
                .iter()
                .filter(|field| !has_rule(section, &field.name))
                .map(|field| field.name.as_str()),
        );
    }
    for (name, path, position) in outputs {
        if !read.contains(name) {
            diagnostics.push(Diagnostic {
                kind: DiagnosticKind::UnreadField((*name).to_owned()),
                path: path.clone(),
                position: *position,
            });
        }
    }
}

fn rule_targets_and_sources(
    section: &MapperSection,
    entry: &SchemaEntry,
    outputs: Option<&[Declared<'_>]>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (i, rule) in section.rules.iter().enumerate() {
        let deprecated = entry
            .fields
            .iter()
            .any(|field| field.name == rule.target && field.deprecated);
        if deprecated {
            diagnostics.push(Diagnostic {
                kind: DiagnosticKind::DeprecatedField(rule.target.clone()),
                path: key_path(&index_path("mapper.rules", i), "target"),
                position: rule.target_position,
            });
        }
        let Some(outputs) = outputs else {
            continue;
        };
        for (name, path, position) in rule_sources(rule, i) {
            if !outputs_contain(outputs, name) {
                diagnostics.push(Diagnostic {
                    kind: DiagnosticKind::UnknownSourceField(name.to_owned()),
                    path,
                    position,
                });
            }
        }
    }
}

fn rule_sources(rule: &RuleEntry, index: usize) -> Vec<Declared<'_>> {
    let rule_path = index_path("mapper.rules", index);
    let position = |j: usize| rule.source_positions.get(j).copied().flatten();
    let mut sources: Vec<Declared<'_>> = match &rule.source {
        SourceEntry::From(name) => vec![(name.as_str(), key_path(&rule_path, "from"), position(0))],
        SourceEntry::Coalesce(names) => {
            let coalesce_path = key_path(&rule_path, "coalesce");
            names
                .iter()
                .enumerate()
                .map(|(j, name)| (name.as_str(), index_path(&coalesce_path, j), position(j)))
                .collect()
        }
        SourceEntry::Constant(_) => Vec::new(),
    };
    if let Some(fallback) = &rule.fallback {
        sources.push((
            fallback.as_str(),
            key_path(&rule_path, "fallback"),
            rule.fallback_position,
        ));
    }
    sources
}
