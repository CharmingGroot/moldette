use crate::explain;
use crate::value::to_json_value;
use moldette::{Explain, Mapped, Response, Value};
use serde_core::ser::SerializeMap;
use serde_core::{Serialize, Serializer};

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
