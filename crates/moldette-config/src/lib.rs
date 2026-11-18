//! Reads a moldette configuration written in YAML or JSON into a
//! [`Config`] and builds the layer specs it describes.
//!
//! Both formats are read into one tree that keeps scalar text as written, so
//! `"01"` stays text. Unknown keys, duplicate keys, mutually exclusive keys
//! and YAML anchors, aliases and tags are rejected with the key path and, for
//! YAML, the line and column (#14). Nothing is read from files (#2).
//!
//! [`Config::build`] calls the parser, mapper and resp builders for the
//! sections present. Caller-defined ops and formats come from a [`Registry`]
//! passed to the call.

mod arguments;
mod decode;
mod error;
mod json;
mod lower;
mod model;
mod node;
mod registry;
mod yaml;

pub use arguments::Arguments;
pub use error::{LoadError, LoadErrorKind, Position, ValueKind};
pub use lower::Specs;
pub use model::{
    BodyEntry, Config, FieldEntry, FormatEntry, MapperSection, OpEntry, ParserSection, RespSection,
    RuleEntry, SchemaEntry, SchemaRef, SourceEntry,
};
pub use registry::{Registry, RegistryError};

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn public_types_are_send_and_sync() {
        assert_send_sync::<Config>();
        assert_send_sync::<LoadError>();
        assert_send_sync::<Arguments>();
        assert_send_sync::<Registry>();
        assert_send_sync::<RegistryError>();
        assert_send_sync::<Specs>();
    }
}
