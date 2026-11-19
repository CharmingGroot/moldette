use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use moldette_bench_parser::parser_spec;
use moldette_bench_support::{Workload, cases};

fn parse(c: &mut Criterion) {
    for workload in Workload::ALL {
        let spec = parser_spec(workload);
        let mut group = c.benchmark_group(format!("parser/{}", workload.format()));
        for case in cases(workload, |i, size| workload.raw(i, size)) {
            group.throughput(Throughput::ElementsAndBytes {
                elements: case.inputs.len() as u64,
                bytes: case.raw_bytes,
            });
            group.sample_size(if case.raw_bytes > 1 << 20 { 10 } else { 50 });
            group.bench_function(&case.name, |b| {
                b.iter(|| {
                    for raw in &case.inputs {
                        let _ = black_box(spec.parse(black_box(raw)));
                    }
                })
            });
        }
        group.finish();
    }
}

criterion_group!(benches, parse);
criterion_main!(benches);
