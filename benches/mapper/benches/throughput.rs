use std::hint::black_box;
use std::iter;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use moldette_bench_mapper::mapper_spec;
use moldette_bench_support::{
    BATCHES, THREADS, Workload, borrowed, cases, on_threads, thread_case,
};

fn map(c: &mut Criterion) {
    let (label, _) = BATCHES[0];
    for workload in Workload::ALL {
        let spec = mapper_spec(workload);
        let mut group = c.benchmark_group(format!("mapper/{}", workload.format()));
        let cases = cases(workload, |i, size| workload.fields(i, size));
        for case in &cases {
            let records: Vec<_> = case.inputs.iter().map(borrowed).collect();
            group.throughput(Throughput::Elements(case.inputs.len() as u64));
            group.sample_size(if case.raw_bytes > 1 << 20 { 10 } else { 50 });
            group.bench_function(&case.name, |b| {
                b.iter(|| {
                    for record in &records {
                        black_box(spec.map(black_box(record)));
                    }
                })
            });
        }
        let case = thread_case(&cases);
        let records: Vec<_> = case.inputs.iter().map(borrowed).collect();
        group.sample_size(10);
        for threads in THREADS {
            group.throughput(Throughput::Elements((records.len() * threads) as u64));
            group.bench_with_input(
                BenchmarkId::new(format!("threads/{label}_each"), threads),
                &threads,
                |b, &threads| {
                    b.iter(|| {
                        on_threads(iter::repeat_n(&records, threads), |records| {
                            for record in records {
                                black_box(spec.map(black_box(record)));
                            }
                        })
                    })
                },
            );
        }
        group.finish();
    }
}

criterion_group!(benches, map);
criterion_main!(benches);
