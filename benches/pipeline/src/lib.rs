//! Pipelines of the benchmark workloads, built from their configuration.

use moldette::Pipeline;
use moldette::config::{Config, Registry};
use moldette_bench_support::Workload;

pub fn pipeline(workload: Workload) -> Pipeline {
    let specs = Config::from_yaml(workload.config_yaml())
        .expect("valid configuration")
        .build(&Registry::new())
        .expect("specs are built");
    Pipeline::from_specs(&specs).expect("all three sections")
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
