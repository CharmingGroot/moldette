use moldette_core::Type;
use sha2::{Digest, Sha256};

use crate::arguments::Arguments;
use crate::model::{
    BodyEntry, Config, FieldEntry, FormatEntry, MapperSection, OpEntry, ParserSection, RespSection,
    RuleEntry, SchemaEntry, SchemaRef, SourceEntry,
};
use crate::node::{Node, NodeValue, Scalar};

pub(crate) fn digest(config: &Config) -> String {
    let mut encoder = Encoder(Sha256::new());
    encoder.config(config);
    let hex: String = encoder
        .0
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("sha256:{hex}")
}

struct Encoder(Sha256);

impl Encoder {
    fn tag(&mut self, tag: u8) {
        self.0.update([tag]);
    }

    fn number(&mut self, value: u64) {
        self.0.update(value.to_le_bytes());
    }

    fn size(&mut self, value: usize) {
        self.number(value as u64);
    }

    fn flag(&mut self, value: bool) {
        self.tag(u8::from(value));
    }

    fn character(&mut self, value: char) {
        self.number(u64::from(value));
    }

    fn text(&mut self, text: &str) {
        self.size(text.len());
        self.0.update(text.as_bytes());
    }

    fn list<T>(&mut self, items: &[T], encode: impl Fn(&mut Self, &T)) {
        self.size(items.len());
        for item in items {
            encode(self, item);
        }
    }

    fn option<T>(&mut self, item: Option<&T>, encode: impl Fn(&mut Self, &T)) {
        match item {
            Some(item) => {
                self.tag(1);
                encode(self, item);
            }
            None => self.tag(0),
        }
    }

    fn pairs(&mut self, pairs: &[(String, String)]) {
        self.list(pairs, |encoder, (key, value)| {
            encoder.text(key);
            encoder.text(value);
        });
    }

    fn config(&mut self, config: &Config) {
        let Config {
            schemas,
            parser,
            mapper,
            resp,
        } = config;
        self.list(schemas, Self::schema);
        self.option(parser.as_ref(), Self::parser);
        self.option(mapper.as_ref(), Self::mapper);
        self.option(resp.as_ref(), Self::resp);
    }

    fn schema(&mut self, entry: &SchemaEntry) {
        let SchemaEntry {
            name,
            version,
            fields,
            position: _,
        } = entry;
        self.text(name);
        self.number(u64::from(*version));
        self.list(fields, Self::field);
    }

    fn field(&mut self, entry: &FieldEntry) {
        let FieldEntry {
            name,
            ty,
            required,
            deprecated,
            position: _,
        } = entry;
        self.text(name);
        self.tag(match ty {
            Type::String => 0,
            Type::Int => 1,
            Type::Float => 2,
            Type::Bool => 3,
            Type::Timestamp => 4,
            Type::Ip => 5,
        });
        self.flag(*required);
        self.flag(*deprecated);
    }

    fn parser(&mut self, section: &ParserSection) {
        let ParserSection {
            format,
            fields,
            bodies,
            empty_as_none,
            field_positions: _,
        } = section;
        self.format(format);
        self.pairs(fields);
        self.list(bodies, Self::body);
        self.flag(*empty_as_none);
    }

    fn format(&mut self, format: &FormatEntry) {
        match format {
            FormatEntry::Json => self.tag(0),
            FormatEntry::Delimited { delimiter } => {
                self.tag(1);
                self.character(*delimiter);
            }
            FormatEntry::Regex { pattern } => {
                self.tag(2);
                self.text(pattern);
            }
            FormatEntry::Kv {
                pair_delimiter,
                key_value_delimiter,
            } => {
                self.tag(3);
                self.character(*pair_delimiter);
                self.character(*key_value_delimiter);
            }
            FormatEntry::Custom { name, arguments } => {
                self.tag(4);
                self.text(name);
                self.arguments(arguments);
            }
        }
    }

    fn body(&mut self, entry: &BodyEntry) {
        let BodyEntry { path, parser } = entry;
        self.text(path);
        self.parser(parser);
    }

    fn mapper(&mut self, section: &MapperSection) {
        let MapperSection {
            schema,
            rules,
            explicit_rules,
        } = section;
        self.schema_ref(schema);
        self.list(rules, Self::rule);
        self.flag(*explicit_rules);
    }

    fn schema_ref(&mut self, reference: &SchemaRef) {
        let SchemaRef {
            name,
            version,
            position: _,
        } = reference;
        self.text(name);
        self.number(u64::from(*version));
    }

    fn rule(&mut self, entry: &RuleEntry) {
        let RuleEntry {
            target,
            source,
            fallback,
            via,
            target_position: _,
            source_positions: _,
            fallback_position: _,
        } = entry;
        self.text(target);
        match source {
            SourceEntry::From(name) => {
                self.tag(0);
                self.text(name);
            }
            SourceEntry::Coalesce(names) => {
                self.tag(1);
                self.list(names, |encoder, name| encoder.text(name));
            }
            SourceEntry::Constant(value) => {
                self.tag(2);
                self.text(value);
            }
        }
        self.option(fallback.as_ref(), |encoder, name| encoder.text(name));
        self.list(via, Self::op);
    }

    fn op(&mut self, op: &OpEntry) {
        match op {
            OpEntry::Trim => self.tag(0),
            OpEntry::Split { separator, index } => {
                self.tag(1);
                self.character(*separator);
                self.size(*index);
            }
            OpEntry::Extract { pattern } => {
                self.tag(2);
                self.text(pattern);
            }
            OpEntry::Lookup { table } => {
                self.tag(3);
                self.pairs(table);
            }
            OpEntry::Timestamp {
                formats,
                year: None,
                offset: None,
            } => {
                self.tag(4);
                self.list(formats, |encoder, format| encoder.text(format));
            }
            OpEntry::Timestamp {
                formats,
                year,
                offset,
            } => {
                self.tag(7);
                self.list(formats, |encoder, format| encoder.text(format));
                self.option(year.as_ref(), |encoder, year| {
                    encoder.text(&year.to_string())
                });
                self.option(offset.as_ref(), |encoder, offset| encoder.text(offset));
            }
            OpEntry::EmptyAsNone => self.tag(5),
            OpEntry::Custom { name, arguments } => {
                self.tag(6);
                self.text(name);
                self.arguments(arguments);
            }
        }
    }

    fn resp(&mut self, section: &RespSection) {
        let RespSection { schema, strict } = section;
        self.schema_ref(schema);
        self.flag(*strict);
    }

    fn arguments(&mut self, arguments: &Arguments) {
        self.list(arguments.entries(), |encoder, (key, node)| {
            encoder.text(&key.text);
            encoder.node(node);
        });
    }

    fn node(&mut self, node: &Node) {
        match &node.value {
            NodeValue::Map(entries) => {
                self.tag(0);
                self.list(entries, |encoder, (key, node)| {
                    encoder.text(&key.text);
                    encoder.node(node);
                });
            }
            NodeValue::Sequence(items) => {
                self.tag(1);
                self.list(items, Self::node);
            }
            NodeValue::Scalar(Scalar::Plain(text)) => {
                self.tag(2);
                self.text(text);
            }
            NodeValue::Scalar(Scalar::Text(text)) => {
                self.tag(3);
                self.text(text);
            }
            NodeValue::Scalar(Scalar::Number(text)) => {
                self.tag(4);
                self.text(text);
            }
            NodeValue::Scalar(Scalar::Bool(value)) => {
                self.tag(5);
                self.flag(*value);
            }
            NodeValue::Scalar(Scalar::Null) => self.tag(6),
        }
    }
}
