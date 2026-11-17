# moldette plan

moldette turns one raw record into one record of a declared output schema.
Parsers, field mappings and operations are declared as data rather than code.
Each layer validates its specification before processing any record.

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

The caller unwraps any transport envelope (for example, splits a JSON array of
documents) and hands moldette one record at a time, or a batch.

## Layers

```
                     configuration
         specs (built once) |  |  |
           +----------------+  |  +-----------------+
           v                   v                    v
raw --> parser --FieldRecord--> mapper --Mapped--> resp --> Response
```

| Layer | Spec (validated when built) | Input | Output | Not its job |
|---|---|---|---|---|
| parser | `ParserSpec` | one raw record (`&str`, `&[u8]`, JSON value) | `FieldRecord` | type coercion, meaning of values |
| mapper | `OutputSchema` + rules | `FieldRecord` | `Mapped` (values, emptied fields, optional explanation) | multi-record work, external lookups |
| resp | `OutputSchema` | `Mapped` | `Response` | quarantine, drop or retry decisions |
| configuration | - | configuration model | layer specs, lineage, validation report | processing records, where configuration is stored |

configuration is not on the data path. It translates a declarative
configuration into the specs of the other layers and runs the checks that only
make sense when layers are combined.

### Each layer works on its own

Every layer can be used without the others, as long as its input and output
contracts are respected.

| Use | configuration | parser | mapper | resp |
|---|---|---|---|---|
| full pipeline from one configuration file | yes | yes | yes | yes |
| normalize raw input into `FieldRecord` for another tool | | yes | | |
| map already parsed data (database rows, API objects) onto a schema | | | yes | |
| map a hand-built `FieldRecord` and check required fields | | | yes | yes |
| check required fields of records produced elsewhere | | | | yes |
| validate rules and compute lineage in an editor, without running | yes | spec only | spec only | spec only |

Using the mapper alone (API not final):

```rust
let schema = OutputSchema::builder()
    .field("order_id", Type::String).required()
    .field("status", Type::String)
    .build()?;

let mapper = Mapper::builder(&schema)
    .rule(Rule::to("order_id").from("id"))
    .rule(Rule::to("status").from("state").via(op::lookup([("P", "paid")])))
    .build()?;                      // rule errors surface here only

let mut rec = FieldRecord::new();   // filled from any source
rec.insert("id", "A-1");
rec.insert("state", "P");
let mapped = mapper.map(&rec);      // data errors come back as values
```

### Contracts

- Layers take and return core types only. No layer accepts another layer's
  types.
- Rule errors are reported only when a spec is built (`SpecError`). Data
  errors are returned per record as values, never as panics.
- When an op receives input of an unexpected shape it returns empty, and the
  field is listed in `Mapped` as emptied.
- No layer performs I/O or keeps global state. The library spawns no threads.
- Core types follow semver. Incompatible changes happen only in a major
  release.

## Crates

```
moldette-core      contracts: Value, FieldRecord, OutputSchema, Mapped, Response, SpecError
   ^      ^      ^
parser  mapper  resp          each depends on core only
   ^      ^      ^
moldette-config     configuration -> specs, cross-layer checks, lineage
   ^
moldette            pipeline facade, layers selected by feature
```

```
moldette/
├── Cargo.toml                 workspace
├── crates/
│   ├── moldette-core/         value.rs record.rs schema.rs mapped.rs response.rs error.rs
│   ├── moldette-parser/       spec.rs json.rs delimited.rs regex.rs kv.rs compose.rs registry.rs
│   ├── moldette-mapper/       rule.rs coerce.rs registry.rs op/
│   ├── moldette-resp/         status.rs explain.rs batch.rs
│   ├── moldette-config/       model.rs lower.rs check.rs plan.rs lineage.rs
│   └── moldette/              facade (features: parser, mapper, resp, config)
├── tests/fixtures/
│   ├── json-api/              order API responses
│   ├── csv/                   delimited files
│   └── nginx-access/          web server access logs
├── docs/PLAN.md
└── .github/workflows/ci.yml
```

