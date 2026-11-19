use std::borrow::Cow;

use crate::Value;

/// Values produced by a mapper, keyed by output field name, with the fields
/// that ended up absent because an op or a conversion could not handle the
/// input (#5).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mapped<'a> {
    values: Vec<(Cow<'a, str>, Value<'a>)>,
    emptied: Vec<Cow<'a, str>>,
}

impl<'a> Mapped<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reserves room for `capacity` values, so that a mapper that knows the
    /// number of schema fields inserts them without growing (#66).
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            values: Vec::with_capacity(capacity),
            emptied: Vec::new(),
        }
    }

    pub fn insert(&mut self, name: impl Into<Cow<'a, str>>, value: Value<'a>) {
        self.values.push((name.into(), value));
    }

    pub fn mark_emptied(&mut self, name: impl Into<Cow<'a, str>>) {
        self.emptied.push(name.into());
    }

    pub fn get(&self, name: &str) -> Option<&Value<'a>> {
        self.values.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value<'a>)> {
        self.values.iter().map(|(n, v)| (n.as_ref(), v))
    }

    pub fn emptied(&self) -> impl Iterator<Item = &str> {
        self.emptied.iter().map(AsRef::as_ref)
    }

    pub fn into_owned(self) -> Mapped<'static> {
        Mapped {
            values: self
                .values
                .into_iter()
                .map(|(n, v)| (Cow::Owned(n.into_owned()), v.into_owned()))
                .collect(),
            emptied: self
                .emptied
                .into_iter()
                .map(|n| Cow::Owned(n.into_owned()))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emptied_fields_are_reported_and_absent() {
        let mut mapped = Mapped::new();
        mapped.insert("amount", Value::Float(12.5));
        mapped.mark_emptied("ordered_at");
        assert_eq!(mapped.get("amount"), Some(&Value::Float(12.5)));
        assert_eq!(mapped.get("ordered_at"), None);
        assert_eq!(mapped.emptied().collect::<Vec<_>>(), ["ordered_at"]);
    }

    #[test]
    fn with_capacity_starts_empty() {
        let mapped = Mapped::with_capacity(8);
        assert_eq!(mapped, Mapped::new());
        assert_eq!(mapped.iter().count(), 0);
    }
}
