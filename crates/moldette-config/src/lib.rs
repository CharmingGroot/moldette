//! Reads a moldette configuration written in YAML or JSON into a
//! [`Config`].
//!
//! Both formats are read into one tree that keeps scalar text as written, so
//! `"01"` stays text. Unknown keys, duplicate keys, mutually exclusive keys
//! and YAML anchors, aliases and tags are rejected with the key path and, for
//! YAML, the line and column (#14). Nothing is read from files (#2).

mod decode;
mod error;
mod json;
mod model;
mod node;
mod yaml;

pub use error::{LoadError, LoadErrorKind, Position, ValueKind};
pub use model::{
    Arguments, BodyEntry, Config, FieldEntry, FormatEntry, MapperSection, OpEntry, ParserSection,
    RespSection, RuleEntry, SchemaEntry, SchemaRef, SourceEntry,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn public_types_are_send_and_sync() {
        assert_send_sync::<Config>();
        assert_send_sync::<LoadError>();
    }
}
