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

A configuration file in YAML or JSON describes all three layers; every key is
described in `docs/CONFIGURATION.md`. It is checked once when it is built:
unknown keys, references between layers and rule errors are reported before
any record is processed. The built specs are immutable and can be shared
across threads. The configuration also yields lineage: which input field each
output field comes from.

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
- `moldette-sidecar`: an HTTP server that runs one configuration over records
  streamed to it, for an application in the same pod.
- `moldette-spec`: output formats of responses. The `json` feature writes a
  response as the command prints it, through `serde`.

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
JSON. `run` reads records from a file or standard input and prints one JSON
response per line; `explain` adds how each field was filled. Both process
records on every available core, or on `--threads <n>`, and keep the input
order. Only built-in formats and operations are available from the command
line.

Records are one per line by default. `--records json` reads one record per
JSON value, or per element of a top-level array, and `--records multiline
--record-start <regex>` joins each line that does not match the pattern to the
record before it. Input files ending in `.gz` are decompressed.

    moldette run --records json order.yaml orders.json
    moldette run --records multiline --record-start '^\d{4}-' app.yaml app.log.gz

The `framing` feature of the `moldette` crate provides the same record
splitting to programs, without I/O.

`moldette-sidecar` runs one configuration for an application in the same pod
or host. It listens on `127.0.0.1:8095` by default, takes the record options
above, and streams one response per line, in input order, as the request body
arrives:

    moldette-sidecar --records json order.yaml
    curl --data-binary @orders.json http://127.0.0.1:8095/v1/process

When the body cannot be read to the end, for example when a record is not
valid UTF-8, the responses before it are followed by a line such as
`{"error":{"message":"not valid UTF-8","record":2}}`. Request size, record
size, time per request and requests at once are limited, and `GET /healthz`
and `GET /readyz` answer probes.

Each request holds at most `--max-buffer-bytes` (16 MiB by default) of
records being processed and responses the client has not read, plus the
batches being processed at that moment. Once it holds that much, the sidecar
stops reading the body until the client reads more of the response. A client
that reads the response only after sending the whole body is therefore served
only while the responses fit in that limit; past it, the request ends at the
time limit. Such a request holds about the limit plus 7 MiB in the process and
up to about 7 MiB more in kernel socket buffers, so with the defaults 16 of
them hold about 450 MiB.

Probes that cannot reach the loopback address, such as those of the kubelet,
use `--health-listen <address>`, which answers only `GET /healthz` and
`GET /readyz`.

`examples/configs` holds configurations for an nginx access log, syslog, a
JSON API response and a CSV export, each with an input and the output of
`moldette run`. The `examples/otlp` program sends the spans of a pipeline to
an OpenTelemetry collector.

## Installation

    cargo add moldette

For the command and the sidecar:

    cargo install moldette-cli
    cargo install moldette-sidecar

The sidecar image `ghcr.io/charminggroot/moldette-sidecar` runs on
`linux/amd64` and `linux/arm64`. It holds only the static binary, runs as user
65532 and needs no writable file system. Images built from release tags carry
a build provenance attestation:

    gh attestation verify oci://ghcr.io/charminggroot/moldette-sidecar:0.6.0 -R CharmingGroot/moldette

`deploy/sidecar/kubernetes.yaml` runs the sidecar next to an application in a
pod with a restricted security context.

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
