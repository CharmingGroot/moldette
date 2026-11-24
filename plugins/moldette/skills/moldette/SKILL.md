---
name: moldette
description: Write, check, debug and deploy moldette configurations, which turn raw records (JSON, CSV and other delimited text, logs read with a regular expression such as nginx or syslog lines, key=value pairs) into records of a declared output schema. Use this skill whenever the user wants to normalize or map records with moldette, writes or edits a moldette YAML or JSON configuration, asks why a field comes out as `emptied`, `missing_required` or `malformed`, sees an error from `moldette check`, or runs `moldette-sidecar` (several configurations, metrics, Kubernetes), even if they only paste sample records and describe the fields they want.
---

# moldette

moldette turns one raw record into one record of a declared output schema in
three steps: a **parser** reads the record into named text fields, a **mapper**
fills each output field from those fields, and **resp** checks the result and
gives it one status. A configuration file in YAML or JSON describes all three.
It has no expression language: what a configuration cannot express is done by
the caller, or by a Rust program that registers its own formats and ops.

The `moldette` command checks a configuration and runs it over records;
`moldette-sidecar` serves the same over HTTP next to an application. Check
the installed version with `moldette --version`: this skill describes 0.8, and
notes where 0.7 differs.

## Working on a configuration

Build the configuration from real records, and let the tool tell you what is
wrong. Each step below is cheap, and skipping them is how configurations end up
emptying fields that looked fine on paper.

1. **Collect samples.** Ask for, or find, 3 to 10 raw records, including odd
   ones: missing fields, `-` or empty values, other timestamp forms, a line
   that is not a record. Ask which output fields are wanted and which of them
   every record must have.

2. **Pick the parser format** from the shape of one record:

   | Record | Format | Field path |
   |---|---|---|
   | one JSON object | `json` | JSON Pointer: `/customer/id`, `/items/0/sku` |
   | CSV or other delimited line | `delimited` with `delimiter` | column index as text: `"0"`, `"1"` |
   | log line with a fixed layout (nginx, syslog) | `regex` with `pattern` | name of a capture group |
   | `key=value` pairs, logfmt | `kv` with `pair_delimiter`, `key_value_delimiter` | the key |

   A field that holds text in another format, such as a JSON body after a
   delimited header, is parsed again with `bodies`.

3. **Write the configuration** in the order schema, parser, mapper, resp. Start
   small: a schema field with no mapper rule reads the parser field of the same
   name, so name parser fields after schema fields where you can and add rules
   only where a value needs a different source, a fallback, a default or ops.
   The keys are summarized in `references/configuration.md`.

4. **Check it**: `moldette check config.yaml`. Errors carry the key path and,
   for YAML, the line and column; fix them one by one. Warnings (a parser field
   no rule reads, an optional field with no source) usually mean a typo or a
   leftover. `references/troubleshooting.md` lists the common messages.

5. **Run it on the samples**: `moldette run config.yaml samples.ndjson` prints
   one JSON response per record and a status count on standard error.
   `moldette explain` adds, for each field, what was read and what each op
   produced, which is the fastest way to see why a field is empty.

6. **Iterate** until every sample gives the expected status. An `emptied`
   field means a text was read but did not survive the ops or the type
   conversion; a `missing_required` field means nothing was read for it.

7. **Choose how it runs**: in a Rust program through the `moldette` crate,
   over files and pipes with the `moldette` command, or next to an application
   with `moldette-sidecar`. For the sidecar, read `references/sidecar.md`, and
   take option names, paths and defaults from it or from
   `moldette-sidecar --help` rather than from other tools' conventions: the
   processing endpoints are `/v1/process` and `/v1/process/<name>`, and the
   configurations are positional `<name>=<path>` arguments. From 0.8, a
   sidecar can also read named configurations from an `http` URL with
   `--config-source` and check one with `POST /v1/check`, for configurations
   that change while it runs.

Options take their value as the next argument (`--threads 4`). moldette 0.8
also accepts `--threads=4`; moldette 0.7 does not, so a manifest written with
the joined form fails to start on 0.7. Write the separate form when the
version is unknown.

## Rules that are easy to get wrong

- **Types convert strictly.** `int` refuses `1.0` and ` 1`; `bool` takes only
  `true` and `false`; `float` takes a decimal point only, so `12,5` is
  refused, and no built-in op rewrites text to fix that; `timestamp` takes RFC
  3339 only, so other forms need the `timestamp` op with a format. A text that
  does not convert leaves the field without a value.
- **`timestamp` formats** use the format description syntax of the Rust `time`
  crate (`[day]/[month repr:short]/[year]:[hour]:[minute]:[second]`), not
  strftime (`%d/%b/%Y`); a strftime pattern passes `moldette check` but matches
  nothing. A format without a time of day, such as `[year]-[month]-[day]`,
  reads the start of the day; moldette 0.7 gives no value for it, so there
  include a time in the format or keep the field a `string`.
- **`fallback` versus `coalesce`.** `fallback` is read when the source gives no
  value after its ops and conversion. `coalesce` picks the first source field
  present in the record and does not move on when that value fails to convert.
- **`default` applies only when no source text is read.** An empty string is a
  text, so it is converted (and usually emptied) rather than replaced by the
  default; use `empty_as_none` or `none_values` for such values.
- **JSON values keep their JSON text.** A JSON number `40` reaches the mapper as
  the text `40`, and an array or object as its JSON text, which a `string`
  field keeps as is.
- **One record at a time.** Header rows, joins, aggregation and lookups against
  external data are the caller's. A CSV header line is read as a record, so
  remove it before the records reach moldette.
- **In YAML, quote what YAML would misread**: column indexes and lookup keys
  that look like numbers are text in moldette, and regular expressions are
  safest in single quotes.

## Example

A JSON order record mapped onto a schema, with one rule that reads a nested
field and falls back to another:

```yaml
schemas:
  - name: order
    version: 1
    fields:
      - {name: id, type: string, required: true}
      - {name: customer, type: string}
      - {name: total, type: float, required: true}
      - {name: created_at, type: timestamp, required: true}

parser:
  format: json
  fields:
    id: /id
    customer_id: /customer/id
    email: /customer/email
    total: /total
    created_at: /created_at

mapper:
  schema: {name: order, version: 1}
  rules:
    - {target: customer, from: customer_id, fallback: email}

resp:
  schema: {name: order, version: 1}
```

```text input
{"id":"A-1","customer":{"id":"C-100"},"total":25.5,"created_at":"2026-11-18T09:30:00Z"}
{"id":"A-2","customer":{"email":"b@example.com"},"total":"n/a","created_at":"2026-11-18T10:00:00Z"}
```

```json output
{"status":"ok","fields":{"id":"A-1","customer":"C-100","total":25.5,"created_at":"2026-11-18T09:30:00Z"},"emptied":[]}
{"status":"missing_required","fields":["total"]}
```

The second record's `total` is a text that is not a number, so the field has
no value, and because it is required the record is `missing_required`.

## References

The reference files are copies of the project's `docs/CONFIGURATION.md`,
`docs/TROUBLESHOOTING.md` and `docs/SIDECAR.md`; where they mention those
paths, read the copy here.

- `references/configuration.md`: every configuration key, the formats, the ops
  and their arguments, with examples.
- `references/troubleshooting.md`: response statuses, `moldette check`
  messages and how to fix them.
- `references/sidecar.md`: running `moldette-sidecar`, several configurations,
  configurations read from a URL, limits, metrics and Kubernetes.
