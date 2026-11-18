//! Parses one raw record into a [`FieldRecord`](moldette_core::FieldRecord)
//! of named text fields.
//!
//! A [`ParserSpec`] is validated when built and is immutable afterwards, so
//! one spec can be shared across threads (#8). Values stay text; type
//! conversion belongs to the mapper (#3).

mod json;
mod spec;

pub use spec::{Format, ParserSpec, ParserSpecBuilder};
