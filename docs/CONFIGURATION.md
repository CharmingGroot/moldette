# Configuration

A configuration file describes how moldette turns one raw record into one
record of an output schema: the schema itself, how the parser reads the raw
record, how the mapper fills each output field and how the result is checked.
This document lists every key. The examples are run by the tests of
`moldette-config`: each configuration is built, and the responses shown after
an input are the ones `moldette run` prints for it.

## File format

A configuration is written in YAML or JSON. The command and the sidecar read a
file whose name ends in `.json` as JSON and any other file as YAML.

The file is checked when it is loaded. An unknown key, a repeated key, a
missing required key or a value of the wrong kind fails with the key path of
the value, such as `mapper.rules[2].via[0].split.index`, and for YAML with its
line and column.

In YAML, a key that takes text accepts any scalar as written, so `index: 0`
and `constant: 10` need no quotes. In JSON, text is a JSON string, an integer
is a JSON number without sign or fraction, and a flag is `true` or `false`.

The top-level keys are:

| Key | Value |
|---|---|
| `schemas` | the output schemas |
| `parser` | how a raw record is read into named fields |
| `mapper` | how each field of one schema is filled from those fields |
| `resp` | how the mapped record is checked against one schema |

Each key is optional. A program can build only the sections it uses, while
`moldette run`, `moldette explain` and the sidecar need all four.

This configuration reads orders sent as JSON. Fields of the schema without a
rule in `mapper` read the parser field of the same name:

```yaml
schemas:
  - name: order
    version: 1
    fields:
      - {name: id, type: string, required: true}
      - {name: status, type: string}
      - {name: total, type: float, required: true}
      - {name: paid, type: bool}
      - {name: created_at, type: timestamp, required: true}

parser:
  format: json
  fields:
    id: /id
    status: /status
    total: /total
    paid: /paid
    created_at: /created_at

mapper:
  schema: {name: order, version: 1}

resp:
  schema: {name: order, version: 1}
```

```text input
{"id":"A-1","status":"paid","total":25.5,"paid":true,"created_at":"2026-11-18T09:30:00Z"}
{"id":"A-2","total":"n/a","created_at":"2026-11-18T10:00:00+09:00"}
{"id":"A-3","status":"open"}
not json
```

```json output
{"status":"ok","fields":{"id":"A-1","status":"paid","total":25.5,"paid":true,"created_at":"2026-11-18T09:30:00Z"},"emptied":[]}
{"status":"missing_required","fields":["total"]}
{"status":"missing_required","fields":["total","created_at"]}
{"status":"malformed","reason":"expected ident at line 1 column 2"}
```

The same configuration in JSON starts as follows:

```json config
{
  "schemas": [
    {
      "name": "order",
      "version": 1,
      "fields": [
        {"name": "id", "type": "string", "required": true},
        {"name": "total", "type": "float", "required": true}
      ]
    }
  ],
  "parser": {"format": "json", "fields": {"id": "/id", "total": "/total"}},
  "mapper": {"schema": {"name": "order", "version": 1}},
  "resp": {"schema": {"name": "order", "version": 1}}
}
```

## schemas

A list of output schemas. Each entry has these keys:

| Key | Value | Required |
|---|---|---|
| `name` | the schema name | yes |
| `version` | a non-negative integer | yes |
| `fields` | the fields, in output order | yes |

Two entries cannot share both name and version. `mapper.schema` and
`resp.schema` refer to an entry by name and version.

Each field has these keys:

| Key | Value | Required |
|---|---|---|
| `name` | the field name, unique in the schema | yes |
| `type` | one of the types below | yes |
| `required` | `true` when a record without the field is reported as `missing_required` | no, `false` |
| `deprecated` | `true` for a field kept for older readers; it is never reported as missing, and the configuration check warns when it is still mapped | no, `false` |

The mapper converts the text it reads to the field type. A text that does not
convert leaves the field without a value:

| Type | Accepted text |
|---|---|
| `string` | any text |
| `int` | a decimal 64-bit integer such as `-42`, without spaces or a fraction |
| `float` | a decimal number such as `12.5` or `1e3`; `NaN` and infinities are refused |
| `bool` | `true` or `false` |
| `timestamp` | RFC 3339, such as `2026-11-18T09:30:00Z`; the offset is kept |
| `ip` | an IPv4 or IPv6 address |

## parser

Reads one raw record into named text fields.

| Key | Value | Required |
|---|---|---|
| `format` | `json`, `delimited`, `regex`, `kv`, or a format registered by a program | yes |
| format arguments | the keys of the format, listed below | per format |
| `fields` | a map from field name to its path in the record | no |
| `bodies` | fields whose text is parsed again, listed below | no |
| `empty_as_none` | `true` to leave empty texts without a value | no, `false` |
| `none_values` | texts read as no value, such as `-` | no |

