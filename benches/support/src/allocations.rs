use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

/// Counts allocations and reallocations of the current thread. A benchmark
/// binary declares it as its `#[global_allocator]`.
pub struct CountingAllocator;

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

/// Runs `run` and returns its result with the allocations it made on the
/// current thread. Counts zero unless `CountingAllocator` is the global
/// allocator.
pub fn allocations<T>(run: impl FnOnce() -> T) -> (T, usize) {
    let before = ALLOCATIONS.with(Cell::get);
    let result = run();
    (result, ALLOCATIONS.with(Cell::get) - before)
}

/// Runs `run` on each input and returns the mean allocations per input. The
/// first input runs once beforehand so that lazily built state, such as a
/// regex cache, is not counted.
pub fn per_input<T>(inputs: &[T], mut run: impl FnMut(&T)) -> f64 {
    if let Some(first) = inputs.first() {
        run(first);
    }
    let ((), count) = allocations(|| inputs.iter().for_each(&mut run));
    count as f64 / inputs.len() as f64
}
