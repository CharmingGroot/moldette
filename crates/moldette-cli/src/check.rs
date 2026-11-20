use std::path::Path;
use std::process::ExitCode;

use serde_json::json;

use crate::{lineage, load};

pub(crate) fn run(path: &Path) -> ExitCode {
    let specs = match load::specs(path) {
        Ok(specs) => specs,
        Err(code) => return code,
    };
    let output = json!({
        "digest": specs.digest(),
        "lineage": specs.lineage().map(lineage::to_json),
    });
    println!("{output:#}");
    ExitCode::SUCCESS
}
