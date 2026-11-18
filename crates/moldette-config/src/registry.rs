use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use moldette_mapper::Op;
use moldette_parser::Format;

use crate::arguments::Arguments;
use crate::error::{LoadError, LoadErrorKind};

type OpConstructor = dyn Fn(&mut Arguments) -> Result<Arc<dyn Op>, LoadError> + Send + Sync;
type FormatConstructor = dyn Fn(&mut Arguments) -> Result<Format, LoadError> + Send + Sync;

const BUILT_IN_OPS: [&str; 6] = [
    "trim",
    "split",
    "extract",
    "lookup",
    "timestamp",
    "empty_as_none",
];
const BUILT_IN_FORMATS: [&str; 4] = ["json", "delimited", "regex", "kv"];

/// Constructors of the caller-defined ops and formats a configuration names.
/// It is passed to [`Config::build`](crate::Config::build); nothing is
/// registered globally (#2).
#[derive(Default)]
pub struct Registry {
    ops: BTreeMap<String, Box<OpConstructor>>,
    formats: BTreeMap<String, Box<FormatConstructor>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers the constructor of the op named `name` in `via`. Fails when
    /// `name` is a built-in op or is already registered.
    pub fn op(
        mut self,
        name: impl Into<String>,
        constructor: impl Fn(&mut Arguments) -> Result<Arc<dyn Op>, LoadError> + Send + Sync + 'static,
    ) -> Result<Self, RegistryError> {
        insert(
            &mut self.ops,
            &BUILT_IN_OPS,
            name.into(),
            Box::new(constructor),
        )?;
        Ok(self)
    }

    /// Registers the constructor of the parser format named `name` in
    /// `format`. Fails when `name` is a built-in format or is already
    /// registered.
    pub fn format(
        mut self,
        name: impl Into<String>,
        constructor: impl Fn(&mut Arguments) -> Result<Format, LoadError> + Send + Sync + 'static,
    ) -> Result<Self, RegistryError> {
        insert(
            &mut self.formats,
            &BUILT_IN_FORMATS,
            name.into(),
            Box::new(constructor),
        )?;
        Ok(self)
    }

    pub(crate) fn build_op(
        &self,
        name: &str,
        arguments: &Arguments,
    ) -> Result<Arc<dyn Op>, LoadError> {
        construct(
            self.ops.get(name).map(Box::as_ref),
            arguments,
            LoadErrorKind::UnknownOp(name.to_owned()),
        )
    }

    pub(crate) fn build_format(
        &self,
        name: &str,
        arguments: &Arguments,
    ) -> Result<Format, LoadError> {
        construct(
            self.formats.get(name).map(Box::as_ref),
            arguments,
            LoadErrorKind::UnknownFormat(name.to_owned()),
        )
    }
}

impl fmt::Debug for Registry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Registry")
            .field("ops", &self.ops.keys().collect::<Vec<_>>())
            .field("formats", &self.formats.keys().collect::<Vec<_>>())
            .finish()
    }
}

fn insert<C: ?Sized>(
    table: &mut BTreeMap<String, Box<C>>,
    built_in: &[&str],
    name: String,
    constructor: Box<C>,
) -> Result<(), RegistryError> {
    if built_in.contains(&name.as_str()) {
        return Err(RegistryError::BuiltInName(name));
    }
    if table.contains_key(&name) {
        return Err(RegistryError::DuplicateName(name));
    }
    table.insert(name, constructor);
    Ok(())
}

fn construct<T, C: Fn(&mut Arguments) -> Result<T, LoadError> + ?Sized>(
    constructor: Option<&C>,
    arguments: &Arguments,
    unknown: LoadErrorKind,
) -> Result<T, LoadError> {
    let constructor = constructor.ok_or_else(|| arguments.error(unknown))?;
    let mut arguments = arguments.unread();
    let value = constructor(&mut arguments)?;
    arguments.reject_unread()?;
    Ok(value)
}

/// A name that cannot be registered in a [`Registry`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RegistryError {
    /// The name of a built-in op or format, which a configuration always
    /// resolves to the built-in one.
    BuiltInName(String),
    DuplicateName(String),
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BuiltInName(name) => write!(f, "`{name}` is a built-in name"),
            Self::DuplicateName(name) => write!(f, "`{name}` is registered more than once"),
        }
    }
}

impl std::error::Error for RegistryError {}
