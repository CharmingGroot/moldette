use std::borrow::Cow;
use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use moldette_bench_core::{TEXT_SIZES, WIDTHS, field_names, schema};
use moldette_core::{FieldRecord, Mapped, Value};

fn field_record(names: &[String]) {
    let mut record = FieldRecord::with_capacity(names.len());
    for name in names {
        record.insert(name.as_str(), name.as_str());
    }
    for name in names {
        black_box(record.get(name));
    }
}

fn mapped(names: &[String]) {
    let mut mapped = Mapped::new();
    for name in names {
        mapped.insert(name.as_str(), Value::Str(Cow::Borrowed(name)));
    }
    for name in names {
        black_box(mapped.get(name));
    }
}

fn core(c: &mut Criterion) {
    let mut group = c.benchmark_group("core");
    for width in WIDTHS {
        let names = field_names(width);
        let schema = schema(width);
        group.throughput(Throughput::Elements(width as u64));
        group.bench_with_input(
            BenchmarkId::new("field_record/insert_get", width),
            &names,
            |b, names| b.iter(|| field_record(black_box(names))),
        );
        group.bench_with_input(
            BenchmarkId::new("mapped/insert_get", width),
            &names,
            |b, names| b.iter(|| mapped(black_box(names))),
        );
        group.bench_with_input(
            BenchmarkId::new("schema/field", width),
            &names,
            |b, names| {
                b.iter(|| {
                    for name in names {
                        black_box(schema.field(black_box(name)));
                    }
                })
            },
        );
    }
    for (label, size) in TEXT_SIZES {
        let text = "x".repeat(size);
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(
            BenchmarkId::new("value/into_owned", label),
            &text,
            |b, text| b.iter(|| black_box(Value::Str(Cow::Borrowed(black_box(text))).into_owned())),
        );
    }
    group.finish();
}

criterion_group!(benches, core);
criterion_main!(benches);
