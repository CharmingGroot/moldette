use crate::error::{LoadError, LoadErrorKind, ValueKind};
use crate::node::{Key, Node, NodeValue, Scalar, index_path, key_path};

#[derive(Debug)]
pub(crate) struct Cursor<'n> {
    pub(crate) node: &'n Node,
    pub(crate) path: String,
}

impl<'n> Cursor<'n> {
    pub(crate) fn new(node: &'n Node, path: String) -> Self {
        Self { node, path }
    }

    pub(crate) fn error(&self, kind: LoadErrorKind) -> LoadError {
        LoadError::new(kind, self.path.clone(), self.node.position)
    }

    fn mismatch(&self, expected: ValueKind) -> LoadError {
        self.error(LoadErrorKind::TypeMismatch(expected))
    }

    pub(crate) fn map(self) -> Result<MapReader<'n>, LoadError> {
        match &self.node.value {
            NodeValue::Map(entries) => Ok(MapReader {
                entries,
                cursor: self,
            }),
            _ => Err(self.mismatch(ValueKind::Map)),
        }
    }

    pub(crate) fn sequence(self) -> Result<Vec<Cursor<'n>>, LoadError> {
        match &self.node.value {
            NodeValue::Sequence(items) => Ok(items
                .iter()
                .enumerate()
                .map(|(i, item)| Cursor::new(item, index_path(&self.path, i)))
                .collect()),
            _ => Err(self.mismatch(ValueKind::Sequence)),
        }
    }

    pub(crate) fn list<T>(
        self,
        decode: impl Fn(Cursor<'n>) -> Result<T, LoadError>,
    ) -> Result<Vec<T>, LoadError> {
        self.sequence()?.into_iter().map(decode).collect()
    }

    pub(crate) fn string(&self) -> Result<String, LoadError> {
        match &self.node.value {
            NodeValue::Scalar(Scalar::Plain(text) | Scalar::Text(text)) => Ok(text.clone()),
            _ => Err(self.mismatch(ValueKind::String)),
        }
    }

    pub(crate) fn bool(&self) -> Result<bool, LoadError> {
        match &self.node.value {
            NodeValue::Scalar(Scalar::Bool(value)) => Ok(*value),
            NodeValue::Scalar(Scalar::Plain(text)) => match text.as_str() {
                "true" | "True" | "TRUE" => Ok(true),
                "false" | "False" | "FALSE" => Ok(false),
                _ => Err(self.mismatch(ValueKind::Bool)),
            },
            _ => Err(self.mismatch(ValueKind::Bool)),
        }
    }

    pub(crate) fn integer<T: TryFrom<u64>>(&self) -> Result<T, LoadError> {
        let text = match &self.node.value {
            NodeValue::Scalar(Scalar::Plain(text) | Scalar::Number(text))
                if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) =>
            {
                text
            }
            _ => return Err(self.mismatch(ValueKind::Integer)),
        };
        text.parse::<u64>()
            .ok()
            .and_then(|value| T::try_from(value).ok())
            .ok_or_else(|| self.error(LoadErrorKind::IntegerOutOfRange(text.clone())))
    }

    pub(crate) fn character(&self) -> Result<char, LoadError> {
        let text = self.string()?;
        let mut chars = text.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => Ok(c),
            _ => Err(self.mismatch(ValueKind::Character)),
        }
    }
}

pub(crate) struct MapReader<'n> {
    pub(crate) entries: &'n [(Key, Node)],
    pub(crate) cursor: Cursor<'n>,
}

impl<'n> MapReader<'n> {
    pub(crate) fn entries(&self) -> impl Iterator<Item = (&'n str, Cursor<'n>)> + '_ {
        self.entries.iter().map(|(key, node)| {
            (
                key.text.as_str(),
                Cursor::new(node, key_path(&self.cursor.path, &key.text)),
            )
        })
    }

    pub(crate) fn allow(&self, keys: &[&str]) -> Result<(), LoadError> {
        match self
            .entries
            .iter()
            .find(|(key, _)| !keys.contains(&key.text.as_str()))
        {
            Some((key, _)) => Err(self.key_error(LoadErrorKind::UnknownKey(key.text.clone()), key)),
            None => Ok(()),
        }
    }

    pub(crate) fn get(&self, key: &str) -> Option<Cursor<'n>> {
        self.entries
            .iter()
            .find(|(k, _)| k.text == key)
            .map(|(_, node)| Cursor::new(node, key_path(&self.cursor.path, key)))
    }

    pub(crate) fn require(&self, key: &str) -> Result<Cursor<'n>, LoadError> {
        self.get(key)
            .ok_or_else(|| self.cursor.error(LoadErrorKind::MissingKey(key.to_owned())))
    }

    pub(crate) fn flag(&self, key: &str) -> Result<bool, LoadError> {
        self.get(key).map_or(Ok(false), |cursor| cursor.bool())
    }

    pub(crate) fn one_of(
        &self,
        keys: &[&'static str],
    ) -> Result<(&'static str, Cursor<'n>), LoadError> {
        let mut present = self
            .entries
            .iter()
            .filter_map(|(key, _)| keys.iter().find(|k| **k == key.text).map(|k| (*k, key)));
        let Some((first, _)) = present.next() else {
            return Err(self.cursor.error(LoadErrorKind::MissingOneOf(
                keys.iter().map(|k| (*k).to_owned()).collect(),
            )));
        };
        if let Some((second, key)) = present.next() {
            return Err(self.key_error(
                LoadErrorKind::ConflictingKeys(first.to_owned(), second.to_owned()),
                key,
            ));
        }
        self.require(first).map(|cursor| (first, cursor))
    }

    pub(crate) fn conflict(&self) -> LoadError {
        let mut keys = self.entries.iter();
        match (keys.next(), keys.next()) {
            (Some((first, _)), Some((second, _))) => self.key_error(
                LoadErrorKind::ConflictingKeys(first.text.clone(), second.text.clone()),
                second,
            ),
            _ => self.cursor.mismatch(ValueKind::Op),
        }
    }

    fn key_error(&self, kind: LoadErrorKind, key: &Key) -> LoadError {
        LoadError::new(kind, self.cursor.path.clone(), key.position)
    }
}
