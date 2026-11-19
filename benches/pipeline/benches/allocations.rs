use std::hint::black_box;

use moldette_bench_pipeline::pipeline;
use moldette_bench_support::{CountingAllocator, Workload, allocations, cases, per_input};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn main() {
    println!("allocations per record");
    for workload in Workload::ALL {
        let pipeline = pipeline(workload);
        for case in cases(workload, |i, size| workload.raw(i, size)) {
            let process = if case.name.starts_with("batch/") {
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
            println!(
                "pipeline/{}/{:<14} {process:>8.2}",
                workload.format(),
                case.name
            );
        }
    }
}
