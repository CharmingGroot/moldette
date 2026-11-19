use std::hint::black_box;
use std::iter;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use moldette_bench_parser::parser_spec;
use moldette_bench_support::{BATCHES, THREADS, Workload, cases, on_threads, thread_case};

fn parse(c: &mut Criterion) {
    let (label, _) = BATCHES[0];
    for workload in Workload::ALL {
        let spec = parser_spec(workload);
        let mut group = c.benchmark_group(format!("parser/{}", workload.format()));
        let cases = cases(workload, |i, size| workload.raw(i, size));
        for case in &cases {
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
        let case = thread_case(&cases);
        group.sample_size(10);
        for threads in THREADS {
            group.throughput(Throughput::ElementsAndBytes {
                elements: (case.inputs.len() * threads) as u64,
                bytes: case.raw_bytes * threads as u64,
            });
            group.bench_with_input(
                BenchmarkId::new(format!("threads/{label}_each"), threads),
                &threads,
                |b, &threads| {
                    b.iter(|| {
                        on_threads(iter::repeat_n(&case.inputs, threads), |records| {
                            for raw in records {
                                let _ = black_box(spec.parse(black_box(raw)));
                            }
                        })
                    })
                },
            );
        }
        group.finish();
    }
}

criterion_group!(benches, parse);
criterion_main!(benches);
