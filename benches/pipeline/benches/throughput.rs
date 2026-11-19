use std::hint::black_box;
use std::iter;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use moldette::Pipeline;
use moldette_bench_pipeline::pipeline;
use moldette_bench_support::{BATCHES, THREADS, Workload, cases, on_threads, thread_case};

fn process_all(pipeline: &Pipeline, records: &[String]) {
    for raw in records {
        black_box(pipeline.process(black_box(raw)));
    }
}

fn process(c: &mut Criterion) {
    let (label, _) = BATCHES[0];
    for workload in Workload::ALL {
        let pipeline = pipeline(workload);
        let mut group = c.benchmark_group(format!("pipeline/{}", workload.format()));
        let cases = cases(workload, |i, size| workload.raw(i, size));
        for case in &cases {
            group.throughput(Throughput::ElementsAndBytes {
                elements: case.inputs.len() as u64,
                bytes: case.raw_bytes,
            });
            group.sample_size(if case.raw_bytes > 1 << 20 { 10 } else { 50 });
            if case.name.starts_with("batch/") {
                group.bench_function(&case.name, |b| {
                    b.iter(|| {
                        black_box(pipeline.process_batch(case.inputs.iter().map(String::as_str)))
                    })
                });
            } else {
                group.bench_function(&case.name, |b| {
                    b.iter(|| process_all(&pipeline, &case.inputs))
                });
            }
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
                            process_all(&pipeline, records)
                        })
                    })
                },
            );
        }
        group.finish();
    }
}

criterion_group!(benches, process);
criterion_main!(benches);
