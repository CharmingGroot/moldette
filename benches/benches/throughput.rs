use std::hint::black_box;
use std::thread;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use moldette_benches::{Workload, workloads};

const THREADS: [usize; 4] = [1, 2, 4, 8];

fn parse_all(workload: &Workload) {
    for record in &workload.records {
        let _ = black_box(workload.parser.parse(black_box(record)));
    }
}

fn process_all(workload: &Workload) {
    for record in &workload.records {
        black_box(workload.pipeline.process(black_box(record)));
    }
}

fn throughput(c: &mut Criterion) {
    for workload in workloads() {
        let records = workload.records.len() as u64;
        let mut group = c.benchmark_group(workload.format);
        group.throughput(Throughput::Elements(records));
        group.bench_function("parse", |b| b.iter(|| parse_all(&workload)));
        group.bench_function("process", |b| b.iter(|| process_all(&workload)));
        for threads in THREADS {
            group.throughput(Throughput::Elements(records * threads as u64));
            group.bench_with_input(
                BenchmarkId::new("process_threads", threads),
                &threads,
                |b, &threads| {
                    b.iter(|| {
                        thread::scope(|scope| {
                            for _ in 0..threads {
                                scope.spawn(|| process_all(&workload));
                            }
                        })
                    })
                },
            );
        }
        group.finish();
    }
}

criterion_group!(benches, throughput);
criterion_main!(benches);
