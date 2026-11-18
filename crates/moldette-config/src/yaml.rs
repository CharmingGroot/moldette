use yaml_rust2::Event;
use yaml_rust2::parser::Parser;
use yaml_rust2::scanner::{Marker, TScalarStyle};

use crate::error::{LoadError, LoadErrorKind, ValueKind};
use crate::node::{Key, MAX_DEPTH, Node, NodeValue, Scalar, index_path, key_path};
use crate::position::Position;

pub(crate) fn read(text: &str) -> Result<Node, LoadError> {
    let mut builder = Builder::default();
    let mut parser = Parser::new_from_str(text);
    loop {
        let (event, marker) = parser.next_token().map_err(|err| {
            LoadError::new(
                LoadErrorKind::Syntax(err.info().to_owned()),
                builder.path(),
                Some(position(err.marker())),
            )
        })?;
        if event == Event::StreamEnd {
            return builder.root.ok_or_else(|| {
                LoadError::new(
                    LoadErrorKind::TypeMismatch(ValueKind::Map),
                    String::new(),
                    Some(position(&marker)),
                )
            });
        }
        builder.push(event, position(&marker))?;
    }
}

fn position(marker: &Marker) -> Position {
    Position::new(marker.line(), marker.col() + 1)
}

#[derive(Default)]
struct Builder {
    stack: Vec<Frame>,
    root: Option<Node>,
    documents: usize,
}

enum Frame {
    Sequence {
        items: Vec<Node>,
        position: Position,
    },
    Map {
        entries: Vec<(Key, Node)>,
        key: Option<Key>,
        position: Position,
    },
}

impl Builder {
    fn push(&mut self, event: Event, position: Position) -> Result<(), LoadError> {
        match event {
            Event::DocumentStart => {
                self.documents += 1;
                if self.documents > 1 {
                    return Err(self.error(LoadErrorKind::MultipleDocuments, position));
                }
                Ok(())
            }
            Event::Alias(_) => Err(self.error(LoadErrorKind::AnchorOrAlias, position)),
            Event::Scalar(text, style, anchor, tag) => {
                self.reject_anchor_and_tag(anchor, tag.is_some(), position)?;
                if let Some(Frame::Map {
                    key: key @ None, ..
                }) = self.stack.last_mut()
                {
                    *key = Some(Key {
                        text,
                        position: Some(position),
                    });
                    return Ok(());
                }
                self.complete(Node {
                    value: NodeValue::Scalar(scalar(text, style)),
                    position: Some(position),
                });
                Ok(())
            }
            Event::SequenceStart(anchor, tag) => {
                self.open(anchor, tag.is_some(), position)?;
                self.stack.push(Frame::Sequence {
                    items: Vec::new(),
                    position,
                });
                Ok(())
            }
            Event::MappingStart(anchor, tag) => {
                self.open(anchor, tag.is_some(), position)?;
                self.stack.push(Frame::Map {
                    entries: Vec::new(),
                    key: None,
                    position,
                });
                Ok(())
            }
            Event::SequenceEnd | Event::MappingEnd => {
                let node = match self.stack.pop() {
                    Some(Frame::Sequence { items, position }) => Node {
                        value: NodeValue::Sequence(items),
                        position: Some(position),
                    },
                    Some(Frame::Map {
                        entries, position, ..
                    }) => Node {
                        value: NodeValue::Map(entries),
                        position: Some(position),
                    },
                    None => return Ok(()),
                };
                self.complete(node);
                Ok(())
            }
            Event::Nothing | Event::StreamStart | Event::StreamEnd | Event::DocumentEnd => Ok(()),
        }
    }

    fn open(&mut self, anchor: usize, tagged: bool, position: Position) -> Result<(), LoadError> {
        self.reject_anchor_and_tag(anchor, tagged, position)?;
        if let Some(Frame::Map { key: None, .. }) = self.stack.last() {
            return Err(self.error(LoadErrorKind::NonScalarKey, position));
        }
        if self.stack.len() >= MAX_DEPTH {
            return Err(self.error(LoadErrorKind::NestingTooDeep, position));
        }
        Ok(())
    }

    fn reject_anchor_and_tag(
        &self,
        anchor: usize,
        tagged: bool,
        position: Position,
    ) -> Result<(), LoadError> {
        if anchor != 0 {
            return Err(self.error(LoadErrorKind::AnchorOrAlias, position));
        }
        if tagged {
            return Err(self.error(LoadErrorKind::Tag, position));
        }
        Ok(())
    }

    fn complete(&mut self, node: Node) {
        match self.stack.last_mut() {
            Some(Frame::Sequence { items, .. }) => items.push(node),
            Some(Frame::Map { entries, key, .. }) => {
                if let Some(key) = key.take() {
                    entries.push((key, node));
                }
            }
            None => self.root = Some(node),
        }
    }

    fn path(&self) -> String {
        let mut path = String::new();
        for frame in &self.stack {
            path = match frame {
                Frame::Sequence { items, .. } => index_path(&path, items.len()),
                Frame::Map { key: Some(key), .. } => key_path(&path, &key.text),
                Frame::Map { key: None, .. } => path,
            };
        }
        path
    }

    fn error(&self, kind: LoadErrorKind, position: Position) -> LoadError {
        LoadError::new(kind, self.path(), Some(position))
    }
}

fn scalar(text: String, style: TScalarStyle) -> Scalar {
    match style {
        TScalarStyle::Plain if matches!(text.as_str(), "" | "~" | "null" | "Null" | "NULL") => {
            Scalar::Null
        }
        TScalarStyle::Plain => Scalar::Plain(text),
        _ => Scalar::Text(text),
    }
}
