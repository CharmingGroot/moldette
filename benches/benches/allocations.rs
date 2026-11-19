use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;

use moldette_benches::workloads;

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

fn per_record<'a>(records: &'a [String], mut run: impl FnMut(&'a str)) -> f64 {
    let before = ALLOCATIONS.with(Cell::get);
    for record in records {
        run(record);
    }
    (ALLOCATIONS.with(Cell::get) - before) as f64 / records.len() as f64
}

fn main() {
    println!("{:<10} {:>8} {:>8}", "format", "parse", "process");
    for workload in workloads() {
        let parse = per_record(&workload.records, |record| {
            let _ = black_box(workload.parser.parse(record));
        });
        let process = per_record(&workload.records, |record| {
            black_box(workload.pipeline.process(record));
        });
        println!("{:<10} {parse:>8.2} {process:>8.2}", workload.format);
    }
}
