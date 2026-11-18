use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use moldette_mapper::Op;
use moldette_parser::{Format, Parser};

use crate::arguments::Arguments;
use crate::error::{LoadError, LoadErrorKind};
use crate::model::{BUILT_IN_FORMATS, BUILT_IN_OPS, built_in_keys};

type OpConstructor = dyn Fn(&mut Arguments) -> Result<Arc<dyn Op>, LoadError> + Send + Sync;
type FormatConstructor =
    dyn Fn(&mut Arguments) -> Result<(String, Format), LoadError> + Send + Sync;

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
    /// `name` is a built-in op or is already registered; the build fails when
    /// [`Op::name`] of the constructed op is not `name`.
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

    /// Registers the constructor of the parser named `name` in `format`.
    /// Fails when `name` is a built-in format or is already registered; the
    /// build fails when [`Parser::name`] of the constructed parser is not
    /// `name`.
    pub fn format<P: Parser + 'static>(
        mut self,
        name: impl Into<String>,
        constructor: impl Fn(&mut Arguments) -> Result<P, LoadError> + Send + Sync + 'static,
    ) -> Result<Self, RegistryError> {
        insert(
            &mut self.formats,
            &BUILT_IN_FORMATS,
            name.into(),
            Box::new(move |arguments: &mut Arguments| {
                let parser = constructor(arguments)?;
                Ok((parser.name().to_owned(), Format::custom(parser)))
            }),
        )?;
        Ok(self)
    }

    pub(crate) fn build_op(
        &self,
        name: &str,
        arguments: &Arguments,
    ) -> Result<Arc<dyn Op>, LoadError> {
        let op = construct(
            self.ops.get(name).map(Box::as_ref),
            arguments,
            LoadErrorKind::UnknownOp(name.to_owned()),
        )?;
        if op.name() != name {
            return Err(arguments.error(name_mismatch(name, op.name())));
        }
        Ok(op)
    }

    pub(crate) fn build_format(
        &self,
        name: &str,
        arguments: &Arguments,
    ) -> Result<Format, LoadError> {
        let (reported, format) = construct(
            self.formats.get(name).map(Box::as_ref),
            arguments,
            LoadErrorKind::UnknownFormat(name.to_owned()),
        )?;
        if reported != name {
            return Err(arguments.key_error("format", name_mismatch(name, &reported)));
        }
        Ok(format)
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

fn name_mismatch(registered: &str, reported: &str) -> LoadErrorKind {
    LoadErrorKind::NameMismatch {
        registered: registered.to_owned(),
        reported: reported.to_owned(),
    }
}

fn insert<C: ?Sized>(
    table: &mut BTreeMap<String, Box<C>>,
    built_in: &[(&'static str, &'static [&'static str])],
    name: String,
    constructor: Box<C>,
) -> Result<(), RegistryError> {
    if built_in_keys(built_in, &name).is_some() {
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
