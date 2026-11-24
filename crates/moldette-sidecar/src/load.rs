use std::fs;
use std::path::Path;
use std::process::ExitCode;

use hyper::header::HeaderValue;
use moldette::Pipeline;
use moldette::config::{Config, Registry, Severity};

/// The format of a configuration text.
#[derive(Clone, Copy)]
pub(crate) enum Format {
    Yaml,
    Json,
}

impl Format {
    /// The format named `yaml` or `json`.
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            "yaml" => Some(Self::Yaml),
            "json" => Some(Self::Json),
            _ => None,
        }
    }
}

/// An error or a warning about a configuration.
pub(crate) struct Message {
    pub(crate) severity: &'static str,
    pub(crate) text: String,
}

impl Message {
    pub(crate) fn error(text: impl ToString) -> Self {
        Self {
            severity: "error",
            text: text.to_string(),
        }
    }
}

/// A configuration ready to process records.
pub(crate) struct Built {
    pub(crate) pipeline: Pipeline,
    pub(crate) digest: HeaderValue,
}

/// The messages about a configuration and, when it has no error, the
/// configuration built.
pub(crate) struct Checked {
    pub(crate) messages: Vec<Message>,
    pub(crate) built: Option<Built>,
}

pub(crate) fn check(text: &str, format: Format) -> Checked {
    let loaded = match format {
        Format::Yaml => Config::from_yaml(text),
        Format::Json => Config::from_json(text),
    };
    let config = match loaded {
        Ok(config) => config,
        Err(err) => {
            return Checked {
                messages: vec![Message::error(err)],
                built: None,
            };
        }
    };
    let report = config.check();
    let mut messages: Vec<Message> = report
        .diagnostics()
        .iter()
        .map(|diagnostic| Message {
            severity: match diagnostic.severity() {
                Severity::Error => "error",
                _ => "warning",
            },
            text: diagnostic.to_string(),
        })
        .collect();
    if report.has_errors() {
        return Checked {
            messages,
            built: None,
        };
    }
    let built = config
        .build(&Registry::new())
        .map_err(Message::error)
        .and_then(|specs| {
            Ok(Built {
                pipeline: Pipeline::from_specs(&specs).map_err(Message::error)?,
                digest: HeaderValue::from_str(specs.digest()).map_err(Message::error)?,
            })
        });
    match built {
        Ok(built) => Checked {
            messages,
            built: Some(built),
        },
        Err(message) => {
            messages.push(message);
            Checked {
                messages,
                built: None,
            }
        }
    }
}

/// Reads and builds a configuration file, printing its messages with its
/// path. A name ending in `.json` is read as JSON, any other as YAML.
pub(crate) fn file(path: &Path) -> Result<Built, ExitCode> {
    let name = path.display();
    let text = fs::read_to_string(path).map_err(|err| {
        eprintln!("error: {name}: {err}");
        ExitCode::FAILURE
    })?;
    let format = if path
        .extension()
        .is_some_and(|extension| extension == "json")
    {
        Format::Json
    } else {
        Format::Yaml
    };
    let checked = check(&text, format);
    for message in &checked.messages {
        eprintln!("{}: {name}: {}", message.severity, message.text);
    }
    checked.built.ok_or(ExitCode::FAILURE)
}
