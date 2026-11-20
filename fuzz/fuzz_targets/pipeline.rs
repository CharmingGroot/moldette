#![no_main]

use std::sync::LazyLock;

use libfuzzer_sys::fuzz_target;
use moldette::Pipeline;
use moldette_fuzz::pipelines;

static PIPELINES: LazyLock<Vec<Pipeline>> = LazyLock::new(pipelines);

fuzz_target!(|data: &[u8]| {
    if let Ok(raw) = std::str::from_utf8(data) {
        for pipeline in PIPELINES.iter() {
            let _ = pipeline.process(raw);
            let _ = pipeline.explain(raw);
        }
    }
});
