//! Pipelines of the benchmark workloads, built from their configuration.

use std::hint::black_box;

use moldette::Pipeline;
use moldette::config::{Config, Registry};
use moldette_bench_support::{BATCHES, Workload, allocations, cases, per_input};

pub fn pipeline(workload: Workload) -> Pipeline {
    let specs = Config::from_yaml(workload.config_yaml())
        .expect("valid configuration")
        .build(&Registry::new())
        .expect("specs are built");
    Pipeline::from_specs(&specs).expect("all three sections")
}

/// Mean allocations per record of each case, at most the first batch size,
/// as `<layer>/<format>/<case>` and a count rounded to two decimals. Counts
/// are zero unless the calling binary uses `CountingAllocator` (#64).
pub fn allocations_per_record() -> Vec<String> {
    let mut rows = Vec::new();
    for workload in Workload::ALL {
        let pipeline = pipeline(workload);
        for case in cases(workload, &BATCHES[..1], |i, size| workload.raw(i, size)) {
            let count = if case.name.starts_with("batch/") {
                black_box(pipeline.process(&case.inputs[0]));
                let ((), count) = allocations(|| {
                    black_box(pipeline.process_batch(case.inputs.iter().map(String::as_str)));
                });
                count as f64 / case.inputs.len() as f64
            } else {
                per_input(&case.inputs, |raw| {
                    black_box(pipeline.process(raw));
                })
            };
            rows.push(format!(
                "pipeline/{}/{} {count:.2}",
                workload.format(),
                case.name
            ));
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use moldette::Response;
    use moldette_bench_support::{Size, Workload, same_mapped};

    use super::pipeline;

    #[test]
    fn processes_raw_records_into_the_workload_output() {
        for workload in Workload::ALL {
            let pipeline = pipeline(workload);
            for size in Size::ALL {
                for i in size.indices() {
                    let raw = workload.raw(i, size);
                    let Response::Ok(mapped) = pipeline.process(&raw) else {
                        panic!("{workload:?} {size:?} {i} is not ok");
                    };
                    assert!(
                        same_mapped(&mapped, &workload.mapped(i, size)),
                        "{workload:?} {size:?} {i}: {mapped:?}"
                    );
                }
            }
        }
    }
}
