use std::fs;
use std::path::Path;
use std::process::ExitCode;

use moldette::config::{Config, Registry, Severity, Specs};

pub(crate) fn specs(path: &Path) -> Result<Specs, ExitCode> {
    let name = path.display();
    let fail = |message: &dyn std::fmt::Display| {
        eprintln!("error: {name}: {message}");
        ExitCode::FAILURE
    };
    let text = fs::read_to_string(path).map_err(|err| fail(&err))?;
    let loaded = if path
        .extension()
        .is_some_and(|extension| extension == "json")
    {
        Config::from_json(&text)
    } else {
        Config::from_yaml(&text)
    };
    let config = loaded.map_err(|err| fail(&err))?;
    let report = config.check();
    for diagnostic in report.diagnostics() {
        let severity = match diagnostic.severity() {
            Severity::Error => "error",
            _ => "warning",
        };
        eprintln!("{severity}: {name}: {diagnostic}");
    }
    if report.has_errors() {
        return Err(ExitCode::FAILURE);
    }
    config.build(&Registry::new()).map_err(|err| fail(&err))
}
