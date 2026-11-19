use std::fmt;

use crate::position::{Position, write_located};

/// The diagnostics of [`Config::check`](crate::Config::check), in the order
/// of the sections `schemas`, `parser`, `mapper` and `resp`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    diagnostics: Vec<Diagnostic>,
}

impl Report {
    pub(crate) fn new(diagnostics: Vec<Diagnostic>) -> Self {
        Self { diagnostics }
    }

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
    pub(crate) fn new(kind: DiagnosticKind, path: String, position: Option<Position>) -> Self {
        Self {
            kind,
            path,
            position,
        }
    }

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
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_located(f, &self.path, &self.kind, self.position)
    }
}

/// Whether a diagnostic fails [`Config::build`](crate::Config::build).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
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
