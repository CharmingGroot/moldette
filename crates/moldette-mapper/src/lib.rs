//! Maps a [`FieldRecord`](moldette_core::FieldRecord) onto an
//! [`OutputSchema`](moldette_core::OutputSchema).
//!
//! Each output field takes its value from a source field, passes it through
//! the ops of its rule and converts it to the field's schema type. A value
//! that an op or the conversion cannot handle is absent and reported as
//! emptied (#5).

mod coerce;
mod op;
mod rule;
mod spec;

pub use rule::Rule;
pub use spec::{MapperSpec, MapperSpecBuilder};

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn public_types_are_send_and_sync() {
        assert_send_sync::<MapperSpec>();
        assert_send_sync::<MapperSpecBuilder>();
        assert_send_sync::<Rule>();
    }
}
