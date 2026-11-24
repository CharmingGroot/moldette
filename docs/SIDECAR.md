# moldette-sidecar

`moldette-sidecar` runs moldette configurations for an application in the same
pod or host. The application sends raw records over HTTP and reads one JSON
response per record, in input order, as the request body arrives. Processing is
the library's: the sidecar only reads requests, chooses the configuration and
writes responses.

## Running

    cargo install moldette-sidecar
    moldette-sidecar --records json order.yaml
    curl --data-binary @orders.json http://127.0.0.1:8095/v1/process

The sidecar listens on `127.0.0.1:8095` by default, so only processes in the
same pod or host reach it. A configuration file whose name ends in `.json` is
read as JSON and any other as YAML; it is checked as `moldette check` checks
it, and the sidecar does not start when it has errors. Only the built-in
formats and ops are available.

The container image `ghcr.io/charminggroot/moldette-sidecar` runs on
`linux/amd64` and `linux/arm64`. It holds only the static binary, runs as user
65532 and needs no writable file system.

## Endpoints

| Endpoint | Answer |
|---|---|
| `POST /v1/process` | with one configuration given without a name: the responses, one JSON line per record |
| `POST /v1/process/<name>` | the same with the configuration of that name |
| `GET /healthz` | 200 while the process runs |
| `POST /v1/check` | checks the configuration in the request body without running it |
| `GET /readyz` | 200 once the configurations are loaded and the address is open; with `--config-source`, once the list has been read |
| `GET /metrics` | only on the `--metrics-listen` address |

Each processing response carries the digest of its configuration in the
`moldette-config-digest` header.

When the body cannot be read to the end, the responses before the failing
record are followed by one line, such as
`{"error":{"message":"not valid UTF-8","record":2}}`, and the response ends. The
status code stays 200, because the response had already started. A body
declared larger than the limit is refused with 413 before it is read, and a
request over the limit of requests at once with 503 and `Retry-After`.

## Records

Records are one per line by default. `--records json` reads one record per JSON
value, or per element of a top-level array, and `--records multiline
--record-start <regex>` joins each line that does not match the pattern to the
record before it.

## Several configurations

Several configurations can run in one sidecar, each under a name and at its
own path. `--records` and `--record-start` without a name apply to every
configuration, and prefixed by a name to that configuration only:

    moldette-sidecar --records orders=json orders=order.yaml access=access.yaml
    curl --data-binary @orders.json http://127.0.0.1:8095/v1/process/orders

Each configuration is a separate object: changing or failing one does not touch
the others. They share the process, the threads and the limits below, so a
burst of requests for one of them slows the others; a configuration that must
not be affected runs in a sidecar of its own. A changed configuration file
takes effect when the sidecar restarts.

## Configurations from a URL

With `--config-source <url>`, the sidecar reads named configurations from an
`http` URL every `--config-poll` seconds, 5 by default, so that a configuration
changed in an application takes effect within seconds without a restart. The
URL answers with a list:

```text
{
  "configs": [
    {"name": "orders", "format": "yaml", "records": "json", "body": "schemas:\n  - name: order\n..."},
    {"name": "access", "format": "yaml", "body": "..."}
  ]
}
```

`name` is the name of the processing path, `format` is `yaml` or `json`, and
`body` is the text of the configuration file. `records` and `record_start`
take the values of the options of the same name, and records are one per line
without them. Configuration files may be given as well:

    moldette-sidecar --config-source http://config.app.svc/moldette access=access.yaml

At each read:

- A list sent with an `ETag` is asked for again with `If-None-Match`, and a 304
  answer changes nothing.
- An entry that changed is checked and built, and replaces the configuration
  of its name. A request in progress ends with the configuration it started
  with.
- An entry that fails to build leaves the configuration of its name as it was,
  and its messages are printed on standard error.
- A name no longer listed is removed, and its paths answer 404.
- A name given as a configuration file is not replaced or removed.
- When the URL cannot be read, the sidecar goes on with the configurations it
  has.

