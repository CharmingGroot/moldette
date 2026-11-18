use std::borrow::Cow;
use std::fmt;

use moldette_core::{Malformed, SpecError};
use serde::de::{
    self, DeserializeSeed, Deserializer as _, IgnoredAny, MapAccess, SeqAccess, Visitor,
};
use serde_json::value::RawValue;

#[derive(Debug, Clone)]
pub(crate) struct JsonPaths {
    root: Node,
    field_count: usize,
}

#[derive(Debug, Clone, Default)]
struct Node {
    fields: Vec<usize>,
    children: Vec<Child>,
}

#[derive(Debug, Clone)]
struct Child {
    token: String,
    index: Option<usize>,
    node: Node,
}

impl JsonPaths {
    pub(crate) fn new(pointers: &[String]) -> Result<Self, SpecError> {
        let mut root = Node::default();
        for (field, pointer) in pointers.iter().enumerate() {
            let tokens =
                reference_tokens(pointer).ok_or_else(|| SpecError::InvalidPath(pointer.clone()))?;
            let mut node = &mut root;
            for token in tokens {
                node = node.child_mut(token);
            }
            node.fields.push(field);
        }
        Ok(Self {
            root,
            field_count: pointers.len(),
        })
    }

    pub(crate) fn find<'a>(&self, raw: &'a str) -> Result<Vec<Option<&'a str>>, Malformed> {
        let mut texts = vec![None; self.field_count];
        let mut deserializer = serde_json::Deserializer::from_str(raw);
        deserializer
            .deserialize_map(Walk {
                node: &self.root,
                texts: &mut texts,
            })
            .and_then(|()| deserializer.end())
            .map_err(malformed)?;
        let document = raw.trim_matches([' ', '\t', '\n', '\r']);
        for &field in &self.root.fields {
            if let Some(text) = texts.get_mut(field) {
                *text = Some(document);
            }
        }
        Ok(texts)
    }
}

pub(crate) fn field_value(text: &str) -> Result<Option<Cow<'_, str>>, Malformed> {
    match text.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        Some(content) if !content.contains('\\') => Ok(Some(Cow::Borrowed(content))),
        Some(_) => serde_json::from_str::<String>(text)
            .map(|s| Some(Cow::Owned(s)))
            .map_err(malformed),
        None if text == "null" => Ok(None),
        None => Ok(Some(Cow::Borrowed(text))),
    }
}

fn reference_tokens(pointer: &str) -> Option<Vec<String>> {
    if pointer.is_empty() {
        return Some(Vec::new());
    }
    pointer
        .strip_prefix('/')?
        .split('/')
        .map(unescape)
        .collect()
}

fn unescape(token: &str) -> Option<String> {
    let mut unescaped = String::with_capacity(token.len());
    let mut chars = token.chars();
    while let Some(c) = chars.next() {
        match c {
            '~' => match chars.next()? {
                '0' => unescaped.push('~'),
                '1' => unescaped.push('/'),
                _ => return None,
            },
            _ => unescaped.push(c),
        }
    }
    Some(unescaped)
}

fn malformed(err: serde_json::Error) -> Malformed {
    Malformed::new(err.to_string())
}

impl Node {
    fn child_mut(&mut self, token: String) -> &mut Node {
        let position = match self.children.iter().position(|c| c.token == token) {
            Some(position) => position,
            None => {
                self.children.push(Child {
                    index: array_index(&token),
                    token,
                    node: Node::default(),
                });
                self.children.len() - 1
            }
        };
        &mut self.children[position].node
    }

    fn element(&self, index: usize) -> Option<&Child> {
        self.children.iter().find(|c| c.index == Some(index))
    }
}

pub(crate) fn array_index(token: &str) -> Option<usize> {
    let leading_zero = token.len() > 1 && token.starts_with('0');
    if leading_zero || !token.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    token.parse().ok()
}

struct Walk<'s, 'a> {
    node: &'s Node,
    texts: &'s mut [Option<&'a str>],
}

impl<'a> Walk<'_, 'a> {
    fn take<E: de::Error>(&mut self, node: &Node, value: &'a RawValue) -> Result<(), E> {
        let text = value.get();
        for &field in &node.fields {
            if let Some(slot) = self.texts.get_mut(field) {
                slot.get_or_insert(text);
            }
        }
        if node.children.is_empty() {
            return Ok(());
        }
        let mut deserializer = serde_json::Deserializer::from_str(text);
        let walk = Walk {
            node,
            texts: self.texts,
        };
        match text.as_bytes().first() {
            Some(b'{') => deserializer.deserialize_map(walk),
            Some(b'[') => deserializer.deserialize_seq(walk),
            _ => Ok(()),
        }
        .map_err(E::custom)
    }
}

impl<'de> Visitor<'de> for Walk<'_, 'de> {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON object")
    }

    fn visit_map<A: MapAccess<'de>>(mut self, mut map: A) -> Result<(), A::Error> {
        let node = self.node;
        while let Some(child) = map.next_key_seed(KeySeed(node))? {
            match child {
                Some(child) => {
                    let value = map.next_value()?;
                    self.take(&child.node, value)?;
                }
                None => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }

    fn visit_seq<A: SeqAccess<'de>>(mut self, mut seq: A) -> Result<(), A::Error> {
        let node = self.node;
        for index in 0.. {
            match node.element(index) {
                Some(child) => match seq.next_element()? {
                    Some(value) => self.take(&child.node, value)?,
                    None => break,
                },
                None => {
                    if seq.next_element::<IgnoredAny>()?.is_none() {
                        break;
                    }
                }
            }
        }
        Ok(())
    }
}

struct KeySeed<'s>(&'s Node);

impl<'de, 's> DeserializeSeed<'de> for KeySeed<'s> {
    type Value = Option<&'s Child>;

    fn deserialize<D: de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl<'de, 's> Visitor<'de> for KeySeed<'s> {
    type Value = Option<&'s Child>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an object key")
    }

    fn visit_str<E: de::Error>(self, key: &str) -> Result<Self::Value, E> {
        Ok(self.0.children.iter().find(|c| c.token == key))
    }
}
