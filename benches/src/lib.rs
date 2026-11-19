//! Workloads shared by the throughput and allocation benchmarks (#64): one
//! pipeline per record format, built from a configuration file, and records
//! generated from a template so that every run reads the same input.
//!
//! `cargo bench -p moldette-benches --bench throughput` measures records per
//! second; `--bench allocations` prints allocations per record.

use moldette::Pipeline;
use moldette::config::{Config, Registry};
use moldette::parser::ParserSpec;

/// Records generated per workload.
pub const RECORDS: usize = 4096;

const ACCESS_LOG: &str =
    include_str!("../../crates/moldette-config/tests/fixtures/nginx_access.yaml");
const ORDER_EXPORT: &str =
    include_str!("../../crates/moldette-config/tests/fixtures/order_export.yaml");
const ORDER_API: &str = include_str!("../../crates/moldette-config/tests/fixtures/order_api.yaml");
const ORDER_EVENTS: &str = include_str!("../data/order_events.yaml");

/// A pipeline and the parser spec it runs, with the records to feed them.
pub struct Workload {
    pub format: &'static str,
    pub parser: ParserSpec,
    pub pipeline: Pipeline,
    pub records: Vec<String>,
}

/// Returns the regex, delimited, json and kv workloads.
pub fn workloads() -> Vec<Workload> {
    vec![
        workload("regex", ACCESS_LOG, access_log_line),
        workload("delimited", ORDER_EXPORT, order_export_row),
        workload("json", ORDER_API, order_api_body),
        workload("kv", ORDER_EVENTS, order_event_line),
    ]
}

fn workload(format: &'static str, yaml: &str, record: fn(usize) -> String) -> Workload {
    let specs = Config::from_yaml(yaml)
        .expect("valid configuration")
        .build(&Registry::new())
        .expect("specs are built");
    Workload {
        format,
        parser: specs.parser().expect("parser section").clone(),
        pipeline: Pipeline::from_specs(&specs).expect("all three sections"),
        records: (0..RECORDS).map(record).collect(),
    }
}

fn access_log_line(i: usize) -> String {
    let status = [200, 201, 304, 404, 500][i % 5];
    let (method, protocol) = [("GET", "HTTP/1.1"), ("POST", "HTTP/2.0")][i % 2];
    let forwarded = if i.is_multiple_of(3) {
        "-".to_owned()
    } else {
        format!("198.51.100.{}, 203.0.113.{}", i % 250, i % 200)
    };
    format!(
        "203.0.113.{} - - [18/Nov/2026:{:02}:{:02}:{:02} +0000] \"{method} /orders?id=A-{i} {protocol}\" {status} {} \"https://shop.example/cart\" \"Mozilla/5.0 (X11; Linux x86_64)\" \"{forwarded}\"",
        i % 254 + 1,
        i / 3600 % 24,
        i / 60 % 60,
        i % 60,
        i * 37 % 8192,
    )
}

fn order_export_row(i: usize) -> String {
    let product = ["Notebook", "\"Desk lamp, brass\"", "Pencil set"][i % 3];
    let note = ["", "\"Leave at the \"\"side\"\" door\"", "gift wrap"][i % 3];
    format!(
        "A-{i},2026-11-18 {:02}:{:02}:{:02},C-{},{product},{},{}.{:02},{},{note}",
        i / 3600 % 24,
        i / 60 % 60,
        i % 60,
        i % 500,
        i % 9 + 1,
        i % 90 + 1,
        i % 100,
        ["yes", "no"][i % 2],
    )
}

fn order_api_body(i: usize) -> String {
    let address = if i.is_multiple_of(4) {
        "null".to_owned()
    } else {
        format!("{{\"city\":\"City {}\"}}", i % 40)
    };
    format!(
        "{{\"id\":\"A-{i}\",\"status\":\"{}\",\"customer\":{{\"id\":\"C-{}\",\"address\":{address}}},\"items\":[{{\"sku\":\"S-{}\",\"quantity\":{}}},{{\"sku\":\"S-{}\",\"quantity\":1}}],\"total\":{}.{:02},\"paid\":{},\"created_at\":\"2026-11-18T{:02}:{:02}:{:02}Z\",\"coupon\":null,\"meta\":{{\"page\":{}}}}}",
        ["paid", "pending", "on hold"][i % 3],
        i % 500,
        i % 64,
        i % 5 + 1,
        i % 32,
        i % 200 + 1,
        i % 100,
        i.is_multiple_of(2),
        i / 3600 % 24,
        i / 60 % 60,
        i % 60,
        i % 7,
    )
}

fn order_event_line(i: usize) -> String {
    format!(
        "id=E-{i} ts=2026-11-18T{:02}:{:02}:{:02}+09:00 customer=C-{} amount={}.{:02} paid={} ch={}",
        i / 3600 % 24,
        i / 60 % 60,
        i % 60,
        i % 500,
        i % 300 + 1,
        i % 100,
        i.is_multiple_of(2),
        ["w", "s"][i % 2],
    )
}

#[cfg(test)]
mod tests {
    use moldette::Response;

    use super::{RECORDS, workloads};

    #[test]
    fn every_generated_record_responds_ok() {
        for workload in workloads() {
            assert_eq!(workload.records.len(), RECORDS);
            for record in &workload.records {
                let response = workload.pipeline.process(record);
                assert!(
                    matches!(response, Response::Ok(_)),
                    "{}: {record}: {response:?}",
                    workload.format
                );
            }
        }
    }
}
