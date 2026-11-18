//! Contract types shared by the moldette layers.
//!
//! Every layer takes and returns only the types defined here, so that each
//! layer can be used without the others (#3).
//!
//! A value is absent when it is `None`. A missing field and a null are both
//! absent; an empty string is a value. Ops that cannot handle their input and
//! failed type conversions also yield `None` (#5, #13).

mod error;
mod mapped;
mod names;
mod record;
mod response;
mod schema;
mod timestamp;
mod value;

pub use error::SpecError;
pub use mapped::Mapped;
pub use names::validate_field_names;
pub use record::FieldRecord;
pub use response::{Malformed, Response, StatusCounts};
pub use schema::{Field, OutputSchema, OutputSchemaBuilder, Type};
pub use timestamp::Timestamp;
pub use value::Value;

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn public_types_are_send_and_sync() {
        assert_send_sync::<Value<'_>>();
        assert_send_sync::<FieldRecord<'_>>();
        assert_send_sync::<Mapped<'_>>();
        assert_send_sync::<Response<'_>>();
        assert_send_sync::<StatusCounts>();
        assert_send_sync::<OutputSchema>();
        assert_send_sync::<SpecError>();
    }
}
