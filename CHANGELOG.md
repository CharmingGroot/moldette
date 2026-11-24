# Changelog

All crates of the workspace share one version. Each release lists its changes
per crate, with the issue or pull request that holds the details.

## Compatibility

moldette is at version 0.x.

- A minor release (0.5 to 0.6) may change the public Rust API, the
  configuration format, the output of the `moldette` command and the HTTP
  interface of `moldette-sidecar`. Each such change is listed under
  "Changed" or "Removed".
- A patch release (0.6.0 to 0.6.1) changes none of them. It fixes defects and
  updates dependencies.
- The minimum supported Rust version, 1.88, is raised only in a minor release
  (#78).

## Unreleased

## 0.8.0 (2026-11-24)

### Added

- `moldette-sidecar`: `--config-source <url>` reads named configurations from
  an `http` URL every `--config-poll` seconds (5 by default), with
  `If-None-Match`; a changed configuration is built and replaces the one of its
  name, one that fails to build keeps the previous one, one no longer listed
  is removed, and configurations given as files are not replaced. `/readyz`
  answers 503 until the list is first read, and `--config-source-max-bytes`
  limits the list (#135, #142, #143).
- `moldette-sidecar`: `POST /v1/check` checks a configuration without running
  it (#143).
- `moldette-sidecar`: the metrics `moldette_config_source_requests_total` and
  `moldette_config_builds_total`; the series of a removed configuration are
  removed (#143).
- A Claude Code plugin marketplace at the repository root, with a skill that
  writes, checks and deploys configurations (#139).

### Changed

- `moldette-mapper`: a `timestamp` format that reads no time of day reads the
  start of the day, so `[year]-[month]-[day]` gives `2026-11-18T00:00:00Z`
  for `2026-11-18` instead of no value (#136).

### Fixed

- `moldette-cli`, `moldette-sidecar`: an option and its value may be joined
  with `=`, as in `--threads=4`, which was refused before (#137).

The README was rewritten, and the sidecar and troubleshooting documents
moved to `docs/SIDECAR.md` and `docs/TROUBLESHOOTING.md` (#138). The other
crates have no changes besides the version.

## 0.7.0 (2026-11-24)

### Added

- `moldette-sidecar`: several configurations in one sidecar, given as
  `<name>=<config>` and run at `POST /v1/process/<name>`, with `--records`
  and `--record-start` for one configuration when prefixed by its name. The
  configurations share the threads and the limits, and a changed
  configuration takes effect on restart (#114, #122, #123).
- `moldette-sidecar`: the `moldette-config-digest` response header, the
  digest of the configuration that processed the request (#123).
- `moldette-sidecar`: `--metrics-listen <address>` serves `GET /metrics` in
  the OpenMetrics text format: requests by configuration and status code,
  records by response status, request body bytes, request durations and
  requests in progress (#124, #126).

The other crates have no changes besides the version.

## 0.6.0 (2026-11-23)

`moldette-spec` and `moldette-sidecar` are new on crates.io. `moldette-spec`
0.5.0 was published ahead of this release so that 0.6.0 could be published
through trusted publishing; its code is that of 0.6.0.

### Added

- `moldette`: the `framing` feature splits text into records without I/O:
  one per line, one per JSON value or element of a top-level array, or lines
  joined from a start pattern (#106).
- `moldette-cli`: `--records <lines|multiline|json>` and
  `--record-start <regex>`; input files ending in `.gz` are decompressed
  (#107).
- `moldette-spec` (new crate): the JSON form of responses, shared by the
  command and the sidecar, with no dependency between the pipeline crates and
  `serde` (#108).
- `moldette-sidecar` (new crate): an HTTP server that runs one configuration
  over records streamed to `POST /v1/process` and streams the responses in
  input order. Request size, record size, time per request, requests at once
  and memory per request (`--max-buffer-bytes`) are limited; `--health-listen`
  serves probes on a second address (#109, #110, #111, #117).
- Sidecar container image `ghcr.io/charminggroot/moldette-sidecar` for
  `linux/amd64` and `linux/arm64`, with build provenance attestations, and a
  Kubernetes example in `deploy/sidecar` (#111).
- `docs/CONFIGURATION.md`, every configuration key with examples that the
  tests run (#112).
- `examples/configs`: configurations, inputs and outputs for an access log,
  syslog, a JSON API response and a CSV export (#113).

### Changed

- `moldette-cli`: errors in the input refer to `record N` instead of
  `line N`; with one record per line the numbers are the same (#107).

## 0.5.0 (2026-11-21)

### Added

- `moldette-mapper`: `year` and `offset` for the `timestamp` op
  (`Rule::timestamp_with`), what `lookup` writes for a text not in its table
  (`Rule::lookup_with`), and a default value for a rule (`Rule::default`)
  (#97, #98, #99).
- `moldette-parser`: `none_values`, texts read as no value (#100).
- `moldette-config`: the keys of these features and
  `LineageField::default`. The digest of a configuration without them is the
  same as in 0.4.0.
- `moldette-cli`: `--threads <n>` for `run` and `explain`, by default the
  number of available cores, keeping the input order (#101).

### Changed

- `moldette-cli`: the keys of a response are written in the order `status`,
  `fields` (in schema order), `emptied` (#101).

## 0.4.0 (2026-11-20)

### Added

- `moldette-cli` (new crate): the `moldette` command with `check`, `run` and
  `explain` (#87, #88, #89).
- Fuzz targets for each parser format and for the pipeline, built on pull
  requests and run weekly (#90, #92).

The library crates were published through trusted publishing (#83, #94).

## 0.3.0 (2026-11-20)

First public release, and the first release on crates.io: `moldette`,
`moldette-config`, `moldette-parser`, `moldette-mapper`, `moldette-resp` and
`moldette-core` (#77 to #82).

## 0.2.0 (2026-11-19)

### Changed

- Fewer allocations per record in the parser and the mapper (#72, #73).
- Public API cleanup before the first public release: `MissingSectionError`,
  `Config::lineage`, and `#[non_exhaustive]` on `ExplainSource` and
  `Severity` (#75).

Benchmarks per crate, input size and thread count were added (#68, #71), with
a CI check of allocations per record (#74).

## 0.1.0 (2026-11-19)

The parser, mapper and resp layers, YAML and JSON configuration with checks
between sections and lineage, and the `moldette` pipeline (milestones M0 to
M5).
