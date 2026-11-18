//! Exports the `tracing` signals of a moldette pipeline to an OpenTelemetry
//! collector over OTLP/HTTP with protobuf payloads. The library emits the
//! signals; the caller owns the exporter (#7).
//!
//! The collector address comes from `OTEL_EXPORTER_OTLP_ENDPOINT` and
//! defaults to `http://localhost:4318`. To print the received spans, run a
//! local collector with its debug exporter, then the example:
//!
//! ```text
//! docker run --rm -p 4318:4318 otel/opentelemetry-collector \
//!     --config 'yaml:receivers: {otlp: {protocols: {http: {endpoint: 0.0.0.0:4318}}}}' \
//!     --config 'yaml:exporters: {debug: {verbosity: detailed}}' \
//!     --config 'yaml:service: {pipelines: {traces: {receivers: [otlp], exporters: [debug]}}}'
//! cargo run -p moldette-otlp-example
//! ```
//!
//! Without a collector the export fails and the example still exits normally.

use std::error::Error;

use moldette::Pipeline;
use moldette::config::{Config, Registry};
use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::SpanExporter;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_subscriber::filter::{LevelFilter, Targets};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

const SERVICE_NAME: &str = "moldette-otlp-example";
const ACCESS_LOG: &str =
    include_str!("../../../crates/moldette-config/tests/fixtures/nginx_access.yaml");
const LINES: [&str; 3] = [
    r#"203.0.113.7 - - [18/Nov/2026:10:15:32 +0000] "GET /orders?id=A-1 HTTP/1.1" 200 512 "-" "curl/8.5.0" "198.51.100.2, 203.0.113.7""#,
    r#"203.0.113.9 - - [18/Nov/2026:10:15:33 +0900] "POST /orders HTTP/2.0" 404 0 "https://shop.example/cart" "Mozilla/5.0" "-""#,
    "GET /orders 200",
];

fn main() -> Result<(), Box<dyn Error>> {
    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(SpanExporter::builder().with_http().build()?)
        .with_resource(Resource::builder().with_service_name(SERVICE_NAME).build())
        .build();
    let subscriber = tracing_subscriber::registry()
        .with(tracing_opentelemetry::layer().with_tracer(provider.tracer(SERVICE_NAME)))
        .with(Targets::new().with_target("moldette", LevelFilter::TRACE));

    let guard = subscriber.set_default();
    let result = run();
    drop(guard);

    if let Err(err) = provider.shutdown() {
        eprintln!("tracer provider shutdown failed: {err}");
    }
    result
}

fn run() -> Result<(), Box<dyn Error>> {
    let specs = Config::from_yaml(ACCESS_LOG)?.build(&Registry::new())?;
    let pipeline = Pipeline::from_specs(&specs).ok_or("the configuration lacks a layer")?;
    let (_, counts) = pipeline.process_batch(LINES);
    println!(
        "ok {}, malformed {}, missing required {}, emptied {}",
        counts.ok(),
        counts.malformed(),
        counts.missing_required(),
        counts.emptied()
    );
    Ok(())
}
