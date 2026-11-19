use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use moldette_bench_config::config_texts;
use moldette_config::{Config, Registry};

fn configure(c: &mut Criterion) {
    let registry = Registry::new();
    for text in config_texts() {
        let config = Config::from_yaml(&text.yaml).expect("valid YAML");
        let mut group = c.benchmark_group(format!("config/{}", text.name));
        group.throughput(Throughput::ElementsAndBytes {
            elements: 1,
            bytes: text.yaml.len() as u64,
        });
        group.sample_size(if text.yaml.len() > 64 << 10 { 10 } else { 50 });
        group.bench_function("from_yaml", |b| {
            b.iter(|| black_box(Config::from_yaml(black_box(&text.yaml))))
        });
        if let Some(json) = &text.json {
            group.throughput(Throughput::ElementsAndBytes {
                elements: 1,
                bytes: json.len() as u64,
            });
            group.bench_function("from_json", |b| {
                b.iter(|| black_box(Config::from_json(black_box(json))))
            });
        }
        group.bench_function("build", |b| b.iter(|| black_box(config.build(&registry))));
        group.bench_function("check", |b| b.iter(|| black_box(config.check())));
        group.finish();
    }
}

criterion_group!(benches, configure);
criterion_main!(benches);
