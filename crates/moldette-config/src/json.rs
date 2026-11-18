use std::fmt;

use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};

use crate::error::{LoadError, LoadErrorKind, Position};
use crate::node::{Key, Node, NodeValue, Scalar};

pub(crate) fn read(text: &str) -> Result<Node, LoadError> {
    let mut deserializer = serde_json::Deserializer::from_str(text);
    Node::deserialize(&mut deserializer)
        .and_then(|node| deserializer.end().map(|()| node))
        .map_err(syntax_error)
}

fn syntax_error(err: serde_json::Error) -> LoadError {
    let message = err.to_string();
    let suffix = format!(" at line {} column {}", err.line(), err.column());
    let message = message.strip_suffix(&suffix).unwrap_or(&message).to_owned();
    let position = (err.line() > 0).then(|| Position::new(err.line(), err.column()));
    LoadError::new(LoadErrorKind::Syntax(message), String::new(), position)
}

fn node(value: NodeValue) -> Node {
    Node {
        value,
        position: None,
    }
}

fn scalar(scalar: Scalar) -> Node {
    node(NodeValue::Scalar(scalar))
}

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(NodeVisitor)
    }
}

struct NodeVisitor;

impl<'de> Visitor<'de> for NodeVisitor {
    type Value = Node;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Node, E> {
        Ok(scalar(Scalar::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Node, E> {
        Ok(scalar(Scalar::Number(value.to_string())))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Node, E> {
        Ok(scalar(Scalar::Number(value.to_string())))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Node, E> {
        Ok(scalar(Scalar::Number(format!("{value:?}"))))
    }

    fn visit_str<E>(self, value: &str) -> Result<Node, E> {
        Ok(scalar(Scalar::Text(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> Result<Node, E> {
        Ok(scalar(Scalar::Text(value)))
    }

    fn visit_unit<E>(self) -> Result<Node, E> {
        Ok(scalar(Scalar::Null))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Node, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(node(NodeValue::Sequence(items)))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Node, A::Error> {
        let mut entries = Vec::new();
        while let Some(text) = map.next_key()? {
            entries.push((
                Key {
                    text,
                    position: None,
                },
                map.next_value()?,
            ));
        }
        Ok(node(NodeValue::Map(entries)))
    }
}