The path notation depends on the format. A path that the format cannot read is
reported when the configuration is built. A field whose path is missing from a
record, or holds a JSON `null`, has no value. A record the format cannot read
at all is reported as `malformed`, with the reason.

### json

One JSON object per record. A path is a JSON Pointer (RFC 6901) such as
`/customer/address/city` or `/items/0/sku`, and `""` is the whole record.
Strings are read without their quotes and escapes; numbers, `true`, `false`,
arrays and objects keep their JSON text. The format has no arguments; the
configuration at the start of this document uses it.

### delimited

One line per record, split into columns. A path is a zero-based column index
such as `"0"`.

| Key | Value | Required |
|---|---|---|
| `delimiter` | one character other than `"`, line feed and carriage return | yes |

A column starting with `"` is quoted: it may contain the delimiter, and `""`
inside it stands for one quote.

```yaml
schemas:
  - name: export
    version: 1
    fields:
      - {name: id, type: string, required: true}
      - {name: name, type: string}
      - {name: quantity, type: int}

parser:
  format: delimited
  delimiter: ","
  fields:
    id: "0"
    name: "1"
    quantity: "2"

mapper:
  schema: {name: export, version: 1}

resp:
  schema: {name: export, version: 1}
```

```text input
S-1,Desk lamp,3
S-2,"Chair, oak",12
S-3,"The ""Big"" box",
```

```json output
{"status":"ok","fields":{"id":"S-1","name":"Desk lamp","quantity":3},"emptied":[]}
{"status":"ok","fields":{"id":"S-2","name":"Chair, oak","quantity":12},"emptied":[]}
{"status":"ok","fields":{"id":"S-3","name":"The \"Big\" box"},"emptied":["quantity"]}
```

