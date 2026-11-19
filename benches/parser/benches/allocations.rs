use std::hint::black_box;

use moldette_bench_parser::parser_spec;
use moldette_bench_support::{CountingAllocator, Workload, cases, per_input};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn main() {
    println!("allocations per record");
    for workload in Workload::ALL {
        let spec = parser_spec(workload);
        for case in cases(workload, |i, size| workload.raw(i, size)) {
            let parse = per_input(&case.inputs, |raw| {
                let _ = black_box(spec.parse(raw));
            });
            println!(
                "parser/{}/{:<14} {parse:>8.2}",
                workload.format(),
                case.name
            );
        }
    }
}
