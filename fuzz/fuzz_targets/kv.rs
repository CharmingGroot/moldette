#![no_main]

use std::sync::LazyLock;

use libfuzzer_sys::fuzz_target;
use moldette::parser::ParserSpec;
use moldette_fuzz::{KV, parser};

static PARSER: LazyLock<ParserSpec> = LazyLock::new(|| parser(KV));

fuzz_target!(|data: &[u8]| {
    if let Ok(raw) = std::str::from_utf8(data) {
        let _ = PARSER.parse(raw);
    }
});
