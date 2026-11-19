use std::hint::black_box;

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use moldette_bench_resp::resp_spec;
use moldette_bench_support::{Workload, cases};

fn respond(c: &mut Criterion) {
    for workload in Workload::ALL {
        let spec = resp_spec(workload);
        let mut group = c.benchmark_group(format!("resp/{}", workload.format()));
        for case in cases(workload, |i, size| workload.mapped(i, size)) {
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
        group.finish();
    }
}

criterion_group!(benches, respond);
criterion_main!(benches);
