//! The resp layer: checks a [`Mapped`](moldette_core::Mapped) record against
//! an output schema and returns a [`Response`](moldette_core::Response).
//!
//! It takes only `moldette-core` types, so it can check records produced by
//! any tool (#3).

mod spec;

pub use spec::RespSpec;

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn public_types_are_send_and_sync() {
        assert_send_sync::<RespSpec>();
    }
}
