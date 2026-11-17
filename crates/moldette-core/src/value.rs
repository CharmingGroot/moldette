use std::borrow::Cow;
use std::net::IpAddr;

use crate::Timestamp;

/// A value converted to an output schema type.
#[derive(Debug, Clone, PartialEq)]
pub enum Value<'a> {
    Str(Cow<'a, str>),
    Int(i64),
    Float(f64),
    Bool(bool),
    Timestamp(Timestamp),
    Ip(IpAddr),
}

impl Value<'_> {
    pub fn into_owned(self) -> Value<'static> {
        match self {
            Self::Str(s) => Value::Str(Cow::Owned(s.into_owned())),
            Self::Int(v) => Value::Int(v),
            Self::Float(v) => Value::Float(v),
            Self::Bool(v) => Value::Bool(v),
            Self::Timestamp(v) => Value::Timestamp(v),
            Self::Ip(v) => Value::Ip(v),
        }
    }
}