## Layer details

### parser

Built in:

| Parser | Behavior |
|---|---|
| `json` | parse text or take a value; extract fields by path |
| `delimited` | split by a delimiter; extract fields by position; quoting |
| `regex` | extract named capture groups |
| `kv` | split `k=v` pairs; keys become field names |
| header + body | strip a header format, hand the body to another parser |

Parsers never coerce types. A raw record that does not match the expected
format yields `Malformed`. A field missing from an otherwise valid record is
empty, not an error.

### mapper

One rule per output field:

| Key | Meaning |
|---|---|
| `target` | output schema field |
| `from` | `FieldRecord` field; omitted when the value comes from `const` |
| `via` | ordered list of ops |
| `fallback` | one alternative rule tried when the result is empty |
| `required` | raise a field to required for this mapping only; cannot lower one the schema requires |

Coercion to the schema type (string, int, float, bool, timestamp, ip) is
implicit and happens after `via`. Ops cover only what parsers and coercion do
not:

| Op | Behavior |
|---|---|
| `trim` | remove surrounding whitespace |
| `split` / `at` | split into a list, take the n-th element |
| `extract` | one regex capture group |
| `lookup` | map values through a table declared in the rule |
| `const` | fixed value |
| `coalesce` | first non-empty value among several fields |
| `timestamp` | parse with an ordered list of formats |

An op is a pure function from one value to one value. Whether an empty value
is acceptable is decided by the schema and `required`, not by the op.

### resp

| Status | Meaning |
|---|---|
| `Ok` | output record |
| `Malformed` | raw record is not in the expected format |
| `MissingRequired` | named required fields are empty |

A response can also carry the spec version, the fields emptied by ops, and a
per-field explanation (`explain`) listing the input field, the ops applied and
the value after each step. The name `explain` avoids confusion with
OpenTelemetry traces. Strict mode treats emptied fields as failures, for use
in tests.

### configuration

```yaml
kind: OutputSchema
name: order
version: 1
fields:
  - { name: order_id,   type: string,    required: true }
  - { name: ordered_at, type: timestamp, required: true }
  - { name: amount,     type: float }
  - { name: status,     type: string }
```

```yaml
kind: Pipeline
name: shop-a
version: 1
parser: json
output: order@1
mapping:
  - { target: order_id,   from: $.id, required: true }
  - { target: ordered_at, from: $.created, via: [ { op: timestamp, formats: ["%Y%m%d%H%M%S"] } ] }
  - { target: amount,     from: $.total }
  - { target: status,     from: $.state, via: [ { op: lookup, table: { P: paid, C: cancelled } } ] }
```

Key names are not final.

Cross-layer checks:

- every `from` names a field the parser produces
- every `target` exists in the output schema
- every required schema field has a rule

A rule that targets a field marked deprecated in the schema is not rejected.
It is dropped and listed in the report, so a schema change does not stop a
running pipeline.

The compiled plan is immutable and carries the configuration version.
Lineage ("output field <- raw path") is computed from the plan without
running it.

## Extension

Custom parsers and ops are Rust types registered before specs are built.
Configuration refers to them by name; arguments are checked against the
type's declared argument schema. Custom parsers and ops also declare the
fields they read and produce, so lineage stays complete.

```rust
pub trait Op: Send + Sync {
    fn name(&self) -> &'static str;
    fn args(&self) -> ArgSchema;
    fn compile(&self, args: &Args) -> Result<Box<dyn CompiledOp>, SpecError>;
}

pub trait CompiledOp: Send + Sync {
    fn apply(&self, value: Value) -> Value;
}
```

`Parser` follows the same pattern.

There is no expression or scripting language in configuration. One would turn
permission to edit rules into permission to run code, let a single slow rule
stall everything on the same worker, and move rule errors from build time to
run time. Sandboxed user ops (WASM) are on the roadmap as an opt-in with time
and memory limits.

## Milestones

