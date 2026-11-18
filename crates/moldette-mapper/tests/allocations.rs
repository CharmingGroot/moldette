use std::alloc::{GlobalAlloc, Layout, System};
use std::borrow::Cow;
use std::cell::Cell;

use moldette_core::{Field, FieldRecord, Mapped, OutputSchema, Type};
use moldette_mapper::{MapperSpec, Op, Rule};

struct CountingAllocator;

thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

fn count_allocation() {
    let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count_allocation();
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count_allocation();
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn allocations<T>(run: impl FnOnce() -> T) -> (T, usize) {
    let before = ALLOCATIONS.with(Cell::get);
    let result = run();
    (result, ALLOCATIONS.with(Cell::get) - before)
}

#[derive(Debug)]
struct StripPrefix(char);

impl Op for StripPrefix {
    fn apply<'a>(&'a self, text: &'a str) -> Option<Cow<'a, str>> {
        Some(Cow::Borrowed(text.strip_prefix(self.0).unwrap_or(text)))
    }

    fn name(&self) -> &str {
        "strip_prefix"
    }
}

fn borrowing_spec() -> MapperSpec {
    let schema = OutputSchema::builder("order", 1)
        .field(Field::new("order_id", Type::String))
        .field(Field::new("quantity", Type::Int))
        .field(Field::new("amount", Type::Float))
        .field(Field::new("paid", Type::Bool))
        .field(Field::new("client", Type::Ip))
        .field(Field::new("channel", Type::String))
        .field(Field::new("currency", Type::String))
        .build()
        .expect("valid schema");
    MapperSpec::builder(schema)
        .rule(
            Rule::new("order_id", "id")
                .empty_as_none()
                .fallback("order_no"),
        )
        .rule(
            Rule::coalesce("quantity", ["qty", "count"])
                .trim()
                .split(' ', 0),
        )
        .rule(Rule::new("amount", "total").via(StripPrefix('$')))
        .rule(Rule::new("channel", "ch").lookup([("w", "WEB"), ("s", "STORE")]))
        .rule(Rule::constant("currency", "USD"))
        .build()
        .expect("valid rules")
}

fn record<'a>(fields: &[(&'a str, &'a str)]) -> FieldRecord<'a> {
    let mut record = FieldRecord::new();
    for &(name, value) in fields {
        record.insert(name, value);
    }
    record
}

#[test]
fn map_allocates_only_for_its_output_when_ops_borrow() {
    let spec = borrowing_spec();
    let records = [
        record(&[
            ("id", "A-1"),
            ("qty", " 3 pcs"),
            ("total", "$12.5"),
            ("paid", "true"),
            ("client", "10.0.0.1"),
            ("ch", "s"),
        ]),
        record(&[
            ("id", ""),
            ("order_no", "B-2"),
            ("count", "x"),
            ("total", "12"),
            ("paid", "yes"),
            ("client", "::1"),
            ("ch", "x"),
        ]),
        record(&[("order_no", "C-3")]),
        FieldRecord::new(),
    ];
    for record in &records {
        let (mapped, mapping) = allocations(|| spec.map(record));
        let (_, rebuilding) = allocations(|| {
            let mut rebuilt = Mapped::new();
            for (name, value) in mapped.iter() {
                rebuilt.insert(name, value.clone());
            }
            for name in mapped.emptied() {
                rebuilt.mark_emptied(name);
            }
            rebuilt
        });
        assert_eq!(mapping, rebuilding, "{record:?}");
    }
}
