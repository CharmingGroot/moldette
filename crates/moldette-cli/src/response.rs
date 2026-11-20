use moldette::{Explain, Mapped, Response, Timestamp, Value};
use serde_core::ser::SerializeMap;
use serde_core::{Serialize, Serializer};
use serde_json::json;
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};

use crate::explain;

pub(crate) struct Printed<'r, 'a> {
    pub(crate) response: &'r Response<'a>,
    pub(crate) explain: Option<Option<&'r Explain<'a>>>,
}

impl Serialize for Printed<'_, '_> {
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
        if let Some(record) = self.explain {
            map.serialize_entry("explain", &record.map(explain::to_json))?;
        }
        map.end()
    }
}

struct Fields<'r, 'a>(&'r Mapped<'a>);

impl Serialize for Fields<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_map(self.0.iter().map(|(name, value)| (name, Json(value))))
    }
}

struct Emptied<'r, 'a>(&'r Mapped<'a>);

impl Serialize for Emptied<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.emptied())
    }
}

struct Json<'v, 'a>(&'v Value<'a>);

impl Serialize for Json<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Value::Str(text) => serializer.serialize_str(text),
            Value::Int(number) => serializer.serialize_i64(*number),
            Value::Float(number) if number.is_finite() => serializer.serialize_f64(*number),
            Value::Bool(flag) => serializer.serialize_bool(*flag),
            Value::Ip(address) => serializer.collect_str(address),
            value => to_json_value(value).serialize(serializer),
        }
    }
}

pub(crate) fn to_json_value(value: &Value<'_>) -> serde_json::Value {
    match value {
        Value::Str(text) => json!(text),
        Value::Int(number) => json!(number),
        Value::Float(number) if number.is_finite() => json!(number),
        Value::Float(number) => json!(number.to_string()),
        Value::Bool(flag) => json!(flag),
        Value::Timestamp(timestamp) => rfc3339(timestamp).map_or_else(
            || {
                json!({
                    "seconds": timestamp.seconds(),
                    "nanos": timestamp.nanos(),
                    "offset_seconds": timestamp.offset_seconds(),
                })
            },
            serde_json::Value::String,
        ),
        Value::Ip(address) => json!(address.to_string()),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn timestamp(seconds: i64, nanos: u32, offset_seconds: i32) -> serde_json::Value {
        to_json_value(&Value::Timestamp(
            Timestamp::new(seconds, nanos, offset_seconds).expect("valid timestamp"),
        ))
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
        assert_eq!(to_json_value(&Value::Float(f64::NAN)), json!("NaN"));
        assert_eq!(
            to_json_value(&Value::Float(f64::NEG_INFINITY)),
            json!("-inf")
        );
        assert_eq!(to_json_value(&Value::Float(2.5)), json!(2.5));
    }

    #[test]
    fn printed_lines_put_the_status_first_and_fields_in_schema_order() {
        let mut mapped = Mapped::new();
        mapped.insert("id", Value::Str("A-1".into()));
        mapped.insert("amount", Value::Float(2.5));
        mapped.mark_emptied("paid");
        let response = Response::Ok(mapped);
        let line = |explain| {
            serde_json::to_string(&Printed {
                response: &response,
                explain,
            })
            .expect("JSON")
        };
        assert_eq!(
            line(None),
            r#"{"status":"ok","fields":{"id":"A-1","amount":2.5},"emptied":["paid"]}"#
        );
        assert_eq!(
            line(Some(None)),
            r#"{"status":"ok","fields":{"id":"A-1","amount":2.5},"emptied":["paid"],"explain":null}"#
        );
        let missing = Response::MissingRequired(vec!["id".into()]);
        assert_eq!(
            serde_json::to_string(&Printed {
                response: &missing,
                explain: None,
            })
            .expect("JSON"),
            r#"{"status":"missing_required","fields":["id"]}"#
        );
    }

    #[test]
    fn ip_is_written_as_text() {
        let address = "2001:db8::1".parse().expect("IPv6 address");
        assert_eq!(to_json_value(&Value::Ip(address)), json!("2001:db8::1"));
    }
}
