use moldette_core::Type;

use crate::arguments::Arguments;
use crate::cursor::{Cursor, MapReader};
use crate::error::{LoadError, LoadErrorKind, ValueKind};
use crate::node::{Node, NodeValue, Scalar, key_path, reject_duplicate_keys};
use crate::position::Position;
use crate::{json, yaml};

/// A configuration file read into its sections. Keys follow the names of the
/// layer builder methods (#14); a missing section is `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Config {
    pub schemas: Vec<SchemaEntry>,
    pub parser: Option<ParserSection>,
    pub mapper: Option<MapperSection>,
    pub resp: Option<RespSection>,
}

#[derive(Debug, Clone, Eq)]
#[non_exhaustive]
pub struct SchemaEntry {
    pub name: String,
    pub version: u32,
    pub fields: Vec<FieldEntry>,
    pub(crate) position: Option<Position>,
}

impl PartialEq for SchemaEntry {
    fn eq(&self, other: &Self) -> bool {
        (&self.name, self.version, &self.fields) == (&other.name, other.version, &other.fields)
    }
}

/// One output field, written with the key `type` for `ty`.
#[derive(Debug, Clone, Eq)]
#[non_exhaustive]
pub struct FieldEntry {
    pub name: String,
    pub ty: Type,
    pub required: bool,
    pub deprecated: bool,
    pub(crate) position: Option<Position>,
}

impl PartialEq for FieldEntry {
    fn eq(&self, other: &Self) -> bool {
        (&self.name, self.ty, self.required, self.deprecated)
            == (&other.name, other.ty, other.required, other.deprecated)
    }
}

#[derive(Debug, Clone, Eq)]
#[non_exhaustive]
pub struct ParserSection {
    pub format: FormatEntry,
    /// Field names and their paths in the raw record, in declared order.
    pub fields: Vec<(String, String)>,
    pub bodies: Vec<BodyEntry>,
    pub empty_as_none: bool,
    pub(crate) field_positions: Vec<Option<Position>>,
}

impl PartialEq for ParserSection {
    fn eq(&self, other: &Self) -> bool {
        (&self.format, &self.fields, &self.bodies, self.empty_as_none)
            == (
                &other.format,
                &other.fields,
                &other.bodies,
                other.empty_as_none,
            )
    }
}

/// The `format` key with its sibling argument keys.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FormatEntry {
    Json,
    Delimited {
        delimiter: char,
    },
    Regex {
        pattern: String,
    },
    Kv {
        pair_delimiter: char,
        key_value_delimiter: char,
    },
    /// A parser registered by the caller, with the remaining keys of the
    /// section as its arguments.
    Custom {
        name: String,
        arguments: Arguments,
    },
}

/// A body parsed from the text at `path`, written as `path` next to the keys
/// of a parser section.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BodyEntry {
    pub path: String,
    pub parser: ParserSection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct MapperSection {
    pub schema: SchemaRef,
    pub rules: Vec<RuleEntry>,
    pub explicit_rules: bool,
}

/// A reference to an entry of `schemas`. The version is always given (#14).
#[derive(Debug, Clone, Eq)]
#[non_exhaustive]
pub struct SchemaRef {
    pub name: String,
    pub version: u32,
    pub(crate) position: Option<Position>,
}

impl PartialEq for SchemaRef {
    fn eq(&self, other: &Self) -> bool {
        (&self.name, self.version) == (&other.name, other.version)
    }
}

#[derive(Debug, Clone, Eq)]
#[non_exhaustive]
pub struct RuleEntry {
    pub target: String,
    pub source: SourceEntry,
    pub fallback: Option<String>,
    pub via: Vec<OpEntry>,
    pub(crate) target_position: Option<Position>,
    pub(crate) source_positions: Vec<Option<Position>>,
    pub(crate) fallback_position: Option<Position>,
}

impl PartialEq for RuleEntry {
    fn eq(&self, other: &Self) -> bool {
        (&self.target, &self.source, &self.fallback, &self.via)
            == (&other.target, &other.source, &other.fallback, &other.via)
    }
}

/// The source of a rule: exactly one of `from`, `coalesce` and `constant`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SourceEntry {
    From(String),
    Coalesce(Vec<String>),
    Constant(String),
}

