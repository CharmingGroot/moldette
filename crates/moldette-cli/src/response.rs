use moldette::{Response, Timestamp, Value};
use serde_json::{Map, json};
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};

pub(crate) fn to_json(response: &Response<'_>) -> serde_json::Value {
    match response {
        Response::Ok(mapped) => json!({
            "status": "ok",
            "fields": mapped
                .iter()
                .map(|(name, value)| (name.to_owned(), to_json_value(value)))
                .collect::<Map<_, _>>(),
            "emptied": mapped.emptied().collect::<Vec<_>>(),
        }),
        Response::Malformed(malformed) => {
            json!({ "status": "malformed", "reason": malformed.reason() })
        }
        Response::MissingRequired(fields) => {
            json!({ "status": "missing_required", "fields": fields })
        }
        Response::Emptied(fields) => json!({ "status": "emptied", "fields": fields }),
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
    fn ip_is_written_as_text() {
        let address = "2001:db8::1".parse().expect("IPv6 address");
        assert_eq!(to_json_value(&Value::Ip(address)), json!("2001:db8::1"));
    }
}
