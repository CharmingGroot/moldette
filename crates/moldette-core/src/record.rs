use std::borrow::Cow;

/// Fields extracted by a parser, as text. Values borrow from the raw record
/// where possible.
///
/// Absent fields are not stored. When a name is inserted more than once,
/// [`FieldRecord::get`] returns the first value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FieldRecord<'a> {
    fields: Vec<(Cow<'a, str>, Cow<'a, str>)>,
}

impl<'a> FieldRecord<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            fields: Vec::with_capacity(capacity),
        }
    }

    pub fn insert(&mut self, name: impl Into<Cow<'a, str>>, value: impl Into<Cow<'a, str>>) {
        self.fields.push((name.into(), value.into()));
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_ref())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.fields.iter().map(|(n, v)| (n.as_ref(), v.as_ref()))
    }

    pub fn len(&self) -> usize {
        self.fields.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    pub fn into_owned(self) -> FieldRecord<'static> {
        FieldRecord {
            fields: self
                .fields
                .into_iter()
                .map(|(n, v)| (Cow::Owned(n.into_owned()), Cow::Owned(v.into_owned())))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_string_is_a_value_and_missing_is_absent() {
        let mut record = FieldRecord::new();
        record.insert("memo", "");
        assert_eq!(record.get("memo"), Some(""));
        assert_eq!(record.get("status"), None);
    }

    #[test]
    fn first_value_wins_for_repeated_names() {
        let mut record = FieldRecord::new();
        record.insert("a", "1");
        record.insert("a", "2");
        assert_eq!(record.get("a"), Some("1"));
        assert_eq!(record.len(), 2);
    }

    #[test]
    fn borrows_until_converted_to_owned() {
        let raw = String::from("id=A-1");
        let mut record = FieldRecord::new();
        record.insert("id", &raw[3..]);
        let owned: FieldRecord<'static> = record.into_owned();
        drop(raw);
        assert_eq!(owned.get("id"), Some("A-1"));
    }
}
