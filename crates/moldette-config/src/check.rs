use std::collections::HashSet;

use crate::diagnostic::{Diagnostic, DiagnosticKind, Report};
use crate::fields::parser_field_names;
use crate::model::{Config, MapperSection, RuleEntry, SchemaEntry, SourceEntry};
use crate::node::{index_path, key_path};
use crate::position::Position;

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
            diagnostics.push(Diagnostic::new(
                DiagnosticKind::SchemaMismatch {
                    name: mapper.schema.name.clone(),
                    version: mapper.schema.version,
                },
                "resp.schema".to_owned(),
                resp.schema.position,
            ));
        }
        Report::new(diagnostics)
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
        diagnostics.push(Diagnostic::new(
            kind(field.name.clone()),
            index_path(&fields_path, k),
            field.position,
        ));
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
            diagnostics.push(Diagnostic::new(
                DiagnosticKind::UnreadField((*name).to_owned()),
                path.clone(),
                *position,
            ));
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
            diagnostics.push(Diagnostic::new(
                DiagnosticKind::DeprecatedField(rule.target.clone()),
                key_path(&index_path("mapper.rules", i), "target"),
                rule.target_position,
            ));
        }
        let Some(outputs) = outputs else {
            continue;
        };
        for (name, path, position) in rule_sources(rule, i) {
            if !outputs_contain(outputs, name) {
                diagnostics.push(Diagnostic::new(
                    DiagnosticKind::UnknownSourceField(name.to_owned()),
                    path,
                    position,
                ));
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
