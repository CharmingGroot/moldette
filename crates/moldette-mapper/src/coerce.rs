use std::borrow::Cow;

use moldette_core::{Timestamp, Type, Value};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub(crate) fn coerce<'a>(text: &Cow<'a, str>, ty: Type) -> Option<Value<'a>> {
    match ty {
        Type::String => Some(Value::Str(text.clone())),
        Type::Int => text.parse().ok().map(Value::Int),
        Type::Float => text
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .map(Value::Float),
        Type::Bool => match text.as_ref() {
            "true" => Some(Value::Bool(true)),
            "false" => Some(Value::Bool(false)),
            _ => None,
        },
        Type::Timestamp => parse_rfc3339(text).map(Value::Timestamp),
        Type::Ip => text.parse().ok().map(Value::Ip),
    }
}

fn parse_rfc3339(text: &str) -> Option<Timestamp> {
    let parsed = OffsetDateTime::parse(text, &Rfc3339).ok()?;
    Timestamp::new(
        parsed.unix_timestamp(),
        parsed.nanosecond(),
        parsed.offset().whole_seconds(),
    )
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use super::*;

    fn convert(text: &str, ty: Type) -> Option<Value<'_>> {
        coerce(&Cow::Borrowed(text), ty)
    }

    #[test]
    fn converts_text_to_each_type() {
        assert_eq!(convert("", Type::String), Some(Value::Str("".into())));
        assert_eq!(convert("-42", Type::Int), Some(Value::Int(-42)));
        assert_eq!(convert("12.5", Type::Float), Some(Value::Float(12.5)));
        assert_eq!(convert("1e3", Type::Float), Some(Value::Float(1000.0)));
        assert_eq!(convert("true", Type::Bool), Some(Value::Bool(true)));
        assert_eq!(convert("false", Type::Bool), Some(Value::Bool(false)));
        assert_eq!(
            convert("10.0.0.1", Type::Ip),
            Some(Value::Ip(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))))
        );
        assert_eq!(
            convert("::1", Type::Ip),
            Some(Value::Ip(IpAddr::V6(Ipv6Addr::LOCALHOST)))
        );
    }

    #[test]
    fn timestamp_keeps_instant_and_offset() {
        let expected = Timestamp::new(1_700_000_000, 250_000_000, 9 * 60 * 60);
        assert_eq!(
            convert("2023-11-15T07:13:20.25+09:00", Type::Timestamp),
            expected.map(Value::Timestamp)
        );
        assert_eq!(
            convert("2023-11-14T22:13:20Z", Type::Timestamp),
            Timestamp::new(1_700_000_000, 0, 0).map(Value::Timestamp)
        );
    }

    #[test]
    fn unconvertible_text_is_absent() {
        let cases = [
            (" 1", Type::Int),
            ("1.0", Type::Int),
            ("9223372036854775808", Type::Int),
            ("", Type::Float),
            ("NaN", Type::Float),
            ("inf", Type::Float),
            ("True", Type::Bool),
            ("1", Type::Bool),
            ("2023-11-14 22:13", Type::Timestamp),
            ("1700000000", Type::Timestamp),
            ("10.0.0.256", Type::Ip),
            ("", Type::Ip),
        ];
        for (text, ty) in cases {
            assert_eq!(convert(text, ty), None, "{text:?} as {ty:?}");
        }
    }
}
