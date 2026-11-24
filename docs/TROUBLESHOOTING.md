# Troubleshooting

This page explains the status of a response, how to find why a field has no
value, and what the messages of `moldette check` mean. The configuration keys
are described in `docs/CONFIGURATION.md`.

## Response statuses

Every record ends with one status:

| Status | Meaning | What to look at |
|---|---|---|
| `ok` | the record maps onto the schema | `emptied` lists the fields whose text was read but did not survive |
| `emptied` | strict mode is on and a field was emptied | the fields listed |
| `missing_required` | a required field has no value | why nothing was read for the field, or why its text was rejected |
| `malformed` | the parser could not read the record | `reason`, and the record itself |

A required field whose text is rejected has no value, so the record is
`missing_required` rather than `ok` with the field in `emptied`.

## Why a field has no value

Run the record through `moldette explain`. For each field it shows the source
read, the text found, the output of each op and the converted value:

```text
{"name":"total","reads":[{"source":{"field":"total"},"steps":[],"text":"12,5","value":null}]}
{"name":"placed_at","reads":[{"source":{"field":"placed"},"steps":[{"op":"timestamp","output":null}],"text":"2026-11-18 09:30","value":null}]}
```

Read it from left to right:

- **No read at all**: the parser did not output the field for this record.
  Check the path in `parser.fields`, and whether the record has the field.
- **A text, then an op with `output` null**: that op could not handle the
  text. Above, the `timestamp` format `[day]/[month]/[year] [hour]:[minute]`
  does not match `2026-11-18 09:30`; add a format that does. A format must
  read a time of day: a date alone, such as `2026-11-18`, does not become a
  timestamp.
- **A text, ops that pass, then `value` null**: the text does not convert to
  the field type. Above, `12,5` is not a `float`. The conversions are strict:
  `int` refuses `1.0` and surrounding spaces, `bool` takes only `true` and
  `false`, `timestamp` takes RFC 3339 only.
- **A text that should count as no value**, such as `-` or an empty string:
  use `none_values` or `empty_as_none` on the parser, or the `empty_as_none`
  op, so that the field is absent instead of emptied.

A `default` applies only when no text is read: a text that is read and then
rejected leaves the field emptied, not defaulted. A `fallback` is read whenever
the source gives no value, including when its text is rejected.

## Why a record is malformed

| Format | Common reasons |
|---|---|
| `json` | the record is not one JSON object, such as a partial line or an array |
| `delimited` | an unterminated quoted column |
| `regex` | the pattern does not match the record; the pattern may match anywhere, so anchor it with `^` and `$` and check each group against a failing line |
| `kv` | a pair with an empty key |
| bodies | the text at a body path is not in the body's format; the reason names the body path |

## Messages of `moldette check`

Errors stop the configuration from being built; warnings do not. Each message
names the file, the key path and, for YAML, the line and column:

```text
error: order.yaml: parser: unknown key `separator` at line 10, column 3
```

| Message | Cause | Fix |
|---|---|---|
| ``unknown key `separator` `` | a key the section does not have, often a typo or a key of another format | see the keys of that section; `delimited` takes `delimiter` |
| ``missing one of `from`, `coalesce`, `constant` `` | a rule without a source | give exactly one of them |
| ``keys `default` and `keep_unmatched` cannot be used together`` | two keys that exclude each other | keep one |
| ``expected …`` | a value of the wrong kind, such as a list where text is expected | in JSON, text must be a string and an integer a number |
| ``unknown field type `number` `` | not one of `string`, `int`, `float`, `bool`, `timestamp`, `ip` | use one of them |
| ``schema `t` version 2 is not defined`` | `mapper.schema` or `resp.schema` names a schema that `schemas` does not declare | match the name and version |
| ``field `x` is not in the output schema`` | a rule targets a field the schema does not have | add the field or fix the target |
| ``the parser outputs no field `amt` `` | `from`, `coalesce` or `fallback` names a field the parser does not output | add it to `parser.fields` or fix the name |
| ``required field `x` has no source in any record`` | a required field has no rule and no parser field of the same name | add a rule or a parser field |
| ``unknown op `x` `` / ``unknown format `x` `` | not a built-in op or format | the command and the sidecar provide only the built-in ones |
| ``no rule reads field `x` `` (warning) | a parser field nothing uses | remove it, or map it |
| ``field `x` has no source in any record`` (warning) | an optional field that can never be filled | add a source, or remove the field |
| ``deprecated field `x` is mapped`` (warning) | a field marked `deprecated` still gets a value | stop mapping it when readers no longer need it |

Op arguments are checked too: an `extract` pattern needs exactly one capture
group, a `timestamp` format must be valid, and a `default` must convert to the
field type.

## The sidecar

| Symptom | Cause |
|---|---|
| 404 `no configuration is named` | the name in `/v1/process/<name>` is not one given at start |
| 404 `name the configuration` | named configurations are running and the request went to `/v1/process` |
| 413 | the declared body is larger than `--max-body-bytes` |
| 503 with `Retry-After` | `--max-requests` requests are in progress |
| last line `{"error":{"record":N,...}}` | the body could not be read to the end, a record is longer than `--max-record-bytes`, or the request took longer than `--timeout`; the responses before it are complete |
| the request stalls, then ends with the time limit | the client sends the whole body before reading the response, and the responses outgrew `--max-buffer-bytes`; read the response while sending |

See `docs/SIDECAR.md` for the options.
