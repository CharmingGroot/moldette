use crate::SpecError;

/// Checks that every name is non-empty and appears once. Shared by every
/// builder that declares named fields.
pub fn validate_field_names<'n>(names: impl IntoIterator<Item = &'n str>) -> Result<(), SpecError> {
    let mut seen: Vec<&str> = Vec::new();
    for name in names {
        if name.is_empty() {
            return Err(SpecError::EmptyFieldName);
        }
        if seen.contains(&name) {
            return Err(SpecError::DuplicateField(name.to_owned()));
        }
        seen.push(name);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_and_repeated_names() {
        assert_eq!(validate_field_names(["a", "b"]), Ok(()));
        assert_eq!(
            validate_field_names(["a", ""]),
            Err(SpecError::EmptyFieldName)
        );
        assert_eq!(
            validate_field_names(["a", "b", "a"]),
            Err(SpecError::DuplicateField("a".into()))
        );
    }
}
