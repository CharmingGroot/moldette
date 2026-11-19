//! Resp specs of the benchmark workloads, built with the resp builder only.

use moldette_bench_support::Workload;
use moldette_resp::RespSpec;

pub fn resp_spec(workload: Workload) -> RespSpec {
    RespSpec::builder(workload.schema()).build()
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
