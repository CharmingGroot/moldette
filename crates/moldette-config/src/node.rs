use std::collections::HashSet;

use crate::error::{LoadError, LoadErrorKind, Position};

#[derive(Debug, Clone, Eq)]
pub(crate) struct Node {
    pub(crate) value: NodeValue,
    pub(crate) position: Option<Position>,
}

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NodeValue {
    Map(Vec<(Key, Node)>),
    Sequence(Vec<Node>),
    Scalar(Scalar),
}

#[derive(Debug, Clone, Eq)]
pub(crate) struct Key {
    pub(crate) text: String,
    pub(crate) position: Option<Position>,
}

impl PartialEq for Key {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Scalar {
    Plain(String),
    Text(String),
    Number(String),
    Bool(bool),
    Null,
}

pub(crate) const MAX_DEPTH: usize = 128;

pub(crate) fn key_path(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_owned()
    } else {
        format!("{path}.{key}")
    }
}

pub(crate) fn index_path(path: &str, index: usize) -> String {
    format!("{path}[{index}]")
}

pub(crate) fn reject_duplicate_keys(node: &Node, path: &str) -> Result<(), LoadError> {
    match &node.value {
        NodeValue::Map(entries) => {
            let mut seen = HashSet::new();
            for (key, value) in entries {
                if !seen.insert(key.text.as_str()) {
                    return Err(LoadError::new(
                        LoadErrorKind::DuplicateKey(key.text.clone()),
                        path.to_owned(),
                        key.position,
                    ));
                }
                reject_duplicate_keys(value, &key_path(path, &key.text))?;
            }
            Ok(())
        }
        NodeValue::Sequence(items) => items
            .iter()
            .enumerate()
            .try_for_each(|(i, item)| reject_duplicate_keys(item, &index_path(path, i))),
        NodeValue::Scalar(_) => Ok(()),
    }
}
