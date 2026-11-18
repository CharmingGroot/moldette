use std::borrow::Cow;
use std::fmt;
use std::panic::RefUnwindSafe;
use std::sync::Arc;

use moldette_core::{Malformed, SpecError};

use crate::delimited::without_line_break;

/// A record format defined by the caller and used with
/// [`Format::custom`](crate::Format::custom).
///
/// Paths are resolved when the spec is built, so every field the spec reads
/// stays declared (#4). A panic is not caught and reaches the caller of
/// [`ParserSpec::parse`](crate::ParserSpec::parse) (#15).
pub trait Parser: fmt::Debug + Send + Sync + RefUnwindSafe {
    /// A field location resolved from the text of a path.
    type Path: fmt::Debug + Send + Sync + RefUnwindSafe;

    /// The name of the format, used by configuration and `tracing` signals.
    fn name(&self) -> &str;

    /// Resolves `path`, or returns `None` when it names no field of this
    /// format, which fails the build with [`SpecError::InvalidPath`].
    fn path(&self, path: &str) -> Option<Self::Path>;

    /// Reads one record without its trailing line break and stores the value
    /// at `paths[i]` in `values[i]`, leaving absent values `None`.
    fn read<'a>(
        &self,
        raw: &'a str,
        paths: &[Self::Path],
        values: &mut [Option<Cow<'a, str>>],
    ) -> Result<(), Malformed>;
}

/// The [`Parser`] held by [`Format::Custom`](crate::Format::Custom). Two
/// formats are equal only when they share one parser instance.
#[derive(Debug, Clone)]
pub struct CustomFormat(Arc<dyn ResolvePaths>);

impl CustomFormat {
    pub(crate) fn new(parser: impl Parser + 'static) -> Self {
        Self(Arc::new(parser))
    }

    pub(crate) fn resolve(&self, paths: &[String]) -> Result<CustomPaths, SpecError> {
        Arc::clone(&self.0).resolve(paths)
    }
}

impl PartialEq for CustomFormat {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for CustomFormat {}

trait ResolvePaths: fmt::Debug + Send + Sync + RefUnwindSafe {
    fn resolve(self: Arc<Self>, paths: &[String]) -> Result<CustomPaths, SpecError>;
}

impl<P: Parser + 'static> ResolvePaths for P {
    fn resolve(self: Arc<Self>, paths: &[String]) -> Result<CustomPaths, SpecError> {
        let paths = paths
            .iter()
            .map(|path| {
                self.path(path)
                    .ok_or_else(|| SpecError::InvalidPath(path.clone()))
            })
            .collect::<Result<_, _>>()?;
        Ok(CustomPaths(Arc::new(Resolved {
            parser: self,
            paths,
        })))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CustomPaths(Arc<dyn FindValues>);

impl CustomPaths {
    pub(crate) fn find<'a>(&self, raw: &'a str) -> Result<Vec<Option<Cow<'a, str>>>, Malformed> {
        self.0.find(without_line_break(raw))
    }
}

trait FindValues: fmt::Debug + Send + Sync + RefUnwindSafe {
    fn find<'a>(&self, raw: &'a str) -> Result<Vec<Option<Cow<'a, str>>>, Malformed>;
}

#[derive(Debug)]
struct Resolved<P: Parser> {
    parser: Arc<P>,
    paths: Vec<P::Path>,
}

impl<P: Parser> FindValues for Resolved<P> {
    fn find<'a>(&self, raw: &'a str) -> Result<Vec<Option<Cow<'a, str>>>, Malformed> {
        let mut values = vec![None; self.paths.len()];
        self.parser.read(raw, &self.paths, &mut values)?;
        Ok(values)
    }
}
