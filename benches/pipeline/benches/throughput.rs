use std::hint::black_box;
use std::thread;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use moldette::Pipeline;
use moldette_bench_pipeline::pipeline;
use moldette_bench_support::{BATCHES, Workload, batch, cases};

const THREADS: [usize; 4] = [1, 2, 4, 8];

fn process_all(pipeline: &Pipeline, records: &[String]) {
    for raw in records {
        black_box(pipeline.process(black_box(raw)));
    }
}

fn process(c: &mut Criterion) {
    for workload in Workload::ALL {
        let pipeline = pipeline(workload);
        let mut group = c.benchmark_group(format!("pipeline/{}", workload.format()));
        for case in cases(workload, |i, size| workload.raw(i, size)) {
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
        let (label, bytes) = BATCHES[0];
        let records = batch(workload, bytes);
        let raw_bytes: u64 = records.iter().map(|raw| raw.len() as u64).sum();
        group.sample_size(10);
        for threads in THREADS {
            group.throughput(Throughput::ElementsAndBytes {
                elements: (records.len() * threads) as u64,
                bytes: raw_bytes * threads as u64,
            });
            group.bench_with_input(
                BenchmarkId::new(format!("threads/{label}_each"), threads),
                &threads,
                |b, &threads| {
                    b.iter(|| {
                        thread::scope(|scope| {
                            for _ in 0..threads {
                                scope.spawn(|| process_all(&pipeline, &records));
                            }
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
