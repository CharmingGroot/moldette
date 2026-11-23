//! Turns one raw record into one [`Response`] by running the parser, mapper
//! and resp layers in turn (#2).
//!
//! Each layer crate is behind the feature of the same name and re-exported as
//! the module `parser`, `mapper`, `resp` or `config`. The `config` feature is
//! on by default and turns on the three other layers. `Pipeline` needs the
//! `parser`, `mapper` and `resp` features. The contract types of
//! `moldette-core` are re-exported at the root.
//!
//! The `framing` feature adds [`framing`], which splits an input stream
//! into raw records before the parser (#103).
//!
//! The `tracing` feature turns on the `tracing` feature of each layer
//! selected. The pipeline emits no signals of its own: in a batch, the parser
//! and mapper spans of each record are children of the
//! `moldette.resp.respond_batch` span (#7).
//!
//! ```
//! # #[cfg(feature = "config")]
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use moldette::config::{Config, Registry};
//! use moldette::{Pipeline, Response};
//!
//! let yaml = r#"
//! schemas:
//!   - name: order
//!     version: 1
//!     fields:
//!       - {name: id, type: string, required: true}
//!       - {name: total, type: float}
//! parser:
//!   format: json
//!   fields: {id: /id, total: /total}
//! mapper:
//!   schema: {name: order, version: 1}
//! resp:
//!   schema: {name: order, version: 1}
//! "#;
//! let specs = Config::from_yaml(yaml)?.build(&Registry::new())?;
//! let pipeline = Pipeline::from_specs(&specs)?;
//! let response = pipeline.process(r#"{"id": "A-1", "total": 25.5}"#);
//! assert!(matches!(response, Response::Ok(_)));
//! # Ok(())
//! # }
//! # #[cfg(not(feature = "config"))]
//! # fn main() {}
//! ```

#[cfg(feature = "framing")]
pub mod framing;
#[cfg(all(feature = "parser", feature = "mapper", feature = "resp"))]
mod pipeline;

#[cfg(feature = "config")]
pub use moldette_config as config;
pub use moldette_core::*;
#[cfg(feature = "mapper")]
pub use moldette_mapper as mapper;
#[cfg(feature = "parser")]
pub use moldette_parser as parser;
#[cfg(feature = "resp")]
pub use moldette_resp as resp;
#[cfg(feature = "config")]
pub use pipeline::MissingSectionError;
#[cfg(all(feature = "parser", feature = "mapper", feature = "resp"))]
pub use pipeline::Pipeline;