Contracts are fixed first. Each layer milestone starts with its standalone API
and ends with tests that use core types only.

### M0 core

- `Value` representation: owned enum or views into the raw record
- `FieldRecord`: empty vs absent
- `OutputSchema`: types, required, deprecated
- `Mapped`, `Response`, `SpecError`
- semver policy for core types
- observability contract: `tracing` feature, `moldette.*` names, label rules
- concurrency contract: `Send + Sync` assertions on public types
- repository baseline:
  - `LICENSE`, `SECURITY.md`
  - protected `main`, changes through pull requests
  - CI: test, clippy, `wasm32-unknown-unknown` build
  - actions pinned to commit SHAs, minimal `permissions:`
  - Dependabot, `cargo-deny` (licenses, advisories, duplicates)

### M1 parser

1. standalone API, `ParserSpec` builder
2. `json`
3. `delimited`
4. `regex`, linear-time engine only (`regex` crate)
5. `kv`
6. header + body composition
7. custom parser registration with declared output fields
8. spans and events behind the `tracing` feature
9. standalone tests with core types only

### M2 mapper

1. standalone API, rule builder
2. rule execution: `from`, `fallback`, `required`
3. implicit coercion to schema types
4. `via` chains, empty on unexpected input
5. built-in ops
6. custom op registration with declared input and output fields; purity
   requirement; decision on `catch_unwind` for custom ops
7. spans and events behind the `tracing` feature
8. standalone tests with core types only, including a multi-threaded test

### M3 resp

1. standalone API
2. statuses: `Ok`, `Malformed`, `MissingRequired`
3. emptied-field report, strict mode
4. per-field `explain`, spec version
5. batch API with per-record status
6. per-status counts and durations behind the `tracing` feature
7. standalone tests with core types only

### M4 configuration

1. configuration model, YAML / JSON, unknown keys rejected
2. lowering into layer specs
3. cross-layer checks
4. immutable versioned plan, deprecated-field report
5. lineage from the plan

### M5 moldette

1. pipeline API, layers selected by feature
2. end-to-end tests over the three fixture formats
3. example and test wiring `tracing` to OpenTelemetry (OTLP)

## Concurrency

The library performs no scheduling. Callers parallelize with the tool of their
choice (thread pools, rayon, async runtimes), and the library guarantees that
doing so is safe.

| Guarantee | How |
|---|---|
| specs and plans are shared without locks | immutable after build, `Send + Sync`, shared through `Arc` |
| processing calls run concurrently | processing methods take `&self` and mutate nothing |
| no hidden contention | no global state, no internal locks in built-in parsers and ops |
| results do not depend on scheduling | each record is processed independently; batch output keeps input order |
| configuration can change while records are in flight | callers swap the `Arc` to a new plan; in-flight records finish on the plan they started with |

Points of attention:

| Area | Risk | Handling |
|---|---|---|
| custom ops and parsers | interior mutability causes contention or nondeterministic output | traits require `Send + Sync`; documentation requires `apply` to be a pure function |
| regex | `regex::Regex` is shared across threads but keeps an internal scratch-space pool; contention at high thread counts is possible (unverified) | measured in the thread-scaling benchmark; per-thread clones if needed |
| async runtimes | CPU-bound calls block runtime worker threads | the API stays synchronous; documentation recommends `spawn_blocking` for large batches |
| panics in custom ops | a panic aborts the batch on that thread | built-in ops are tested not to panic; whether to wrap custom ops in `catch_unwind` is decided in M2 |

A parallel batch API behind a `rayon` feature is an option after the core. It
does not change the rule that the library spawns no threads by default.

Verification:

- compile-time assertions that public spec, plan and layer types implement
  `Send + Sync` (`static_assertions`)
- a multi-threaded test that runs one spec from many threads and compares the
  output with a single-threaded run
- a thread-scaling benchmark (1, 2, 4, 8 threads)

## Observability

moldette follows OpenTelemetry's split between API and SDK: libraries emit
signals, applications decide where they go.

