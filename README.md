# moldette

English | [한국어](README.ko.md)

moldette turns raw records into records of the schema you declare. You
describe, in one YAML or JSON file, how to read a record (JSON, CSV and other
delimited text, log lines read with a regular expression, `key=value` pairs),
how to fill each output field and which fields are required. moldette checks
that file once, then processes each record on its own and tells you, for every
record, whether it fits the schema and why not.

It handles one record at a time. Fetching data, storage, deduplication,
lookups against external data and anything spanning more than one record
(aggregation, joins) are left to the caller.

## Quick start

Install the `moldette` command, from the binaries attached to each
[GitHub Release](https://github.com/CharmingGroot/moldette/releases) or with
Cargo:

```bash
cargo install moldette-cli
```

Write a configuration, `order.yaml`. It declares the output schema, reads four
fields from each JSON record, and fills the schema fields of the same name:

```yaml
schemas:
  - name: order
    version: 1
    fields:
      - {name: id, type: string, required: true}
      - {name: status, type: string}
      - {name: total, type: float, required: true}
      - {name: created_at, type: timestamp, required: true}

parser:
  format: json
  fields:
    id: /id
    status: /status
    total: /total
    created_at: /created_at

mapper:
  schema: {name: order, version: 1}

resp:
  schema: {name: order, version: 1}
```

Check it, then run it over records, one per line:

```bash
moldette check order.yaml
moldette run order.yaml orders.ndjson
```

```text input
{"id":"A-1","status":"paid","total":25.5,"created_at":"2026-11-18T09:30:00Z"}
{"id":"A-2","total":"n/a","created_at":"2026-11-18T10:00:00+09:00"}
not json
```

```json output
{"status":"ok","fields":{"id":"A-1","status":"paid","total":25.5,"created_at":"2026-11-18T09:30:00Z"},"emptied":[]}
{"status":"missing_required","fields":["total"]}
{"status":"malformed","reason":"expected ident at line 1 column 2"}
```

The second record's `total` is not a number, so the required field has no
value; the third is not JSON. `moldette explain order.yaml orders.ndjson`
shows, for each field, what was read and how it was converted.

## How it works

A record passes through three layers, all described by the configuration:

- **parser** reads the raw record into named text fields. A field that holds
  text in another format, such as a JSON body after a delimited header, can be
  parsed in turn.
- **mapper** fills each field of the output schema from those fields, with
  fallbacks, defaults, constants and a small set of operations (trim, split,
  extract, lookup, timestamp parsing), then converts the text to string, int,
  float, bool, timestamp or IP address.
- **resp** checks the result against the schema and gives the record one
  status: `ok`, `malformed`, `missing_required`, or `emptied` in strict mode.

The configuration is checked when it is loaded: unknown keys, references
between layers and rule errors are reported with their location before any
record is processed. It has no expression language; custom parsers and
operations are registered as Rust types. The configuration also yields
lineage, which input field each output field comes from.

## Three ways to use it

**The `moldette` command** checks a configuration and runs it over files or
standard input, on every core while keeping the input order. `--records json`
reads JSON values or the elements of a top-level array, `--records multiline`
joins lines into records, and `.gz` files are decompressed.

**`moldette-sidecar`** runs configurations next to an application, in the same
pod or host. The application posts records over HTTP and reads the responses as
they are produced. One sidecar can run several named configurations, read
them from an HTTP URL as they change, serves probes and Prometheus metrics,
and comes as a container image with a Kubernetes example. See
`docs/SIDECAR.md`.

**The `moldette` crate** gives the same pipeline to Rust programs. With `yaml`
holding a configuration and `raw` one record:

```rust
use moldette::Pipeline;
use moldette::config::{Config, Registry};

let specs = Config::from_yaml(yaml)?.build(&Registry::new())?;
let pipeline = Pipeline::from_specs(&specs)?;
let response = pipeline.process(raw);
```

The built pipeline is immutable and can be shared across threads. Each layer
is also usable on its own: a program can parse records without mapping them,
or check required fields of records produced elsewhere.

## Installation

| What | How |
|---|---|
| the library | `cargo add moldette` |
| the command | `cargo install moldette-cli`, or a binary from a GitHub Release (Linux x86_64 and aarch64, macOS x86_64 and aarch64, Windows x86_64) |
| the sidecar | `cargo install moldette-sidecar`, or the image `ghcr.io/charminggroot/moldette-sidecar` (`linux/amd64`, `linux/arm64`) |
| a Claude Code skill | `/plugin marketplace add CharmingGroot/moldette`, then `/plugin install moldette@moldette` |

Release binaries come with a `SHA256SUMS` file, and binaries and images carry
build provenance attestations:

    gh attestation verify moldette-cli-0.8.0-x86_64-unknown-linux-musl.tar.gz -R CharmingGroot/moldette

The default `config` feature of the `moldette` crate turns on every layer. To
use one layer without configuration files, depend on its crate, such as
`moldette-parser`, or turn off the default features and select the layer
feature.

## Documentation

- `docs/CONFIGURATION.md`: every configuration key, format and operation, with
  examples that the tests run.
- `docs/TROUBLESHOOTING.md`: response statuses, finding why a field is empty,
  and the messages of `moldette check`.
- `docs/SIDECAR.md`: running the sidecar, several configurations,
  configurations read from a URL, limits, metrics and Kubernetes.
- `examples/configs`: configurations for an nginx access log, syslog, a JSON
  API response and a CSV export, each with an input and its output.
- `examples/otlp`: sending the spans of a pipeline to an OpenTelemetry
  collector.
- `plugins/moldette`: a Claude Code skill that helps write, check and deploy
  configurations, built on the documents above.
- `CHANGELOG.md`: changes per release, and the compatibility rule.

## Crates

| Crate | Content |
|---|---|
| `moldette` | the pipeline, with each layer selected by a feature of the same name; record framing |
| `moldette-config` | configuration files, checks between sections, lineage |
| `moldette-parser`, `moldette-mapper`, `moldette-resp` | one layer each |
| `moldette-core` | the types the layers exchange |
| `moldette-cli` | the `moldette` command |
| `moldette-sidecar` | the HTTP sidecar |
| `moldette-spec` | output formats of responses, through `serde` |

Each layer crate depends only on `moldette-core`. No library crate performs
I/O or keeps global state, and each builds for `wasm32-unknown-unknown`. With
the `tracing` feature, the layers emit `tracing` spans and events; exporting
them is left to the caller.

## Status

moldette is at version 0.x and requires Rust 1.88 or later. A minor release
may change the public API, the configuration format and the output of the
command and the sidecar; a patch release does not. Changes are listed in
`CHANGELOG.md`, and the scope, layers and milestones are described in
`docs/PLAN.md`.

## Contributing

Issues are welcome. Pull requests are accepted only for issues assigned by the
maintainer; see `CONTRIBUTING.md`. Report vulnerabilities as described in
`SECURITY.md`. Participation is governed by `CODE_OF_CONDUCT.md`.

## License

Apache-2.0. See `LICENSE`.
