//! Inputs of the core benchmarks: records and schemas of a given width, and
//! text values of a given size.

use moldette_core::{Field, OutputSchema, Type};

/// Field counts of a record or schema.
pub const WIDTHS: [usize; 3] = [8, 64, 512];

/// Text value sizes.
pub const TEXT_SIZES: [(&str, usize); 3] = [("100B", 100), ("10KiB", 10 << 10), ("1MiB", 1 << 20)];

pub fn field_names(width: usize) -> Vec<String> {
    (0..width).map(|i| format!("field_{i}")).collect()
}

pub fn schema(width: usize) -> OutputSchema {
    field_names(width)
        .into_iter()
        .fold(OutputSchema::builder("wide", 1), |builder, name| {
            builder.field(Field::new(name, Type::String))
        })
        .build()
        .expect("valid schema")
}

#[cfg(test)]
mod tests {
    use super::{WIDTHS, field_names, schema};

    #[test]
    fn schemas_have_every_field_name() {
        for width in WIDTHS {
            let schema = schema(width);
            assert!(
                field_names(width)
                    .iter()
                    .all(|name| schema.field(name).is_some())
            );
        }
    }
}
