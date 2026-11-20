//! Specs shared by the fuzz targets, built from the test configurations.

use moldette::Pipeline;
use moldette::config::{Config, Registry, Specs};
use moldette::parser::ParserSpec;

pub const ORDER_API: &str =
    include_str!("../../crates/moldette-config/tests/fixtures/order_api.yaml");
pub const ORDER_EXPORT: &str =
    include_str!("../../crates/moldette-config/tests/fixtures/order_export.yaml");
pub const NGINX_ACCESS: &str =
    include_str!("../../crates/moldette-config/tests/fixtures/nginx_access.yaml");
pub const ORDER_BODIES: &str =
    include_str!("../../crates/moldette-config/tests/fixtures/order_bodies.yaml");
pub const KV: &str = "parser:
  format: kv
  pair_delimiter: ' '
  key_value_delimiter: '='
  fields: {id: id, total: total, note: note}
";

pub fn specs(yaml: &str) -> Specs {
    Config::from_yaml(yaml)
        .expect("test configuration loads")
        .build(&Registry::new())
        .expect("test configuration builds")
}

pub fn parser(yaml: &str) -> ParserSpec {
    specs(yaml).parser().expect("parser section").clone()
}

pub fn pipelines() -> Vec<Pipeline> {
    [ORDER_API, ORDER_EXPORT, NGINX_ACCESS]
        .into_iter()
        .map(|yaml| Pipeline::from_specs(&specs(yaml)).expect("pipeline sections"))
        .collect()
}
