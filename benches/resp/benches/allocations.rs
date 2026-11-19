use std::hint::black_box;

use moldette_bench_resp::resp_spec;
use moldette_bench_support::{CountingAllocator, Workload, allocations, cases};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn main() {
    println!("allocations per record");
    for workload in Workload::ALL {
        let spec = resp_spec(workload);
        for case in cases(workload, |i, size| workload.mapped(i, size)) {
            let records = case.inputs.len() as f64;
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
            println!(
                "resp/{}/{:<14} {:>8.2}",
                workload.format(),
                case.name,
                count as f64 / records
            );
        }
    }
}
