use crate::SpecError;

/// Types an output field can have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Type {
    String,
    Int,
    Float,
    Bool,
    Timestamp,
    Ip,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    name: String,
    ty: Type,
    required: bool,
    deprecated: bool,
}

impl Field {
    pub fn new(name: impl Into<String>, ty: Type) -> Self {
        Self {
            name: name.into(),
            ty,
            required: false,
            deprecated: false,
        }
    }

    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    pub fn deprecated(mut self) -> Self {
        self.deprecated = true;
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn ty(&self) -> Type {
        self.ty
    }

    pub fn is_required(&self) -> bool {
        self.required
    }

    pub fn is_deprecated(&self) -> bool {
        self.deprecated
    }
}

/// A named, versioned set of output fields. Immutable once built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputSchema {
    name: String,
    version: u32,
    fields: Vec<Field>,
}

impl OutputSchema {
    pub fn builder(name: impl Into<String>, version: u32) -> OutputSchemaBuilder {
        OutputSchemaBuilder {
            schema: Self {
                name: name.into(),
                version,
                fields: Vec::new(),
            },
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }
}

pub struct OutputSchemaBuilder {
    schema: OutputSchema,
}

impl OutputSchemaBuilder {
    pub fn field(mut self, field: Field) -> Self {
        self.schema.fields.push(field);
        self
    }

    pub fn build(self) -> Result<OutputSchema, SpecError> {
        for (i, field) in self.schema.fields.iter().enumerate() {
            if field.name.is_empty() {
                return Err(SpecError::EmptyFieldName);
            }
            if self.schema.fields[..i].iter().any(|f| f.name == field.name) {
                return Err(SpecError::DuplicateField(field.name.clone()));
            }
        }
        Ok(self.schema)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_fields_in_declared_order() {
        let schema = OutputSchema::builder("order", 1)
            .field(Field::new("order_id", Type::String).required())
            .field(Field::new("amount", Type::Float))
            .build()
            .expect("valid schema");
        let names: Vec<_> = schema.fields().iter().map(Field::name).collect();
        assert_eq!(names, ["order_id", "amount"]);
        assert!(schema.field("order_id").is_some_and(Field::is_required));
    }

    #[test]
    fn rejects_duplicate_and_empty_names() {
        let duplicate = OutputSchema::builder("order", 1)
            .field(Field::new("id", Type::String))
            .field(Field::new("id", Type::Int))
            .build();
        assert_eq!(duplicate, Err(SpecError::DuplicateField("id".into())));

        let empty = OutputSchema::builder("order", 1)
            .field(Field::new("", Type::String))
            .build();
        assert_eq!(empty, Err(SpecError::EmptyFieldName));
    }
}
