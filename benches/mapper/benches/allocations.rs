use moldette_bench_mapper::allocations_per_record;
use moldette_bench_support::CountingAllocator;

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn main() {
    println!("allocations per record");
    for row in allocations_per_record() {
        println!("{row}");
    }
}
