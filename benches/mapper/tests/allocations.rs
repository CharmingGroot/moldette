use moldette_bench_mapper::allocations_per_record;
use moldette_bench_support::CountingAllocator;

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

const EXPECTED: &str = "\
mapper/regex/record/100B 3.00
mapper/regex/record/10KiB 3.00
mapper/regex/record/1MiB 3.00
mapper/regex/batch/1MiB 3.00
mapper/delimited/record/100B 1.00
mapper/delimited/record/10KiB 1.00
mapper/delimited/record/1MiB 1.00
mapper/delimited/batch/1MiB 1.00
mapper/json/record/100B 1.00
mapper/json/record/10KiB 1.00
mapper/json/record/1MiB 1.00
mapper/json/batch/1MiB 1.00
mapper/kv/record/100B 1.00
mapper/kv/record/10KiB 1.00
mapper/kv/record/1MiB 1.00
mapper/kv/batch/1MiB 1.00";

#[test]
fn allocations_per_record_match_the_recorded_counts() {
    assert_eq!(allocations_per_record().join("\n"), EXPECTED);
}
