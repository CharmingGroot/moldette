use std::fmt;

/// A one-based line and column in the configuration text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    line: usize,
    column: usize,
}

impl Position {
    pub(crate) fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }

    pub fn line(&self) -> usize {
        self.line
    }

    pub fn column(&self) -> usize {
        self.column
    }
}

pub(crate) fn write_located(
    f: &mut fmt::Formatter<'_>,
    path: &str,
    message: &dyn fmt::Display,
    position: Option<Position>,
) -> fmt::Result {
    if !path.is_empty() {
        write!(f, "{path}: ")?;
    }
    write!(f, "{message}")?;
    if let Some(position) = position {
        write!(f, " at line {}, column {}", position.line, position.column)?;
    }
    Ok(())
}
