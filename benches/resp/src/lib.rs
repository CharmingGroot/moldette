//! Resp specs of the benchmark workloads, built with the resp builder only.

use std::hint::black_box;

use moldette_bench_support::{BATCHES, Workload, allocations, cases};
use moldette_resp::RespSpec;

pub fn resp_spec(workload: Workload) -> RespSpec {
    RespSpec::builder(workload.schema()).build()
}

/// Mean allocations per record of each case, at most the first batch size,
/// as `<layer>/<format>/<case>` and a count rounded to two decimals. Counts
/// are zero unless the calling binary uses `CountingAllocator` (#64).
pub fn allocations_per_record() -> Vec<String> {
    let mut rows = Vec::new();
    for workload in Workload::ALL {
        let spec = resp_spec(workload);
        for case in cases(workload, &BATCHES[..1], |i, size| workload.mapped(i, size)) {
            black_box(spec.respond(case.inputs[0].clone()));
            let inputs = case.inputs.clone();
            let ((), count) = if case.name.starts_with("batch/") {
                allocations(|| {
                    black_box(spec.respond_batch(inputs));
                })
            } else {
                allocations(|| {
                    for mapped in inputs {
                        black_box(spec.respond(mapped));
                    }
                })
            };
            let count = count as f64 / case.inputs.len() as f64;
            rows.push(format!(
                "resp/{}/{} {count:.2}",
                workload.format(),
                case.name
            ));
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use moldette_bench_support::{Size, Workload, same_mapped};
    use moldette_core::Response;

    use super::resp_spec;

    #[test]
    fn responds_ok_to_every_workload_output() {
        for workload in Workload::ALL {
            let spec = resp_spec(workload);
            for size in Size::ALL {
                for i in size.indices() {
                    let expected = workload.mapped(i, size);
                    let Response::Ok(mapped) = spec.respond(expected.clone()) else {
                        panic!("{workload:?} {size:?} {i} is not ok");
                    };
                    assert!(same_mapped(&mapped, &expected));
                }
            }
        }
    }
}
