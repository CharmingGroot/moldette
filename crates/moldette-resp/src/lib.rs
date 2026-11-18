//! The resp layer: checks a [`Mapped`](moldette_core::Mapped) record against
//! an output schema and returns a [`Response`](moldette_core::Response).
//!
//! It takes only `moldette-core` types, so it can check records produced by
//! any tool (#3).
//!
//! With the `tracing` feature, each batch runs in an `INFO` span
//! `moldette.resp.respond_batch` that carries the response count per status.
//! Its duration includes reading the input records. Signals never carry record
//! values (#7).

mod spec;
mod telemetry;

pub use spec::{RespSpec, RespSpecBuilder};

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn public_types_are_send_and_sync() {
        assert_send_sync::<RespSpec>();
    }
}
