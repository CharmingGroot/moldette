# moldette plan

moldette turns one raw record into one record of a declared output schema.
This document is an overview. Design decisions and open questions are tracked
as issues labeled `type:decision`; filter by `layer:*` or `area:*`.

## Scope

In scope:

- parsing a raw record into named fields
- mapping those fields onto a declared output schema
- returning the result, or the reason it failed, as a value

Out of scope:

- fetching data (connections, credentials, paging, rate limits, offsets)
- storage, deduplication, quarantine
- enrichment that needs an external lookup
- anything spanning more than one record (aggregation, joins)

moldette is a Rust library. A command-line tool runs a configuration over
files or standard input (#84). A sidecar and language bindings follow the core
(#2).

## Layers

```
                     configuration
         specs (built once) |  |  |
           +----------------+  |  +-----------------+
           v                   v                    v
raw --> parser --FieldRecord--> mapper --Mapped--> resp --> Response
```

| Layer | Spec | Input | Output |
|---|---|---|---|
| parser | `ParserSpec` | one raw record | `FieldRecord` |
| mapper | `OutputSchema` and rules | `FieldRecord` | `Mapped` |
| resp | `OutputSchema` | `Mapped` | `Response` |
| configuration | - | configuration model | layer specs, lineage, validation report |

Each layer is usable on its own and exchanges only `moldette-core` types (#3).

| Use | configuration | parser | mapper | resp |
|---|---|---|---|---|
| full pipeline from one configuration file | yes | yes | yes | yes |
| normalize raw input into `FieldRecord` | | yes | | |
| map already parsed data onto a schema | | | yes | |
| check required fields of records produced elsewhere | | | | yes |
| validate rules and compute lineage without running | yes | spec only | spec only | spec only |

## Crates

```
moldette-core      Value, FieldRecord, OutputSchema, Mapped, Response, SpecError
   ^      ^      ^
parser  mapper  resp          each depends on core only
   ^      ^      ^
moldette-config     configuration -> specs, cross-layer checks, lineage
   ^
moldette            pipeline facade, layers selected by feature
   ^
moldette-cli        `moldette` binary: check, run, explain
```

## Built-in components

| Layer | Components |
|---|---|
| parser | `json`, `delimited`, `regex`, `kv`, header + body composition; `empty_as_none`, `none_values` (#95) |
| mapper | rule keys `target`, `from`, `coalesce`, `constant`, `via`, `fallback` (#14), `default` (#95); ops `trim`, `split`, `extract`, `lookup` (`default`, `keep_unmatched`), `timestamp` (`year`, `offset`), `empty_as_none`; implicit coercion to string, int, float, bool, timestamp, ip; `explain` (#6, #39) |
| resp | statuses `Ok`, `Malformed`, `MissingRequired`, `Emptied` (strict mode, #5); strict mode |

Custom parsers and ops are registered as Rust types. Configuration has no
expression language (#4).

## Cross-cutting rules

| Topic | Rule | Decision |
|---|---|---|
| errors | rule errors at spec build (`SpecError`); data errors as values per record | #3 |
| concurrency | specs and plans immutable, `Send + Sync`; processing takes `&self`; no threads spawned | #8 |
| observability | library crates emit `tracing` signals only; exporters belong to the caller | #7 |
| portability | no I/O, no global state; every library crate builds for `wasm32-unknown-unknown`; `moldette-cli` is host only | #2, #84 |
| CI | each affected crate checked on its own | #9 |
| toolchain | library crates declare `rust-version`, checked in CI | #78 |
| fuzzing | parser formats and pipeline `process` fuzzed with `cargo-fuzz`; no panic on any input | #84 |
| publishing | crates published from 0.3.0 on, in dependency order | #10 |

## Milestones

Each layer milestone starts with its standalone API and ends with tests that
use core types only. M0 to M5 make up v0.1 (tag `v0.1.0`); the v0.2
improvements are tagged `v0.2.0`, the first public release `v0.3.0`, the
CLI release `v0.4.0` and the configuration and parallel CLI release `v0.5.0`.

### M0 core

- `Value` representation, empty vs absent, field lookup, timestamp type (#13)
- `FieldRecord`, `OutputSchema`, `Mapped`, `Response`, `SpecError`
- `Send + Sync` assertions on public types
- repository baseline: `SECURITY.md`, Dependabot

### M1 parser

1. standalone API, `ParserSpec` builder
2. `json`
3. `delimited`
4. `regex`
5. `kv`
6. header + body composition
7. custom parser registration
8. `tracing` feature, `moldette.*` names, spans and events
9. standalone tests

### M2 mapper

1. standalone API, rule builder
2. rule execution: `from`, `fallback`
3. implicit coercion
4. `via` chains
5. built-in ops
6. custom op registration (#15)
7. spans and events
8. standalone and multi-threaded tests
9. `explain` (#39)

### M3 resp

1. standalone API
2. statuses
3. emptied-field report, strict mode
4. spec version (`RespSpec::schema()`)
5. batch API
6. per-status counts; durations from `tracing` spans (#7)
7. standalone tests

### M4 configuration

1. configuration model, YAML and JSON, unknown keys rejected (#14)
2. lowering into layer specs
3. cross-layer checks
4. immutable versioned plan, deprecated-field report
5. lineage from the plan

### M5 moldette

1. pipeline API, layers selected by feature
2. end-to-end tests over JSON API responses, CSV and nginx access logs
3. `tracing` to OpenTelemetry (OTLP) example

### v0.2 improvements

1. benchmarks per dependency branch (core, parser, mapper, resp, configuration, pipeline); records of 100 B, 10 KiB and 1 MiB, batches of 1 MiB and 16 MiB; records per second and MB/s (#64)
2. fewer allocations per record in parse and process
3. throughput scaling at 2, 4 and 8 threads
4. allocations per record checked in CI (#64)
5. public API review before the first public release (#66)

### v0.3 first public release

1. README in plain text, without images or badges
2. crate docs and metadata (`keywords`, `categories`, `readme`, `rust-version`); `publish = false` removed from the library crates
3. `CONTRIBUTING.md`, issue and pull request templates, code of conduct (#12)
4. OpenSSF Scorecard workflow, which runs once the repository is public
5. in order, each confirmed by the maintainer first (#10): public repository; repository settings that need a public repository (ruleset for `main`, private vulnerability reporting, secret scanning); crates.io release 0.3.0 in dependency order

### v0.4 CLI and fuzzing

1. `moldette-cli` crate with `check`: validation report and lineage, non-zero exit on errors (#84)
2. `run`: one record per line from a file or standard input, one JSON `Response` per line, status counts at the end
3. `explain`: `run` with the explain record of each record
4. fuzz targets for `json`, `delimited`, `regex`, `kv`, header + body composition and pipeline `process`, seeded from test fixtures
5. fuzzing in CI: weekly and manual runs per target; fuzz targets built on pull requests
6. release 0.4.0, confirmed by the maintainer first: library crates through trusted publishing; `moldette-cli` published first by the maintainer, then added to trusted publishing (#10)
7. OpenSSF Scorecard follow-ups: `SECURITY.md` with the reporting channel, response time and supported versions; CodeQL analysis on pull requests and `main`

### v0.5 configuration and parallel CLI

1. `timestamp` op: `year` and `offset` arguments for formats without them (#95)
2. `lookup` op: `default` and `keep_unmatched` arguments for values outside the table
3. rule key `default`: a constant when no source text is read, recorded in lineage and explain
4. parser key `none_values`: texts read as absent
5. CLI `--threads`, defaulting to the available cores: parsing and mapping in parallel batches, output in input order, batch size set by measurement
6. regex parser: cost per record measured, improvement decided from the result
7. the public log samples of #95 normalized again and compared with 0.4.0; release 0.5.0, confirmed by the maintainer first

## After v0.5

| Item | Content |
|---|---|
| Parallel batch | library API, decided from the CLI measurements (#95) |
| Ports | sidecar (HTTP / gRPC, OTLP export), JVM (UniFFI or WASM on Chicory), Python (PyO3), sandboxed WASM user ops |
| Supply chain | artifact attestations at the first binary release |

## Workflow

| Item | Rule |
|---|---|
| branches | `main` plus one short-lived branch per change |
| merge | pull request, `ci-ok` passing, squash merge by the maintainer |
| commit messages | Conventional Commits, imperative mood, signed off (`-s`) |
| pull request body | becomes the squash commit message: background, then changes |
| pull request scope | one milestone item per pull request |
| releases | `vX.Y.Z` tags on `main`; `0.x` until the core contracts are stable; a tag publishes the library crates through trusted publishing after the maintainer approves the `crates-io` environment (#10) |

## Contributing

Pull requests are accepted only for issues assigned by the maintainer (#12).
The rules are in `CONTRIBUTING.md`.

## License

Apache-2.0 (#11).
