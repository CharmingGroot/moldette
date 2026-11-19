use std::borrow::Cow;
use std::hint::black_box;

use moldette_bench_core::{WIDTHS, field_names};
use moldette_bench_support::{CountingAllocator, allocations};
use moldette_core::{FieldRecord, Mapped, Value};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn main() {
    println!("allocations per record");
    for width in WIDTHS {
        let names = field_names(width);
        let ((), record) = allocations(|| {
            let mut record = FieldRecord::new();
            for name in &names {
                record.insert(name.as_str(), name.as_str());
            }
            black_box(record);
        });
        let ((), mapped) = allocations(|| {
            let mut mapped = Mapped::new();
            for name in &names {
                mapped.insert(name.as_str(), Value::Str(Cow::Borrowed(name)));
            }
            black_box(mapped);
        });
        println!("core/field_record/insert/{width:<6} {record:>8}");
        println!("core/mapped/insert/{width:<10} {mapped:>8}");
    }
}
