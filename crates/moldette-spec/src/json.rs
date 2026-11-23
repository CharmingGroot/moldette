//! The JSON representation of responses.
//!
//! A response is one object whose `status` is `ok`, `malformed`,
//! `missing_required` or `emptied`:
//!
//! - `ok`: `fields`, the mapped fields in schema order, and `emptied`, the
//!   names of fields emptied by a fallback.
//! - `malformed`: `reason`.
//! - `missing_required`, `emptied`: `fields`, the names of the fields.
//!
//! Values are JSON strings, numbers and booleans. A float that is not finite
//! is a string (`"NaN"`, `"inf"`, `"-inf"`), an IP address is its text, and a
//! timestamp is RFC 3339 text with its offset, or an object of `nanos`,
//! `offset_seconds` and `seconds` when RFC 3339 cannot represent it.
//!
//! The types implement `serde::Serialize`, so any serde serializer accepts
//! them; the representation is chosen for JSON.

use moldette_core::{
    Explain, ExplainRead, ExplainSource, ExplainStep, Mapped, Response, Timestamp, Value,
};
use serde_core::ser::{SerializeMap, SerializeStruct};
use serde_core::{Serialize, Serializer};
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};

/// A response in its JSON representation, with an optional `explain` entry.
#[derive(Debug, Clone, Copy)]
pub struct JsonResponse<'r, 'a> {
    response: &'r Response<'a>,
    explain: Option<Option<&'r Explain<'a>>>,
}

impl<'r, 'a> JsonResponse<'r, 'a> {
    pub fn new(response: &'r Response<'a>) -> Self {
        Self {
            response,
            explain: None,
        }
    }

    /// Adds an `explain` entry after the response, `null` when `explain` is
    /// `None`.
    pub fn with_explain(response: &'r Response<'a>, explain: Option<&'r Explain<'a>>) -> Self {
        Self {
            response,
            explain: Some(explain),
        }
    }
}

impl Serialize for JsonResponse<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        match self.response {
            Response::Ok(mapped) => {
                map.serialize_entry("status", "ok")?;
                map.serialize_entry("fields", &Fields(mapped))?;
                map.serialize_entry("emptied", &Emptied(mapped))?;
            }
            Response::Malformed(malformed) => {
                map.serialize_entry("status", "malformed")?;
                map.serialize_entry("reason", malformed.reason())?;
            }
            Response::MissingRequired(fields) => {
                map.serialize_entry("status", "missing_required")?;
                map.serialize_entry("fields", fields)?;
            }
            Response::Emptied(fields) => {
                map.serialize_entry("status", "emptied")?;
                map.serialize_entry("fields", fields)?;
            }
        }
        if let Some(explain) = self.explain {
            map.serialize_entry("explain", &explain.map(JsonExplain::new))?;
        }
        map.end()
    }
}

/// A value in its JSON representation.
#[derive(Debug, Clone, Copy)]
pub struct JsonValue<'v, 'a>(&'v Value<'a>);

impl<'v, 'a> JsonValue<'v, 'a> {
    pub fn new(value: &'v Value<'a>) -> Self {
        Self(value)
    }
}

impl Serialize for JsonValue<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Value::Str(text) => serializer.serialize_str(text),
            Value::Int(number) => serializer.serialize_i64(*number),
            Value::Float(number) if number.is_finite() => serializer.serialize_f64(*number),
            Value::Float(number) => serializer.collect_str(number),
            Value::Bool(flag) => serializer.serialize_bool(*flag),
            Value::Timestamp(timestamp) => match rfc3339(timestamp) {
                Some(text) => serializer.serialize_str(&text),
                None => {
                    let mut parts = serializer.serialize_struct("Timestamp", 3)?;
                    parts.serialize_field("nanos", &timestamp.nanos())?;
                    parts.serialize_field("offset_seconds", &timestamp.offset_seconds())?;
                    parts.serialize_field("seconds", &timestamp.seconds())?;
                    parts.end()
                }
            },
            Value::Ip(address) => serializer.collect_str(address),
        }
    }
}

