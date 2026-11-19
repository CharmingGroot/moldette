use moldette_bench_pipeline::allocations_per_record;
use moldette_bench_support::CountingAllocator;

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

const EXPECTED: &str = "\
pipeline/regex/record/100B 5.00
pipeline/regex/record/10KiB 5.00
pipeline/regex/record/1MiB 5.00
pipeline/regex/batch/1MiB 5.00
pipeline/delimited/record/100B 2.94
pipeline/delimited/record/10KiB 3.25
pipeline/delimited/record/1MiB 3.00
pipeline/delimited/batch/1MiB 3.00
pipeline/json/record/100B 3.00
pipeline/json/record/10KiB 3.00
pipeline/json/record/1MiB 3.00
pipeline/json/batch/1MiB 3.00
pipeline/kv/record/100B 2.00
pipeline/kv/record/10KiB 2.00
pipeline/kv/record/1MiB 2.00
pipeline/kv/batch/1MiB 2.00";

#[test]
fn allocations_per_record_match_the_recorded_counts() {
    assert_eq!(allocations_per_record().join("\n"), EXPECTED);
}
