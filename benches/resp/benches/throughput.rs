use std::hint::black_box;

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use moldette_bench_resp::resp_spec;
use moldette_bench_support::{BATCHES, THREADS, Workload, cases, on_threads, thread_case};

fn respond(c: &mut Criterion) {
    let (label, _) = BATCHES[0];
    for workload in Workload::ALL {
        let spec = resp_spec(workload);
        let mut group = c.benchmark_group(format!("resp/{}", workload.format()));
        let cases = cases(workload, |i, size| workload.mapped(i, size));
        for case in &cases {
            group.throughput(Throughput::Elements(case.inputs.len() as u64));
            group.sample_size(if case.raw_bytes > 1 << 20 { 10 } else { 50 });
            let batch = case.name.starts_with("batch/");
            group.bench_function(&case.name, |b| {
                b.iter_batched(
                    || case.inputs.clone(),
                    |inputs| {
                        if batch {
                            black_box(spec.respond_batch(inputs));
                        } else {
                            for mapped in inputs {
                                black_box(spec.respond(mapped));
                            }
                        }
                    },
                    BatchSize::PerIteration,
                )
            });
        }
        let case = thread_case(&cases);
        group.sample_size(10);
        for threads in THREADS {
            group.throughput(Throughput::Elements((case.inputs.len() * threads) as u64));
            group.bench_with_input(
                BenchmarkId::new(format!("threads/{label}_each"), threads),
                &threads,
                |b, &threads| {
                    b.iter_batched(
                        || vec![case.inputs.clone(); threads],
                        |inputs| {
                            on_threads(inputs, |inputs| {
                                for mapped in inputs {
                                    black_box(spec.respond(mapped));
                                }
                            })
                        },
                        BatchSize::PerIteration,
                    )
                },
            );
        }
        group.finish();
    }
}

criterion_group!(benches, respond);
criterion_main!(benches);
