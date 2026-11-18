use std::fmt;

use moldette_core::SpecError;

use crate::diagnostic::{Diagnostic, DiagnosticKind};
use crate::position::{Position, write_located};

/// An error found while reading a configuration, with the key path and, when
/// the reader reports one, the position of the offending node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadError {
    kind: LoadErrorKind,
    path: String,
    position: Option<Position>,
}

impl LoadError {
    pub(crate) fn new(kind: LoadErrorKind, path: String, position: Option<Position>) -> Self {
        Self {
            kind,
            path,
            position,
        }
    }

    pub(crate) fn from_diagnostic(diagnostic: &Diagnostic) -> Self {
        Self::new(
            LoadErrorKind::Diagnostic(diagnostic.kind().clone()),
            diagnostic.path().to_owned(),
            diagnostic.position(),
        )
    }

    pub fn kind(&self) -> &LoadErrorKind {
        &self.kind
    }

    /// The key path of the node, such as `mapper.rules[2].via[0].split.index`.
    /// Empty for the document itself.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The position of the node. JSON reports positions for syntax errors only.
    pub fn position(&self) -> Option<Position> {
        self.position
    }
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_located(f, &self.path, &self.kind, self.position)
    }
}

impl std::error::Error for LoadError {}

/// What is wrong with the configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LoadErrorKind {
    /// The text is not valid YAML or JSON; holds the message of the reader.
    Syntax(String),
    MultipleDocuments,
    AnchorOrAlias,
    Tag,
    NestingTooDeep,
    NonScalarKey,
    DuplicateKey(String),
    UnknownKey(String),
    MissingKey(String),
    /// None of the mutually exclusive keys is present.
    MissingOneOf(Vec<String>),
    /// Two mutually exclusive keys are present.
    ConflictingKeys(String, String),
    /// The node has another kind than the one expected.
    TypeMismatch(ValueKind),
    IntegerOutOfRange(String),
    UnknownFieldType(String),
    /// No entry of `schemas` has this name and version.
    UnknownSchema {
        name: String,
        version: u32,
    },
    /// Another entry of `schemas` has the same name and version.
    DuplicateSchema {
        name: String,
        version: u32,
    },
    /// The op is neither built in nor in the [`Registry`](crate::Registry).
    UnknownOp(String),
    /// The parser format is neither built in nor in the
    /// [`Registry`](crate::Registry).
    UnknownFormat(String),
    /// The op or parser built by the constructor registered under one name
    /// reports another name, which `explain` and [`Lineage`](crate::Lineage)
    /// would show apart.
    NameMismatch {
        registered: String,
        reported: String,
    },
    /// A layer builder or a registered constructor rejected the spec.
    Spec(SpecError),
    /// The first error of the [`Report`](crate::Report) of
    /// [`Config::check`](crate::Config::check).
    Diagnostic(DiagnosticKind),
}

impl fmt::Display for LoadErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(message) => write!(f, "syntax error: {message}"),
            Self::MultipleDocuments => f.write_str("more than one YAML document"),
            Self::AnchorOrAlias => f.write_str("YAML anchors and aliases are not allowed"),
            Self::Tag => f.write_str("YAML tags are not allowed"),
            Self::NestingTooDeep => f.write_str("nesting is too deep"),
            Self::NonScalarKey => f.write_str("a key must be a scalar"),
            Self::DuplicateKey(key) => write!(f, "key `{key}` appears more than once"),
            Self::UnknownKey(key) => write!(f, "unknown key `{key}`"),
            Self::MissingKey(key) => write!(f, "missing key `{key}`"),
            Self::MissingOneOf(keys) => write!(f, "missing one of `{}`", keys.join("`, `")),
            Self::ConflictingKeys(first, second) => {
                write!(f, "keys `{first}` and `{second}` cannot be used together")
            }
            Self::TypeMismatch(expected) => write!(f, "expected {expected}"),
            Self::IntegerOutOfRange(text) => write!(f, "integer `{text}` is out of range"),
            Self::UnknownFieldType(name) => write!(f, "unknown field type `{name}`"),
            Self::UnknownSchema { name, version } => {
                write!(f, "schema `{name}` version {version} is not defined")
            }
            Self::DuplicateSchema { name, version } => {
                write!(
                    f,
                    "schema `{name}` version {version} is defined more than once"
                )
            }
            Self::UnknownOp(name) => write!(f, "unknown op `{name}`"),
            Self::UnknownFormat(name) => write!(f, "unknown format `{name}`"),
            Self::NameMismatch {
                registered,
                reported,
            } => write!(f, "registered as `{registered}` but named `{reported}`"),
            Self::Spec(error) => write!(f, "{error}"),
            Self::Diagnostic(kind) => write!(f, "{kind}"),
        }
    }
}

/// The kind of node a key expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ValueKind {
    Map,
    Sequence,
    String,
    Bool,
    Integer,
    Character,
    /// An op name, or a map from one op name to its arguments.
    Op,
}

impl fmt::Display for ValueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Map => "a map",
            Self::Sequence => "a sequence",
            Self::String => "a string",
            Self::Bool => "a bool",
            Self::Integer => "a non-negative integer",
            Self::Character => "a single character",
            Self::Op => "an op name or a map with one op name",
        })
    }
}