fn rfc3339(timestamp: &Timestamp) -> Option<String> {
    let offset = UtcOffset::from_whole_seconds(timestamp.offset_seconds()).ok()?;
    OffsetDateTime::from_unix_timestamp(timestamp.seconds())
        .ok()?
        .replace_nanosecond(timestamp.nanos())
        .ok()?
        .checked_to_offset(offset)?
        .format(&Rfc3339)
        .ok()
}

/// How each field of a record was filled, in its JSON representation: an
/// object of `fields`, `schema` and `version`.
#[derive(Debug, Clone, Copy)]
pub struct JsonExplain<'r, 'a>(&'r Explain<'a>);

impl<'r, 'a> JsonExplain<'r, 'a> {
    pub fn new(explain: &'r Explain<'a>) -> Self {
        Self(explain)
    }
}

impl Serialize for JsonExplain<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_struct("Explain", 3)?;
        map.serialize_field("fields", &Seq(self.0.fields(), ExplainFieldJson))?;
        map.serialize_field("schema", self.0.schema())?;
        map.serialize_field("version", &self.0.version())?;
        map.end()
    }
}

struct ExplainFieldJson<'r, 'a>(&'r moldette_core::ExplainField<'a>);

impl Serialize for ExplainFieldJson<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_struct("ExplainField", 2)?;
        map.serialize_field("name", self.0.name())?;
        map.serialize_field("reads", &Seq(self.0.reads(), ExplainReadJson))?;
        map.end()
    }
}

struct ExplainReadJson<'r, 'a>(&'r ExplainRead<'a>);

impl Serialize for ExplainReadJson<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_struct("ExplainRead", 4)?;
        map.serialize_field("source", &ExplainSourceJson(self.0.source()))?;
        map.serialize_field("steps", &Seq(self.0.steps(), ExplainStepJson))?;
        map.serialize_field("text", &self.0.text())?;
        map.serialize_field("value", &self.0.value().map(JsonValue::new))?;
        map.end()
    }
}

struct ExplainSourceJson<'r, 'a>(&'r ExplainSource<'a>);

impl Serialize for ExplainSourceJson<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            ExplainSource::Field(name) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("field", name)?;
                map.end()
            }
            ExplainSource::Constant => serializer.serialize_str("constant"),
            _ => serializer.serialize_none(),
        }
    }
}

struct ExplainStepJson<'r, 'a>(&'r ExplainStep<'a>);

impl Serialize for ExplainStepJson<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_struct("ExplainStep", 2)?;
        map.serialize_field("op", self.0.op())?;
        map.serialize_field("output", &self.0.output())?;
        map.end()
    }
}

struct Fields<'r, 'a>(&'r Mapped<'a>);

impl Serialize for Fields<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_map(
            self.0
                .iter()
                .map(|(name, value)| (name, JsonValue::new(value))),
        )
    }
}

struct Emptied<'r, 'a>(&'r Mapped<'a>);

impl Serialize for Emptied<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.emptied())
    }
}

struct Seq<'r, T, F>(&'r [T], F);

