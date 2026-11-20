use std::fs;
use std::path::Path;
use std::process::ExitCode;

use moldette::config::{Config, Registry, Severity};
use serde_json::json;

use crate::lineage;

pub(crate) fn run(path: &Path) -> ExitCode {
    let name = path.display();
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("error: {name}: {err}");
            return ExitCode::FAILURE;
        }
    };
    let loaded = if path
        .extension()
        .is_some_and(|extension| extension == "json")
    {
        Config::from_json(&text)
    } else {
        Config::from_yaml(&text)
    };
    let config = match loaded {
        Ok(config) => config,
        Err(err) => {
            eprintln!("error: {name}: {err}");
            return ExitCode::FAILURE;
        }
    };
    let report = config.check();
    for diagnostic in report.diagnostics() {
        let severity = match diagnostic.severity() {
            Severity::Error => "error",
            _ => "warning",
        };
        eprintln!("{severity}: {name}: {diagnostic}");
    }
    if report.has_errors() {
        return ExitCode::FAILURE;
    }
    let specs = match config.build(&Registry::new()) {
        Ok(specs) => specs,
        Err(err) => {
            eprintln!("error: {name}: {err}");
            return ExitCode::FAILURE;
        }
    };
    let output = json!({
        "digest": specs.digest(),
        "lineage": specs.lineage().map(lineage::to_json),
    });
    println!("{output:#}");
    ExitCode::SUCCESS
}
