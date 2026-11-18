//! Parses one raw record into a [`FieldRecord`](moldette_core::FieldRecord)
//! of named text fields.
//!
//! A [`ParserSpec`] is validated when built and is immutable afterwards, so
//! one spec can be shared across threads (#8). Values stay text; type
//! conversion belongs to the mapper (#3).
//!
//! With the `tracing` feature, building a spec runs in an `INFO` span
//! `moldette.parser.build` that reports a failure as a `DEBUG` event
//! `moldette.parser.spec_error`. Each record is parsed in a `TRACE` span
//! `moldette.parser.parse`, each body in a child `TRACE` span
//! `moldette.parser.body`, and a record a format cannot read gives a `TRACE`
//! event `moldette.parser.malformed` in the span of that format.
//! Signals carry format names, field counts and body paths, never record
//! values or malformed reasons (#7).

mod custom;
mod delimited;
mod json;
mod kv;
mod regex;
mod spec;
mod telemetry;
mod text;

pub use custom::{CustomFormat, Parser};
pub use spec::{Format, ParserSpec, ParserSpecBuilder};
