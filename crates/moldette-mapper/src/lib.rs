//! Maps a [`FieldRecord`](moldette_core::FieldRecord) onto an
//! [`OutputSchema`](moldette_core::OutputSchema).
//!
//! Each output field takes its value from a source field, passes it through
//! the ops of its rule and converts it to the field's schema type. A value
//! that an op or the conversion cannot handle is absent and reported as
//! emptied (#5).
//!
//! With the `tracing` feature, building a spec runs in an `INFO` span
//! `moldette.mapper.build` that reports a failure as a `DEBUG` event
//! `moldette.mapper.spec_error`. Each record is mapped in a `TRACE` span
//! `moldette.mapper.map` with a `TRACE` event `moldette.mapper.emptied` per
//! emptied field.
//! Signals carry schema and field names, never record values (#7).

mod coerce;
mod op;
mod recorder;
mod rule;
mod spec;
mod telemetry;

pub use op::{Op, TimestampDefaults, Unmatched};
pub use rule::Rule;
pub use spec::{MapperSpec, MapperSpecBuilder};

#[cfg(test)]
mod tests {
    use std::panic::{RefUnwindSafe, UnwindSafe};

    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    fn assert_unwind_safe<T: UnwindSafe + RefUnwindSafe>() {}

    #[test]
    fn public_types_are_send_and_sync() {
        assert_send_sync::<MapperSpec>();
        assert_send_sync::<MapperSpecBuilder>();
        assert_send_sync::<Rule>();
        assert_send_sync::<TimestampDefaults>();
        assert_send_sync::<Unmatched>();
        assert_send_sync::<Box<dyn Op>>();
    }

    #[test]
    fn spec_stays_unwind_safe_with_custom_ops() {
        assert_unwind_safe::<MapperSpec>();
        assert_unwind_safe::<Rule>();
    }
}