The empty last column of the third record is not an integer, so `quantity` is
listed in `emptied`; see [mapper](#mapper).

### regex

One record searched with a regular expression in the syntax of the Rust
`regex` crate. A path is the name of a capture group.

| Key | Value | Required |
|---|---|---|
| `pattern` | the regular expression | yes |

The pattern may match anywhere in the record, so `^` and `$` are needed to
match all of it. A record without a match is `malformed`, and a group that
takes no part in the match leaves its field without a value.

```yaml
schemas:
  - name: access
    version: 1
    fields:
      - {name: client, type: ip, required: true}
      - {name: user, type: string}
      - {name: method, type: string}
      - {name: path, type: string}
      - {name: status, type: int, required: true}
      - {name: bytes, type: int}

parser:
  format: regex
  pattern: '^(?P<client>\S+) - (?P<user>\S+) "(?P<method>\S+) (?P<path>\S+)[^"]*" (?P<status>\d+) (?P<bytes>\S+)$'
  fields:
    client: client
    user: user
    method: method
    path: path
    status: status
    bytes: bytes
  none_values: ["-"]

mapper:
  schema: {name: access, version: 1}

resp:
  schema: {name: access, version: 1}
```

```text input
10.0.0.1 - alice "GET /orders HTTP/1.1" 200 512
10.0.0.2 - - "POST /orders HTTP/1.1" 201 -
garbage
```

```json output
{"status":"ok","fields":{"client":"10.0.0.1","user":"alice","method":"GET","path":"/orders","status":200,"bytes":512},"emptied":[]}
{"status":"ok","fields":{"client":"10.0.0.2","method":"POST","path":"/orders","status":201},"emptied":[]}
{"status":"malformed","reason":"record does not match the pattern"}
```

With `none_values: ["-"]`, the `-` of the second record leaves `user` and
`bytes` without a value instead of failing to convert.

### kv

One record of key-value pairs, such as logfmt. A path is a key, and the first
pair with that key gives the value.

| Key | Value | Required |
|---|---|---|
| `pair_delimiter` | the character between pairs | yes |
| `key_value_delimiter` | the character between a key and its value | yes |

The two delimiters must differ, and neither can be `"`, line feed or carriage
return. Consecutive pair delimiters count as one, and a pair splits at the
first key-value delimiter. A value starting with `"` is quoted: it may contain
both delimiters and uses the backslash escapes of JSON strings. A key without
a key-value delimiter leaves its field without a value, and a pair with an
empty key makes the record `malformed`.

```yaml
schemas:
  - name: event
    version: 1
    fields:
      - {name: level, type: string, required: true}
      - {name: message, type: string}
      - {name: duration_ms, type: int}

parser:
  format: kv
  pair_delimiter: " "
  key_value_delimiter: "="
  fields:
    level: level
    message: msg
    duration_ms: duration_ms

mapper:
  schema: {name: event, version: 1}

resp:
  schema: {name: event, version: 1}
```

```text input
level=info msg="order saved" duration_ms=12
level=warn msg="retry \"A-2\"" duration_ms=
```

```json output
{"status":"ok","fields":{"level":"info","message":"order saved","duration_ms":12},"emptied":[]}
{"status":"ok","fields":{"level":"warn","message":"retry \"A-2\""},"emptied":["duration_ms"]}
```

### bodies

A body is the text at one path of the record, parsed by a parser of its own
and adding its fields to the record. Each entry of `bodies` has the key `path`,
in the notation of the enclosing format, and the keys of a parser section,
including `bodies` of its own. A body that its parser cannot read makes the
record `malformed`. Field names must be unique across the record and its
bodies.

```yaml
schemas:
  - name: delivery
    version: 1
    fields:
      - {name: received_at, type: timestamp, required: true}
      - {name: topic, type: string, required: true}
      - {name: order_id, type: string, required: true}
      - {name: total, type: float}

parser:
  format: delimited
  delimiter: "|"
  fields:
    received_at: "0"
    topic: "1"
  bodies:
    - path: "2"
      format: json
      fields:
        order_id: /id
        total: /total

mapper:
  schema: {name: delivery, version: 1}

resp:
  schema: {name: delivery, version: 1}
```

```text input
2026-11-18T09:30:00Z|orders|{"id":"A-1","total":25.5}
2026-11-18T09:31:00Z|orders|not json
```

```json output
{"status":"ok","fields":{"received_at":"2026-11-18T09:30:00Z","topic":"orders","order_id":"A-1","total":25.5},"emptied":[]}
{"status":"malformed","reason":"body `2`: expected ident at line 1 column 2"}
```

`empty_as_none` and `none_values` of a section also apply to the text at its
body paths, while each body applies its own settings to its fields.

## mapper

Fills each field of one schema from the parser fields.

| Key | Value | Required |
|---|---|---|
| `schema` | `{name, version}` of an entry of `schemas` | yes |
| `rules` | rules for some or all schema fields | no |
| `explicit_rules` | `true` to leave a schema field without a rule empty instead of reading the parser field of the same name | no, `false` |

A field has no value when no source text is read for it. When a source text is
read but the field still ends without a value, because an op cannot handle the
text or the text does not convert to the field type, the field is listed in
`emptied` (see [resp](#resp)).

### Rules

Each rule fills one schema field. A field has at most one rule.

| Key | Value | Required |
|---|---|---|
| `target` | the schema field | yes |
| `from` | the parser field to read | one of `from`, `coalesce` and `constant` |
| `coalesce` | parser fields; the first one present in the record is read | one of `from`, `coalesce` and `constant` |
| `constant` | a text taken without reading the record | one of `from`, `coalesce` and `constant` |
| `fallback` | the parser field read when the source gives no value, after ops and conversion | no |
| `default` | the text taken when no source text is read; it must convert to the field type | no |
| `via` | ops applied in order to the text before conversion | no |

The ops run on the fallback text as well, but not on `default`.

```yaml
schemas:
  - name: order
    version: 1
    fields:
      - {name: id, type: string, required: true}
      - {name: customer, type: string}
      - {name: channel, type: string}
      - {name: currency, type: string}
      - {name: total, type: float, required: true}

parser:
  format: json
  fields:
    order_id: /id
    legacy_id: /orderId
    customer_id: /customer/id
    email: /customer/email
    currency: /currency
    total: /total

mapper:
  schema: {name: order, version: 1}
  rules:
    - {target: id, coalesce: [order_id, legacy_id]}
    - {target: customer, from: customer_id, fallback: email}
    - {target: channel, constant: web}
    - {target: currency, from: currency, default: EUR}

resp:
  schema: {name: order, version: 1}
```

```text input
{"id":"A-1","customer":{"id":"C-100"},"currency":"USD","total":25.5}
{"orderId":"A-2","customer":{"email":"b@example.com"},"total":40}
```

```json output
{"status":"ok","fields":{"id":"A-1","customer":"C-100","channel":"web","currency":"USD","total":25.5},"emptied":[]}
{"status":"ok","fields":{"id":"A-2","customer":"b@example.com","channel":"web","currency":"EUR","total":40.0},"emptied":[]}
```

`total` has no rule and reads the parser field `total`.

### Ops

An item of `via` is the name of an op without arguments, such as `trim`, or a
map from one op name to its arguments.

| Op | Arguments | Result |
|---|---|---|
| `trim` | none | the text without leading and trailing whitespace |
| `split` | `separator`: one character; `index`: a zero-based integer | the part at `index` after splitting at `separator`; no value when there are fewer parts |
| `extract` | `pattern`: a regular expression with exactly one capture group | the text of the group in the first match; no value without a match |
| `lookup` | `table`: a map from text to text; `default`: a text, or `keep_unmatched`: `true` | the value of the text in `table`, where the first entry wins when a key repeats; for a text not in the table, `default`, the text itself with `keep_unmatched`, or else no value |
| `timestamp` | `formats`: a list of formats; `year`: an integer; `offset`: `+HH:MM` or `-HH:MM` | the text read with the first format that matches it, written as RFC 3339; no value when none matches |
| `empty_as_none` | none | no value for an empty text |

`timestamp` formats use the format description syntax, version 2, of the Rust
`time` crate, such as `[day]/[month]/[year] [hour]:[minute]`. A format that
reads no year takes `year` (up to 9999), and a format that reads no
offset takes `offset`, or UTC without it. `default` and `keep_unmatched` of
`lookup` cannot be given together.

```yaml
schemas:
  - name: order
    version: 1
    fields:
      - {name: id, type: string, required: true}
      - {name: region, type: string}
      - {name: number, type: int}
      - {name: status, type: string}
      - {name: placed_at, type: timestamp}
      - {name: note, type: string}

parser:
  format: kv
  pair_delimiter: " "
  key_value_delimiter: "="
  fields:
    id: id
    status: status
    placed: placed
    note: note

mapper:
  schema: {name: order, version: 1}
  rules:
    - {target: id, from: id, via: [trim]}
    - target: region
      from: id
      via:
        - split: {separator: "-", index: 0}
    - target: number
      from: id
      via:
        - extract: {pattern: '-(\d+)$'}
    - target: status
      from: status
      via:
        - lookup:
            table: {P: paid, H: on hold}
            default: unknown
    - target: placed_at
      from: placed
      via:
        - timestamp:
            formats: ["[day]/[month]/[year] [hour]:[minute]"]
            offset: "+09:00"
    - {target: note, from: note, via: [trim, empty_as_none]}

resp:
  schema: {name: order, version: 1}
```

```text input
id=EU-1001 status=P placed="18/11/2026 09:30" note="  gift  "
id=US-1002 status=X placed=yesterday note="   "
```

```json output
{"status":"ok","fields":{"id":"EU-1001","region":"EU","number":1001,"status":"paid","placed_at":"2026-11-18T09:30:00+09:00","note":"gift"},"emptied":[]}
{"status":"ok","fields":{"id":"US-1002","region":"US","number":1002,"status":"unknown"},"emptied":["placed_at","note"]}
```

## resp

Checks the mapped record against one schema and gives it one status.

| Key | Value | Required |
|---|---|---|
| `schema` | `{name, version}` of an entry of `schemas`, the same as `mapper.schema` | yes |
| `strict` | `true` to fail records with emptied fields | no, `false` |

| Status | When | Output |
|---|---|---|
| `malformed` | the parser cannot read the record | `reason` |
| `missing_required` | a required field that is not deprecated has no value | `fields`: the missing fields in schema order |
| `emptied` | in strict mode, a field is emptied | `fields`: the emptied fields |
| `ok` | otherwise | `fields`: the values; `emptied`: the emptied fields |

```yaml
schemas:
  - name: order
    version: 1
    fields:
      - {name: id, type: string, required: true}
      - {name: total, type: float}

parser:
  format: json
  fields:
    id: /id
    total: /total

mapper:
  schema: {name: order, version: 1}

resp:
  schema: {name: order, version: 1}
  strict: true
```

```text input
{"id":"A-1","total":25.5}
{"id":"A-2","total":"n/a"}
{"id":"A-3"}
```

```json output
{"status":"ok","fields":{"id":"A-1","total":25.5},"emptied":[]}
{"status":"emptied","fields":["total"]}
{"status":"ok","fields":{"id":"A-3"},"emptied":[]}
```

## Checks between sections

After each section is built, the sections are checked against each other.
`moldette check` prints the findings; an error fails the build and a warning
does not.

| Finding | Severity |
|---|---|
| a `from`, `coalesce` or `fallback` names a field the parser does not output | error |
| a required field has no rule and no parser field of the same name | error |
| `resp.schema` differs from `mapper.schema` | error |
| an optional field has no rule and no parser field of the same name | warning |
| no rule reads a parser field | warning |
| a deprecated field is mapped | warning |

## Formats and ops registered by programs

A program can register parser formats and ops in a `Registry` passed to
`Config::build`. A registered format takes the other keys of its parser
section as arguments, and a registered op takes the keys of its map in `via`.
The registered constructor reads its arguments, and keys it leaves unread are
rejected. Built-in names cannot be registered. The command and the sidecar
provide only the built-in formats and ops.
