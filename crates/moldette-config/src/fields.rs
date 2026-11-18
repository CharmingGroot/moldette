use crate::model::ParserSection;
use crate::node::{index_path, key_path};
use crate::position::Position;

pub(crate) fn parser_field_names<'s>(
    section: &'s ParserSection,
    path: &str,
) -> Vec<(&'s str, String, Option<Position>)> {
    let fields_path = key_path(path, "fields");
    let mut names: Vec<_> = section
        .fields
        .iter()
        .enumerate()
        .map(|(i, (name, _))| {
            (
                name.as_str(),
                key_path(&fields_path, name),
                section.field_positions.get(i).copied().flatten(),
            )
        })
        .collect();
    for (i, body) in section.bodies.iter().enumerate() {
        names.extend(parser_field_names(&body.parser, &body_path(path, i)));
    }
    names
}

pub(crate) fn body_path(path: &str, index: usize) -> String {
    index_path(&key_path(path, "bodies"), index)
}
