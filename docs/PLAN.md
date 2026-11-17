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

moldette is a Rust library. A sidecar and language bindings follow the core
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
```

## Built-in components

| Layer | Components |
|---|---|
| parser | `json`, `delimited`, `regex`, `kv`, header + body composition |
| mapper | rule keys `target`, `from`, `via`, `fallback`, `required` (#14); ops `trim`, `split`/`at`, `extract`, `lookup`, `const`, `coalesce`, `timestamp`; implicit coercion to string, int, float, bool, timestamp, ip |
| resp | statuses `Ok`, `Malformed`, `MissingRequired`; emptied-field list (#5); `explain` (#6); strict mode |

Custom parsers and ops are registered as Rust types. Configuration has no
expression language (#4).

## Cross-cutting rules

| Topic | Rule | Decision |
|---|---|---|
| errors | rule errors at spec build (`SpecError`); data errors as values per record | #3 |
| concurrency | specs and plans immutable, `Send + Sync`; processing takes `&self`; no threads spawned | #8 |
| observability | library crates emit `tracing` signals only; exporters belong to the caller | #7 |
| portability | no I/O, no global state; every crate builds for `wasm32-unknown-unknown` | #2 |
| CI | each affected crate checked on its own | #9 |
| publishing | crates stay unpublished until the first public release | #10 |

## Milestones

Each layer milestone starts with its standalone API and ends with tests that
use core types only.

### M0 core

- `Value` representation, empty vs absent, field lookup, timestamp type (#13)
- `FieldRecord`, `OutputSchema`, `Mapped`, `Response`, `SpecError`
- `Send + Sync` assertions on public types
- `tracing` feature, `moldette.*` names
- repository baseline: `SECURITY.md`, Dependabot

### M1 parser

1. standalone API, `ParserSpec` builder
2. `json`
3. `delimited`
4. `regex`
5. `kv`
6. header + body composition
7. custom parser registration
8. spans and events
9. standalone tests

### M2 mapper

1. standalone API, rule builder
2. rule execution: `from`, `fallback`, `required`
3. implicit coercion
4. `via` chains
5. built-in ops
6. custom op registration (#15)
7. spans and events
8. standalone and multi-threaded tests

### M3 resp

1. standalone API
2. statuses
3. emptied-field report, strict mode
4. `explain`, spec version
5. batch API
6. per-status counts and durations
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

## After the core

| Item | Content |
|---|---|
| CLI | `check`, `run`, `explain` |
| Benchmarks | records per second and allocations per record, per parser; throughput at 1, 2, 4 and 8 threads |
| Parallel batch | optional `rayon` feature |
| Ports | sidecar (HTTP / gRPC, OTLP export), JVM (UniFFI or WASM on Chicory), Python (PyO3), sandboxed WASM user ops |
| Supply chain | OpenSSF Scorecard and Best Practices badge at the first public release; artifact attestations at the first binary release |

## Workflow

| Item | Rule |
|---|---|
| branches | `main` plus one short-lived branch per change |
| merge | pull request, `ci-ok` passing, squash merge by the maintainer |
| commit messages | Conventional Commits, imperative mood, signed off (`-s`) |
| pull request body | becomes the squash commit message: background, then changes |
| pull request scope | one milestone item per pull request |
| releases | `vX.Y.Z` tags on `main`; `0.x` until the core contracts are stable |

## Contributing

Pull requests are accepted only for issues assigned by the maintainer (#12).

## License

Apache-2.0 (#11).
