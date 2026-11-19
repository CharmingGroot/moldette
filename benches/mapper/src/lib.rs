//! Mapper specs of the benchmark workloads, built with the rule builder only.

use std::hint::black_box;

use moldette_bench_support::{BATCHES, Workload, borrowed, cases, per_input};
use moldette_mapper::{MapperSpec, Rule};

const STATUS_CLASSES: [(&str, &str); 5] = [
    ("1", "informational"),
    ("2", "success"),
    ("3", "redirection"),
    ("4", "client_error"),
    ("5", "server_error"),
];

pub fn mapper_spec(workload: Workload) -> MapperSpec {
    let rules = match workload {
        Workload::AccessLog => vec![
            Rule::new("client", "forwarded_for")
                .fallback("remote_addr")
                .split(',', 0)
                .trim(),
            Rule::new("time", "time_local").timestamp([
                "[day]/[month repr:short]/[year]:[hour]:[minute]:[second] [offset_hour][offset_minute]",
            ]),
            Rule::new("path", "uri").split('?', 0),
            Rule::new("http_version", "protocol").extract(r"^HTTP/(\d(?:\.\d)?)$"),
            Rule::new("status_class", "status")
                .extract(r"^(\d)")
                .lookup(STATUS_CLASSES),
            Rule::new("user_agent", "agent"),
            Rule::constant("site", "web-01"),
        ],
        Workload::OrderExport => vec![
            Rule::new("placed_at", "placed_at")
                .timestamp(["[year]-[month]-[day] [hour]:[minute]:[second]"]),
            Rule::new("gift", "gift").lookup([("yes", "true"), ("no", "false")]),
        ],
        Workload::OrderApi => Vec::new(),
        Workload::OrderEvents => {
            vec![Rule::new("channel", "channel").lookup([("w", "web"), ("s", "store")])]
        }
    };
    rules
        .into_iter()
        .fold(MapperSpec::builder(workload.schema()), |builder, rule| {
            builder.rule(rule)
        })
        .build()
        .expect("valid mapper spec")
}

/// Mean allocations per record of each case, at most the first batch size,
/// as `<layer>/<format>/<case>` and a count rounded to two decimals. Counts
/// are zero unless the calling binary uses `CountingAllocator` (#64).
pub fn allocations_per_record() -> Vec<String> {
    let mut rows = Vec::new();
    for workload in Workload::ALL {
        let spec = mapper_spec(workload);
        for case in cases(workload, &BATCHES[..1], |i, size| workload.fields(i, size)) {
            let records: Vec<_> = case.inputs.iter().map(borrowed).collect();
            let count = per_input(&records, |record| {
                black_box(spec.map(record));
            });
            rows.push(format!(
                "mapper/{}/{} {count:.2}",
                workload.format(),
                case.name
            ));
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use moldette_bench_support::{Size, Workload, borrowed, same_mapped};

    use super::mapper_spec;

    #[test]
    fn maps_workload_fields_into_the_workload_output() {
        for workload in Workload::ALL {
            let spec = mapper_spec(workload);
            for size in Size::ALL {
                for i in size.indices() {
                    let fields = workload.fields(i, size);
                    let mapped = spec.map(&borrowed(&fields));
                    assert!(
                        same_mapped(&mapped, &workload.mapped(i, size)),
                        "{workload:?} {size:?} {i}: {mapped:?}"
                    );
                }
            }
        }
    }
}