| Where | Does | Uses |
|---|---|---|
| core and layer crates | emit spans and events; near zero cost when no subscriber is installed | the `tracing` crate, behind a feature |
| calling application | bridges to OpenTelemetry and exports over OTLP | `tracing-opentelemetry`, `opentelemetry-otlp` |
| sidecar (port) | owns the process, so it embeds the SDK | traces, metrics and logs over OTLP; W3C `traceparent` propagated from incoming HTTP / gRPC |

The library never installs a subscriber or exporter, so the no-I/O and
no-global-state contracts hold. `tracing` builds for `wasm32-unknown-unknown`.

| Signal | Content |
|---|---|
| metrics | records by status (`ok`, `malformed`, `missing_required`), emptied fields per field, parser and mapper duration histograms, spec build failures |
| spans | per batch and per spec build; per-record detail through sampling or `explain` on request |

Per-record spans are not emitted by default because of their cost at high
throughput.

Attributes use the `moldette.` namespace (`moldette.pipeline`,
`moldette.status`). Labels carry only values bounded by the schema, such as
field names, never record values, to keep metric cardinality bounded.

## After the core

| Item | Content |
|---|---|
| CLI | `check` (validate configuration), `run` (records from stdin), `explain` (per-field explanation for one record) |
| Benchmarks | records per second and allocations per record, per parser; throughput at 1, 2, 4 and 8 threads |
| Parallel batch | optional `rayon` feature for a parallel batch API |
| Ports | sidecar (HTTP / gRPC, with built-in OpenTelemetry SDK and OTLP export), JVM (UniFFI, or the WASM build on Chicory), Python (PyO3), sandboxed WASM user ops |

## Supply chain

The repository baseline in M0 satisfies most checks of the later steps. Each
step is adopted at the stage where it applies.

| When | Adopt |
|---|---|
| M0 | repository baseline (see M0) |
| first public release | OpenSSF Scorecard action; OpenSSF Best Practices badge at Passing; crates.io Trusted Publishing from GitHub Actions instead of a stored token |
| first binary release (CLI, sidecar image) | artifact attestations (Sigstore signing, SLSA build provenance) |
| more than one maintainer | Best Practices Silver / Gold, two-person review |

## Related tools

| Tool | Overlap | Difference |
|---|---|---|
| Vector (VRL) | parsing and remapping | VRL is a scripting language; moldette rules are declarative and checked before running |
| Logstash grok | parsing | grok is one stage inside a pipeline runtime; moldette is a library with no runtime |
| jq, JSONata, JSLT | JSON transformation | general expression languages over JSON; moldette targets a declared schema from any input format |
| RML engines | declarative mapping per input format | output is RDF; moldette outputs schema-typed records |

To be checked against each project's documentation before the first public
release.

## Workflow

| Item | Rule |
|---|---|
| branches | `main` plus one short-lived branch per change (`feat/m1-json-parser`, `docs/plan`) |
| merge | pull request, CI passing, squash merge |
| `main` protection | pull request required, status checks required, no force push, linear history; approving review not required |
| commit messages | Conventional Commits (`feat:`, `fix:`, `docs:`, `test:`, `ci:`, `chore:`), imperative mood, signed off (`-s`); the pull request title becomes the squash commit message |
| pull request scope | one milestone item per pull request |
| releases | `vX.Y.Z` tags on `main`; `0.x` until the core contracts are stable |

## Contributing

The project is maintained by one person. Contributions follow these rules:

1. Bug reports and proposals start as issues.
2. Pull requests are accepted only for issues the maintainer has assigned.
   Pull requests without an assigned issue are closed.
3. Commits are signed off under the Developer Certificate of Origin (`-s`).
4. One issue per pull request, with tests. No force pushes after review
   starts.
5. Generated pull requests without an assigned issue are closed without
   review.

`CONTRIBUTING.md` and issue and pull request templates are added before the
repository goes public.

## License

Apache-2.0.

## Open questions

- configuration key names and how schemas are referenced
- `Value` representation (M0)
