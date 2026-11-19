use moldette_bench_parser::allocations_per_record;
use moldette_bench_support::CountingAllocator;

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

const EXPECTED: &str = "\
parser/regex/record/100B 2.00
parser/regex/record/10KiB 2.00
parser/regex/record/1MiB 2.00
parser/regex/batch/1MiB 2.00
parser/delimited/record/100B 1.62
parser/delimited/record/10KiB 1.94
parser/delimited/record/1MiB 1.75
parser/delimited/batch/1MiB 1.67
parser/json/record/100B 2.00
parser/json/record/10KiB 2.00
parser/json/record/1MiB 2.00
parser/json/batch/1MiB 2.00
parser/kv/record/100B 1.00
parser/kv/record/10KiB 1.00
parser/kv/record/1MiB 1.00
parser/kv/batch/1MiB 1.00";

#[test]
fn allocations_per_record_match_the_recorded_counts() {
    assert_eq!(allocations_per_record().join("\n"), EXPECTED);
}