/// One item of `via`: an op name, or a map from one op name to its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpEntry {
    Trim,
    Split {
        separator: char,
        index: usize,
    },
    Extract {
        pattern: String,
    },
    /// Entries in declared order.
    Lookup {
        table: Vec<(String, String)>,
    },
    /// `year` and `offset` fill what a format does not read (#95).
    Timestamp {
        formats: Vec<String>,
        year: Option<i32>,
        offset: Option<String>,
    },
    EmptyAsNone,
    /// An op registered by the caller.
    Custom {
        name: String,
        arguments: Arguments,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RespSection {
    pub schema: SchemaRef,
    pub strict: bool,
}

const PARSER_KEYS: [&str; 4] = ["format", "fields", "bodies", "empty_as_none"];

pub(crate) const BUILT_IN_FORMATS: [(&str, &[&str]); 4] = [
    ("json", &[]),
    ("delimited", &["delimiter"]),
    ("regex", &["pattern"]),
    ("kv", &["pair_delimiter", "key_value_delimiter"]),
];

pub(crate) const BUILT_IN_OPS: [(&str, &[&str]); 6] = [
    ("trim", &[]),
    ("split", &["separator", "index"]),
    ("extract", &["pattern"]),
    ("lookup", &["table"]),
    ("timestamp", &["formats", "year", "offset"]),
    ("empty_as_none", &[]),
];

pub(crate) fn built_in_keys(
    built_in: &[(&'static str, &'static [&'static str])],
    name: &str,
) -> Option<&'static [&'static str]> {
    built_in
        .iter()
        .find(|(built_in_name, _)| *built_in_name == name)
        .map(|(_, keys)| *keys)
}

impl Config {
    pub fn from_yaml(text: &str) -> Result<Self, LoadError> {
        Self::decode(&yaml::read(text)?)
    }

    pub fn from_json(text: &str) -> Result<Self, LoadError> {
        Self::decode(&json::read(text)?)
    }

    fn decode(root: &Node) -> Result<Self, LoadError> {
        reject_duplicate_keys(root, "")?;
        let map = Cursor::new(root, String::new()).map()?;
        map.allow(&["schemas", "parser", "mapper", "resp"])?;
        Ok(Self {
            schemas: map
                .get("schemas")
                .map(|cursor| cursor.list(schema))
                .transpose()?
                .unwrap_or_default(),
            parser: map
                .get("parser")
                .map(|cursor| parser(&cursor.map()?, &[]))
                .transpose()?,
            mapper: map.get("mapper").map(mapper).transpose()?,
            resp: map.get("resp").map(resp).transpose()?,
        })
    }
}

fn schema(cursor: Cursor<'_>) -> Result<SchemaEntry, LoadError> {
    let position = cursor.node.position;
    let map = cursor.map()?;
    map.allow(&["name", "version", "fields"])?;
    Ok(SchemaEntry {
        name: map.require("name")?.string()?,
        version: map.require("version")?.integer()?,
        fields: map.require("fields")?.list(field)?,
        position,
    })
}

fn field(cursor: Cursor<'_>) -> Result<FieldEntry, LoadError> {
    let position = cursor.node.position;
    let map = cursor.map()?;
    map.allow(&["name", "type", "required", "deprecated"])?;
    let ty = map.require("type")?;
    Ok(FieldEntry {
        name: map.require("name")?.string()?,
        ty: field_type(&ty)?,
        required: map.flag("required")?,
        deprecated: map.flag("deprecated")?,
        position,
    })
}

fn field_type(cursor: &Cursor<'_>) -> Result<Type, LoadError> {
    let name = cursor.string()?;
    Ok(match name.as_str() {
        "string" => Type::String,
        "int" => Type::Int,
        "float" => Type::Float,
        "bool" => Type::Bool,
        "timestamp" => Type::Timestamp,
        "ip" => Type::Ip,
        _ => return Err(cursor.error(LoadErrorKind::UnknownFieldType(name))),
    })
}

fn parser(map: &MapReader<'_>, extra_keys: &[&str]) -> Result<ParserSection, LoadError> {
    let format = map.require("format")?;
    let name = format.string()?;
    let Some(format_keys) = built_in_keys(&BUILT_IN_FORMATS, &name) else {
        let format = FormatEntry::Custom {
            arguments: Arguments::from_map(
                map,
                &[&PARSER_KEYS[..], extra_keys].concat(),
                format.node.position,
            ),
            name,
        };
        return parser_section(map, format);
    };
    map.allow(&[&PARSER_KEYS[..], extra_keys, format_keys].concat())?;
    let format = match name.as_str() {
        "delimited" => FormatEntry::Delimited {
            delimiter: map.require("delimiter")?.character()?,
        },
        "regex" => FormatEntry::Regex {
            pattern: map.require("pattern")?.string()?,
        },
        "kv" => FormatEntry::Kv {
            pair_delimiter: map.require("pair_delimiter")?.character()?,
            key_value_delimiter: map.require("key_value_delimiter")?.character()?,
        },
        _ => FormatEntry::Json,
    };
    parser_section(map, format)
}

fn parser_section(map: &MapReader<'_>, format: FormatEntry) -> Result<ParserSection, LoadError> {
    let (fields, field_positions) = map
        .get("fields")
        .map(located_string_map)
        .transpose()?
        .unwrap_or_default()
        .into_iter()
        .unzip();
    Ok(ParserSection {
        format,
        fields,
        field_positions,
        bodies: map
            .get("bodies")
            .map(|cursor| cursor.list(body))
            .transpose()?
            .unwrap_or_default(),
        empty_as_none: map.flag("empty_as_none")?,
    })
}

fn body(cursor: Cursor<'_>) -> Result<BodyEntry, LoadError> {
    let map = cursor.map()?;
    Ok(BodyEntry {
        path: map.require("path")?.string()?,
        parser: parser(&map, &["path"])?,
    })
}

fn mapper(cursor: Cursor<'_>) -> Result<MapperSection, LoadError> {
    let map = cursor.map()?;
    map.allow(&["schema", "rules", "explicit_rules"])?;
    Ok(MapperSection {
        schema: schema_ref(map.require("schema")?)?,
        rules: map
            .get("rules")
            .map(|cursor| cursor.list(rule))
            .transpose()?
            .unwrap_or_default(),
        explicit_rules: map.flag("explicit_rules")?,
    })
}

fn resp(cursor: Cursor<'_>) -> Result<RespSection, LoadError> {
    let map = cursor.map()?;
    map.allow(&["schema", "strict"])?;
    Ok(RespSection {
        schema: schema_ref(map.require("schema")?)?,
        strict: map.flag("strict")?,
    })
}

fn schema_ref(cursor: Cursor<'_>) -> Result<SchemaRef, LoadError> {
    let position = cursor.node.position;
    let map = cursor.map()?;
    map.allow(&["name", "version"])?;
    Ok(SchemaRef {
        name: map.require("name")?.string()?,
        version: map.require("version")?.integer()?,
        position,
    })
}

fn rule(cursor: Cursor<'_>) -> Result<RuleEntry, LoadError> {
    let map = cursor.map()?;
    map.allow(&["target", "from", "coalesce", "constant", "fallback", "via"])?;
    let (source, source_positions) = match map.one_of(&["from", "coalesce", "constant"])? {
        ("from", cursor) => (
            SourceEntry::From(cursor.string()?),
            vec![cursor.node.position],
        ),
        ("coalesce", cursor) => {
            let (names, positions) = cursor.list(located_string)?.into_iter().unzip();
            (SourceEntry::Coalesce(names), positions)
        }
        (_, cursor) => (SourceEntry::Constant(cursor.string()?), Vec::new()),
    };
    let (fallback, fallback_position) = match map.get("fallback") {
        Some(cursor) => (Some(cursor.string()?), cursor.node.position),
        None => (None, None),
    };
    let target = map.require("target")?;
    Ok(RuleEntry {
        target: target.string()?,
        target_position: target.node.position,
        source,
        fallback,
        source_positions,
        fallback_position,
        via: map
            .get("via")
            .map(|cursor| cursor.list(op))
            .transpose()?
            .unwrap_or_default(),
    })
}

fn op(cursor: Cursor<'_>) -> Result<OpEntry, LoadError> {
    match &cursor.node.value {
        NodeValue::Scalar(Scalar::Plain(name) | Scalar::Text(name)) => {
            let empty = Node {
                value: NodeValue::Map(Vec::new()),
                position: cursor.node.position,
            };
            op_with_arguments(name, Cursor::new(&empty, key_path(&cursor.path, name)))
        }
        NodeValue::Map(_) => {
            let map = cursor.map()?;
            let mut entries = map.entries();
            match (entries.next(), entries.next()) {
                (Some((name, arguments)), None) => op_with_arguments(name, arguments),
                _ => Err(map.conflict()),
            }
        }
        _ => Err(cursor.error(LoadErrorKind::TypeMismatch(ValueKind::Op))),
    }
}

fn op_with_arguments(name: &str, cursor: Cursor<'_>) -> Result<OpEntry, LoadError> {
    let position = cursor.node.position;
    let map = cursor.map()?;
    let Some(keys) = built_in_keys(&BUILT_IN_OPS, name) else {
        return Ok(OpEntry::Custom {
            name: name.to_owned(),
            arguments: Arguments::from_map(&map, &[], position),
        });
    };
    map.allow(keys)?;
    Ok(match name {
        "split" => OpEntry::Split {
            separator: map.require("separator")?.character()?,
            index: map.require("index")?.integer()?,
        },
        "extract" => OpEntry::Extract {
            pattern: map.require("pattern")?.string()?,
        },
        "lookup" => OpEntry::Lookup {
            table: string_map(map.require("table")?)?,
        },
        "timestamp" => OpEntry::Timestamp {
            formats: map.require("formats")?.list(|item| item.string())?,
            year: map.get("year").map(|year| year.integer()).transpose()?,
            offset: map
                .get("offset")
                .map(|offset| offset.string())
                .transpose()?,
        },
        "trim" => OpEntry::Trim,
        _ => OpEntry::EmptyAsNone,
    })
}

fn string_map(cursor: Cursor<'_>) -> Result<Vec<(String, String)>, LoadError> {
    cursor
        .map()?
        .entries()
        .map(|(key, value)| Ok((key.to_owned(), value.string()?)))
        .collect()
}

type Located<T> = (T, Option<Position>);

fn located_string_map(cursor: Cursor<'_>) -> Result<Vec<Located<(String, String)>>, LoadError> {
    cursor
        .map()?
        .entries()
        .map(|(key, value)| Ok(((key.to_owned(), value.string()?), value.node.position)))
        .collect()
}

fn located_string(cursor: Cursor<'_>) -> Result<Located<String>, LoadError> {
    Ok((cursor.string()?, cursor.node.position))
}
