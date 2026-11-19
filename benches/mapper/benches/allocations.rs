use std::hint::black_box;

use moldette_bench_mapper::mapper_spec;
use moldette_bench_support::{CountingAllocator, Workload, borrowed, cases, per_input};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn main() {
    println!("allocations per record");
    for workload in Workload::ALL {
        let spec = mapper_spec(workload);
        for case in cases(workload, |i, size| workload.fields(i, size)) {
            let records: Vec<_> = case.inputs.iter().map(borrowed).collect();
            let map = per_input(&records, |record| {
                black_box(spec.map(record));
            });
            println!("mapper/{}/{:<14} {map:>8.2}", workload.format(), case.name);
        }
    }
}
