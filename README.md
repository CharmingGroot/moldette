# moldette

moldette is a Rust library that turns one raw record into one record of a
declared output schema. It parses the record into named fields, maps those
fields onto the schema, and returns the result, or the reason it failed, as a
value.

It handles one record at a time. Fetching data, storage, deduplication,
enrichment that needs an external lookup, and anything spanning more than one
record (aggregation, joins) are left to the caller.

## How it works

A record passes through three layers:

- parser: reads one raw record (JSON, delimited text such as CSV, a regular
  expression, or key=value pairs) into named text fields. A field whose value
  is itself in one of these formats can be parsed in turn, such as a JSON body
  after a delimited header.
- mapper: fills each field of the output schema from those fields, with
  fallbacks, constants, a small set of operations (trim, split, extract,
  lookup, timestamp parsing) and coercion to string, int, float, bool,
  timestamp or IP address. For any record it can explain which source and
  which steps produced each output field.
- resp: checks the mapped record against the schema. Each record ends with
  one status: `Ok`, `Malformed` (the parser could not read it),
  `MissingRequired` or `Emptied` (strict mode).

A configuration file in YAML or JSON describes all three layers. It is checked
once when it is built: unknown keys, references between layers and rule errors
are reported before any record is processed. The built specs are immutable and
can be shared across threads. The configuration also yields lineage: which
input field each output field comes from.

Custom parsers and operations are registered as Rust types; the configuration
has no expression language.

Each layer is usable on its own. A program can parse records without mapping
them, map data that was parsed elsewhere, or check required fields of records
produced by another system.

## Crates

- `moldette`: the pipeline, with each layer selected by a feature of the same
  name. The `config` feature is on by default and turns on the other three.
- `moldette-config`: configuration files, cross-layer checks, lineage.
- `moldette-parser`, `moldette-mapper`, `moldette-resp`: one layer each.
- `moldette-core`: the types the layers exchange.
- `moldette-cli`: the `moldette` command, which checks a configuration file
  and runs it over records.

Of the moldette crates, each layer crate depends only on `moldette-core`. No
library crate performs I/O or keeps global state, and each builds for
`wasm32-unknown-unknown`. With the
`tracing` feature, the layers emit `tracing` spans and events; exporting them
is left to the caller.

## Usage

With `yaml` holding a configuration and `raw` one record, inside a function
that returns a `Result`:

```rust
use moldette::Pipeline;
use moldette::config::{Config, Registry};

let specs = Config::from_yaml(yaml)?.build(&Registry::new())?;
let pipeline = Pipeline::from_specs(&specs)?;
let response = pipeline.process(raw);
```

The same configuration can be checked and run from the command line:

    moldette check order.yaml
    moldette run order.yaml orders.ndjson
    moldette explain order.yaml orders.ndjson

`check` reports what the configuration gets wrong and prints its lineage as
JSON. `run` reads one record per line from a file or standard input and prints
one JSON response per line; `explain` adds how each field was filled. Only
built-in formats and operations are available from the command line.

Configuration examples for a JSON API response, a CSV export and an nginx
access log are in `crates/moldette-config/tests/fixtures`. The
`examples/otlp` program sends the spans of a pipeline to an OpenTelemetry
collector.

## Installation

    cargo add moldette

For the command:

    cargo install moldette-cli

The default `config` feature turns on every layer. To use one layer without
configuration files, depend on its crate, such as `moldette-parser`, or turn
off the default features and select the layer feature.

## Status

moldette is at version 0.x and requires Rust 1.88 or later. The public API may
change between minor versions until the core contracts are stable. The scope,
layers and milestones are described in `docs/PLAN.md`.

## Contributing

Issues are welcome. Pull requests are accepted only for issues assigned by the
maintainer; see `CONTRIBUTING.md`. Report vulnerabilities as described in
`SECURITY.md`. Participation is governed by `CODE_OF_CONDUCT.md`.

## License

Apache-2.0. See `LICENSE`.
