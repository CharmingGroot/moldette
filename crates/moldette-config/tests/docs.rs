//! Runs the examples of the user documentation. Every configuration block
//! must build, and the responses after an input block must match. Shell,
//! Rust and plain text blocks are left alone.

use moldette_config::{Config, Registry, Specs};
use moldette_core::Response;
use moldette_spec::json::JsonResponse;

const DOCUMENTS: [(&str, &str); 4] = [
    (
        "docs/CONFIGURATION.md",
        include_str!("../../../docs/CONFIGURATION.md"),
    ),
    ("README.md", include_str!("../../../README.md")),
    (
        "docs/TROUBLESHOOTING.md",
        include_str!("../../../docs/TROUBLESHOOTING.md"),
    ),
    ("docs/SIDECAR.md", include_str!("../../../docs/SIDECAR.md")),
];

/// Blocks that show commands, code or messages rather than examples to run.
const UNCHECKED: [&str; 3] = ["bash", "rust", "text"];

struct Block<'d> {
    info: &'d str,
    line: usize,
    text: String,
}

fn blocks(document: &str) -> Vec<Block<'_>> {
    let mut blocks = Vec::new();
    let mut open: Option<Block<'_>> = None;
    for (index, line) in document.lines().enumerate() {
        match (open.take(), line.strip_prefix("```")) {
            (None, Some(info)) => {
                open = Some(Block {
                    info,
                    line: index + 1,
                    text: String::new(),
                });
            }
            (Some(block), Some("")) => blocks.push(block),
            (Some(mut block), _) => {
                block.text.push_str(line);
                block.text.push('\n');
                open = Some(block);
            }
            (None, None) => {}
        }
    }
    assert!(open.is_none(), "a code block is not closed");
    blocks
}

fn respond(specs: &Specs, raw: &str) -> serde_json::Value {
    let parser = specs.parser().expect("parser section");
    let mapper = specs.mapper().expect("mapper section");
    let resp = specs.resp().expect("resp section");
    let response = match parser.parse(raw) {
        Ok(record) => resp.respond(mapper.map(&record)),
        Err(malformed) => Response::Malformed(malformed),
    };
    serde_json::to_value(JsonResponse::new(&response)).expect("response JSON")
}

#[test]
fn documented_examples_build_and_respond_as_shown() {
    let mut outputs = 0;
    for (path, document) in DOCUMENTS {
        outputs += run_examples(path, document);
    }
    assert!(outputs > 0, "no example was run");
}

/// Runs the examples of one document and returns how many outputs matched.
fn run_examples(path: &str, document: &str) -> usize {
    let registry = Registry::new();
    let mut specs: Option<Specs> = None;
    let mut input: Option<String> = None;
    let mut outputs = 0;
    for block in blocks(document) {
        let at = format!("{path}:{}", block.line);
        match block.info {
            "yaml" | "json config" => {
                let config = if block.info == "yaml" {
                    Config::from_yaml(&block.text)
                } else {
                    Config::from_json(&block.text)
                };
                let built = config
                    .and_then(|config| config.build(&registry))
                    .unwrap_or_else(|err| panic!("{at}: {err}"));
                specs = Some(built);
                input = None;
            }
            "text input" => {
                assert!(specs.is_some(), "{at}: an input needs a configuration");
                input = Some(block.text);
            }
            "json output" => {
                let specs = specs.as_ref().expect("configuration before the output");
                let records = input.take().unwrap_or_else(|| panic!("{at}: no input"));
                let expected: Vec<serde_json::Value> = block
                    .text
                    .lines()
                    .map(|line| {
                        serde_json::from_str(line).unwrap_or_else(|err| panic!("{at}: {err}"))
                    })
                    .collect();
                let actual: Vec<serde_json::Value> =
                    records.lines().map(|raw| respond(specs, raw)).collect();
                assert_eq!(actual, expected, "{at}");
                outputs += 1;
            }
            other if UNCHECKED.contains(&other) => {}
            other => panic!("{at}: code block `{other}` is not checked"),
        }
    }
    outputs
}
