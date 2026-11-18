use moldette_core::SpecError;

use crate::decode::Cursor;
use crate::error::{LoadError, LoadErrorKind, Position};
use crate::node::{Key, Node, key_path};

/// The argument map of a caller-defined op or format, read by its
/// [`Registry`](crate::Registry) constructor. Keys the constructor leaves
/// unread are rejected.
#[derive(Debug, Clone, Eq)]
pub struct Arguments {
    entries: Vec<(Key, Node)>,
    path: String,
    position: Option<Position>,
    read: Vec<bool>,
}

impl PartialEq for Arguments {
    fn eq(&self, other: &Self) -> bool {
        self.entries == other.entries && self.path == other.path
    }
}

impl Arguments {
    pub(crate) fn new(entries: Vec<(Key, Node)>, path: String, position: Option<Position>) -> Self {
        let read = vec![false; entries.len()];
        Self {
            entries,
            path,
            position,
            read,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.entries.iter().any(|(k, _)| k.text == key)
    }

    /// Reads `key` as a string. A missing key or a value of another kind
    /// fails with its key path and position.
    pub fn string(&mut self, key: &str) -> Result<String, LoadError> {
        self.read(key)?.string()
    }

    /// Reads `key` as a single character.
    pub fn character(&mut self, key: &str) -> Result<char, LoadError> {
        self.read(key)?.character()
    }

    /// Reads `key` as a non-negative integer that fits in `T`.
    pub fn integer<T: TryFrom<u64>>(&mut self, key: &str) -> Result<T, LoadError> {
        self.read(key)?.integer()
    }

    pub fn bool(&mut self, key: &str) -> Result<bool, LoadError> {
        self.read(key)?.bool()
    }

    /// Reads `key` as a sequence and returns its items in declared order.
    pub fn list(&mut self, key: &str) -> Result<Vec<Argument<'_>>, LoadError> {
        Argument(self.read(key)?).list()
    }

    /// Reads `key` as a map and returns its entries in declared order. Every
    /// entry counts as read.
    pub fn map(&mut self, key: &str) -> Result<Vec<(&str, Argument<'_>)>, LoadError> {
        Argument(self.read(key)?).map()
    }

    /// Reports `error`, found by the constructor in the value of `key`, at
    /// the key path and position of that value.
    pub fn spec_error(&self, key: &str, error: SpecError) -> LoadError {
        let position = self
            .entries
            .iter()
            .find(|(k, _)| k.text == key)
            .map_or(self.position, |(_, node)| node.position);
        LoadError::new(
            LoadErrorKind::Spec(error),
            key_path(&self.path, key),
            position,
        )
    }

    pub(crate) fn error(&self, kind: LoadErrorKind) -> LoadError {
        LoadError::new(kind, self.path.clone(), self.position)
    }

    pub(crate) fn unread(&self) -> Self {
        Self::new(self.entries.clone(), self.path.clone(), self.position)
    }

    pub(crate) fn reject_unread(&self) -> Result<(), LoadError> {
        match self
            .entries
            .iter()
            .zip(&self.read)
            .find(|(_, read)| !**read)
        {
            Some(((key, _), _)) => Err(LoadError::new(
                LoadErrorKind::UnknownKey(key.text.clone()),
                self.path.clone(),
                key.position,
            )),
            None => Ok(()),
        }
    }

    fn read(&mut self, key: &str) -> Result<Cursor<'_>, LoadError> {
        let ((_, node), read) = self
            .entries
            .iter()
            .zip(self.read.iter_mut())
            .find(|((k, _), _)| k.text == key)
            .ok_or_else(|| {
                LoadError::new(
                    LoadErrorKind::MissingKey(key.to_owned()),
                    self.path.clone(),
                    self.position,
                )
            })?;
        *read = true;
        Ok(Cursor::new(node, key_path(&self.path, key)))
    }
}

/// One item of a list argument or one value of a map argument, read with the
/// same checks as a key of [`Arguments`].
#[derive(Debug)]
pub struct Argument<'a>(Cursor<'a>);

impl<'a> Argument<'a> {
    pub fn string(&self) -> Result<String, LoadError> {
        self.0.string()
    }

    pub fn character(&self) -> Result<char, LoadError> {
        self.0.character()
    }

    pub fn integer<T: TryFrom<u64>>(&self) -> Result<T, LoadError> {
        self.0.integer()
    }

    pub fn bool(&self) -> Result<bool, LoadError> {
        self.0.bool()
    }

    pub fn list(&self) -> Result<Vec<Argument<'a>>, LoadError> {
        Ok(self
            .cursor()
            .sequence()?
            .into_iter()
            .map(Argument)
            .collect())
    }

    pub fn map(&self) -> Result<Vec<(&'a str, Argument<'a>)>, LoadError> {
        Ok(self
            .cursor()
            .map()?
            .entries()
            .map(|(key, cursor)| (key, Argument(cursor)))
            .collect())
    }

    /// Reports `error`, found by the constructor in this value, at its key
    /// path and position.
    pub fn spec_error(&self, error: SpecError) -> LoadError {
        self.0.error(LoadErrorKind::Spec(error))
    }

    fn cursor(&self) -> Cursor<'a> {
        Cursor::new(self.0.node, self.0.path.clone())
    }
}
