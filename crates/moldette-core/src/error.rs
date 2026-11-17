use std::fmt;

/// An error found while building a spec. Data errors are never reported here.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SpecError {
    EmptyFieldName,
    DuplicateField(String),
    UnknownField(String),
    DuplicateRule(String),
}

impl fmt::Display for SpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyFieldName => f.write_str("field name is empty"),
            Self::DuplicateField(name) => write!(f, "field `{name}` is declared more than once"),
            Self::UnknownField(name) => write!(f, "field `{name}` is not in the output schema"),
            Self::DuplicateRule(name) => write!(f, "field `{name}` has more than one rule"),
        }
    }
}

impl std::error::Error for SpecError {}
