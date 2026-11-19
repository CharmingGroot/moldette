use moldette_bench_resp::allocations_per_record;
use moldette_bench_support::CountingAllocator;

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

const EXPECTED: &str = "\
resp/regex/record/100B 0.00
resp/regex/record/10KiB 0.00
resp/regex/record/1MiB 0.00
resp/regex/batch/1MiB 0.00
resp/delimited/record/100B 0.00
resp/delimited/record/10KiB 0.00
resp/delimited/record/1MiB 0.00
resp/delimited/batch/1MiB 0.00
resp/json/record/100B 0.00
resp/json/record/10KiB 0.00
resp/json/record/1MiB 0.00
resp/json/batch/1MiB 0.00
resp/kv/record/100B 0.00
resp/kv/record/10KiB 0.00
resp/kv/record/1MiB 0.00
resp/kv/batch/1MiB 0.00";

#[test]
fn allocations_per_record_match_the_recorded_counts() {
    assert_eq!(allocations_per_record().join("\n"), EXPECTED);
}
