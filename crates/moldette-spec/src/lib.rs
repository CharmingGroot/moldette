//! Output formats of moldette responses (#103).
//!
//! The pipeline crates have no serialization dependency (#84). This crate
//! gives each output format a module behind a feature of the same name, so
//! that programs which send responses out of the process, such as the
//! `moldette` command and the sidecar, write them the same way.
//!
//! This crate depends on `moldette-core` only, and no pipeline crate depends
//! on it.
//!
//! - `json` (default): [`json::JsonResponse`] and the other types implement
//!   `serde::Serialize` with the JSON representation of responses.

#[cfg(feature = "json")]
pub mod json;