`/readyz` answers 503 until the list has been read once. Only `http` URLs are
read, so the URL should be inside the cluster or behind a service mesh that
encrypts traffic.

`POST /v1/check` checks a configuration as the sidecar would build it, without
running it, so an application can refuse a configuration before listing it.
The body is the configuration text, YAML or, with `?format=json`, JSON. The
answer is 200 with the digest and the warnings, or 422 with the errors, here
for a `delimited` parser given `separator` instead of `delimiter`:

```text
{"digest":null,"messages":[{"message":"parser: unknown key `separator` at line 3, column 3","severity":"error"}]}
```

## Options

| Option | Default | Meaning |
|---|---|---|
| `--listen <address>` | `127.0.0.1:8095` | the address of the processing endpoints |
| `--health-listen <address>` | none | a second address that answers only `GET /healthz` and `GET /readyz` |
| `--metrics-listen <address>` | none | an address that answers only `GET /metrics` |
| `--threads <n>` | available cores | threads that parse and map records |
| `--records [<name>=]<how>` | `lines` | `lines`, `multiline` or `json` |
| `--record-start [<name>=]<regex>` | none | for `multiline`, the pattern of a line that starts a record |
| `--max-body-bytes <n>` | 67108864 | largest request body |
| `--max-record-bytes <n>` | 1048576 | largest record |
| `--max-buffer-bytes <n>` | 16777216 | bytes one request holds; see below |
| `--timeout <seconds>` | 30 | time for one request, from reading its body to writing the last response |
| `--max-requests <n>` | 16 | requests processed at once; more are answered with 503 |
| `--config-source <url>` | none | an `http` URL that lists named configurations; see above |
| `--config-poll <seconds>` | 5 | interval between reads of the configuration source |
| `--config-source-max-bytes <n>` | 16777216 | largest list the configuration source may send |

An option and its value may also be joined with `=`, as in
`--health-listen=0.0.0.0:8096`. SIGTERM stops accepting connections and waits
for the requests in progress.

## Memory and slow clients

Each request holds at most `--max-buffer-bytes` of records being processed and
responses the client has not read, plus the batches being processed at that
moment. Once it holds that much, the sidecar stops reading the body until the
client reads more of the response.

A client that sends the whole body before reading the response is therefore
served only while the responses fit in that limit; past it, the request ends at
the time limit. Read the response while sending. A request that is not read
holds about the limit plus 7 MiB in the process and up to about 7 MiB more in
kernel socket buffers, so with the defaults 16 such requests hold about
450 MiB.

## Metrics

`--metrics-listen <address>` answers `GET /metrics` in the OpenMetrics text
format, which Prometheus reads. Without it, no metrics address is opened.

| Metric | Labels | Meaning |
|---|---|---|
| `moldette_requests_total` | `config`, `code` | requests to a processing path by status code (200, 413, 503, 405) |
| `moldette_records_total` | `config`, `status` | records by response status |
| `moldette_received_bytes_total` | `config` | request body bytes read |
| `moldette_request_duration_seconds` | `config` | time from the start of a request to its last response |
| `moldette_requests_in_progress` | | requests being processed |
| `moldette_config_source_requests_total` | `result` | reads of the configuration source: `ok`, `not_modified`, `error` |
| `moldette_config_builds_total` | `config`, `result` | builds of configurations read from the source: `ok`, `error` |

`config` is the configuration name, empty for one given without a name.
Requests for a name that is not configured are not counted, and the series of a
configuration removed from the source are removed.

## Kubernetes

`deploy/sidecar/kubernetes.yaml` runs the sidecar as a native sidecar container
(Kubernetes 1.29 or later) next to an application, with a restricted security
context: non-root, read-only root file system, no capabilities. Records go to
`127.0.0.1:8095`, which is not reachable from outside the pod; the probes use
`--health-listen` and Prometheus `--metrics-listen`. A changed ConfigMap takes
effect when the pod restarts.

Images built from release tags carry a build provenance attestation:

    gh attestation verify oci://ghcr.io/charminggroot/moldette-sidecar:0.8.0 -R CharmingGroot/moldette