impl<'r, T, F, J> Serialize for Seq<'r, T, F>
where
    F: Fn(&'r T) -> J,
    J: Serialize,
{
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(&self.1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use moldette_core::{ExplainField, Malformed};
    use serde_json::json;

    fn to_json(value: &impl Serialize) -> serde_json::Value {
        serde_json::to_value(value).expect("JSON")
    }

    fn text(value: &impl Serialize) -> String {
        serde_json::to_string(value).expect("JSON")
    }

    #[test]
    fn responses_put_the_status_first_and_fields_in_schema_order() {
        let mut mapped = Mapped::new();
        mapped.insert("id", Value::Str("A-1".into()));
        mapped.insert("amount", Value::Float(2.5));
        mapped.mark_emptied("paid");
        let response = Response::Ok(mapped);
        assert_eq!(
            text(&JsonResponse::new(&response)),
            r#"{"status":"ok","fields":{"id":"A-1","amount":2.5},"emptied":["paid"]}"#
        );
        assert_eq!(
            text(&JsonResponse::with_explain(&response, None)),
            r#"{"status":"ok","fields":{"id":"A-1","amount":2.5},"emptied":["paid"],"explain":null}"#
        );
    }

    #[test]
    fn responses_other_than_ok_name_their_reason_or_fields() {
        let cases = [
            (
                Response::MissingRequired(vec!["id".into()]),
                r#"{"status":"missing_required","fields":["id"]}"#,
            ),
            (
                Response::Emptied(vec!["total".into(), "paid".into()]),
                r#"{"status":"emptied","fields":["total","paid"]}"#,
            ),
            (
                Response::Malformed(Malformed::new("unexpected end")),
                r#"{"status":"malformed","reason":"unexpected end"}"#,
            ),
        ];
        for (response, expected) in cases {
            assert_eq!(text(&JsonResponse::new(&response)), expected);
        }
    }

    #[test]
    fn explain_lists_each_read_with_its_steps_and_value() {
        let mut read = ExplainRead::new(ExplainSource::Field("total".into()), Some("12.50".into()));
        read.push(ExplainStep::new("float", Some("12.5".into())));
        read.set_value(Value::Float(12.5));
        let mut total = ExplainField::new("total");
        total.push(read);
        let mut region = ExplainField::new("region");
        region.push(ExplainRead::new(ExplainSource::Constant, None));
        let mut explain = Explain::new("order", 2);
        explain.push(total);
        explain.push(region);
        assert_eq!(
            text(&JsonExplain::new(&explain)),
            concat!(
                r#"{"fields":[{"name":"total","reads":[{"source":{"field":"total"},"#,
                r#""steps":[{"op":"float","output":"12.5"}],"text":"12.50","value":12.5}]},"#,
                r#"{"name":"region","reads":[{"source":"constant","steps":[],"text":null,"value":null}]}],"#,
                r#""schema":"order","version":2}"#
            )
        );
    }

    fn timestamp(seconds: i64, nanos: u32, offset_seconds: i32) -> serde_json::Value {
        to_json(&JsonValue::new(&Value::Timestamp(
            Timestamp::new(seconds, nanos, offset_seconds).expect("valid timestamp"),
        )))
    }

    #[test]
    fn timestamp_keeps_offset_and_fraction_in_rfc3339() {
        assert_eq!(
            timestamp(1_794_994_200, 250_000_000, 9 * 3600),
            json!("2026-11-18T18:30:00.25+09:00")
        );
    }

    #[test]
    fn timestamp_outside_rfc3339_keeps_its_parts() {
        let parts = |seconds: i64, offset_seconds: i32| json!({ "seconds": seconds, "nanos": 0, "offset_seconds": offset_seconds });
        assert_eq!(timestamp(0, 0, 30), parts(0, 30));
        assert_eq!(timestamp(253_402_300_800, 0, 0), parts(253_402_300_800, 0));
    }

    #[test]
    fn non_finite_float_is_written_as_text() {
        let value = |number| to_json(&JsonValue::new(&Value::Float(number)));
        assert_eq!(value(f64::NAN), json!("NaN"));
        assert_eq!(value(f64::NEG_INFINITY), json!("-inf"));
        assert_eq!(value(2.5), json!(2.5));
    }

    #[test]
    fn integers_booleans_and_ip_addresses_keep_their_kind() {
        let value = |value: Value<'static>| to_json(&JsonValue::new(&value));
        assert_eq!(value(Value::Int(-7)), json!(-7));
        assert_eq!(value(Value::Bool(false)), json!(false));
        let address = "2001:db8::1".parse().expect("IPv6 address");
        assert_eq!(value(Value::Ip(address)), json!("2001:db8::1"));
    }

    #[test]
    fn public_types_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<JsonResponse<'_, '_>>();
        assert_send_sync::<JsonValue<'_, '_>>();
        assert_send_sync::<JsonExplain<'_, '_>>();
    }
}
