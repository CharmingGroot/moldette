use std::hint::black_box;

use moldette_bench_config::config_texts;
use moldette_bench_support::{CountingAllocator, allocations};
use moldette_config::{Config, Registry};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn main() {
    println!("allocations per configuration");
    let registry = Registry::new();
    for text in config_texts() {
        let (config, load) = allocations(|| Config::from_yaml(&text.yaml).expect("valid YAML"));
        let (specs, build) = allocations(|| config.build(&registry).expect("specs are built"));
        let (report, check) = allocations(|| config.check());
        black_box((specs, report));
        println!(
            "config/{:<12} from_yaml {load:>9} build {build:>9} check {check:>9}",
            text.name
        );
    }
}
