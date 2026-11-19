//! Configuration texts of the configuration benchmarks: the configuration of
//! each workload and generated configurations of many fields.

use std::fmt::Write;

use moldette_bench_support::Workload;

/// Sizes of the generated configurations.
pub const WIDE_SIZES: [(&str, usize); 2] = [("10KiB", 10 << 10), ("1MiB", 1 << 20)];

pub struct ConfigText {
    pub name: String,
    pub yaml: String,
    pub json: Option<String>,
}

/// The configuration of each workload, then generated configurations of each
/// of `WIDE_SIZES`.
pub fn config_texts() -> Vec<ConfigText> {
    let workloads = Workload::ALL.into_iter().map(|workload| ConfigText {
        name: workload.format().to_owned(),
        yaml: workload.config_yaml().to_owned(),
        json: None,
    });
    let wide = WIDE_SIZES.into_iter().map(|(label, bytes)| {
        let widths: Vec<usize> = (1..=bytes).collect();
        let width = widths[widths.partition_point(|&width| wide_yaml(width).len() < bytes)];
        ConfigText {
            name: format!("wide/{label}"),
            yaml: wide_yaml(width),
            json: Some(wide_json(width)),
        }
    });
    workloads.chain(wide).collect()
}

/// A kv configuration of `width` string fields, each read from its own key
/// and trimmed.
pub fn wide_yaml(width: usize) -> String {
    let mut yaml = String::from("schemas:\n  - name: wide\n    version: 1\n    fields:\n");
    for i in 0..width {
        let _ = writeln!(yaml, "      - {{name: field_{i}, type: string}}");
    }
    yaml.push_str("\nparser:\n  format: kv\n  pair_delimiter: \" \"\n  key_value_delimiter: \"=\"\n  fields:\n");
    for i in 0..width {
        let _ = writeln!(yaml, "    field_{i}: key_{i}");
    }
    yaml.push_str("\nmapper:\n  schema: {name: wide, version: 1}\n  rules:\n");
    for i in 0..width {
        let _ = writeln!(
            yaml,
            "    - target: field_{i}\n      from: field_{i}\n      via:\n        - trim"
        );
    }
    yaml.push_str("\nresp:\n  schema: {name: wide, version: 1}\n");
    yaml
}

/// The configuration of `wide_yaml` in JSON.
pub fn wide_json(width: usize) -> String {
    let fields: Vec<String> = (0..width)
        .map(|i| format!("{{\"name\":\"field_{i}\",\"type\":\"string\"}}"))
        .collect();
    let keys: Vec<String> = (0..width)
        .map(|i| format!("\"field_{i}\":\"key_{i}\""))
        .collect();
    let rules: Vec<String> = (0..width)
        .map(|i| format!("{{\"target\":\"field_{i}\",\"from\":\"field_{i}\",\"via\":[\"trim\"]}}"))
        .collect();
    format!(
        "{{\"schemas\":[{{\"name\":\"wide\",\"version\":1,\"fields\":[{}]}}],\"parser\":{{\"format\":\"kv\",\"pair_delimiter\":\" \",\"key_value_delimiter\":\"=\",\"fields\":{{{}}}}},\"mapper\":{{\"schema\":{{\"name\":\"wide\",\"version\":1}},\"rules\":[{}]}},\"resp\":{{\"schema\":{{\"name\":\"wide\",\"version\":1}}}}}}",
        fields.join(","),
        keys.join(","),
        rules.join(",")
    )
}

#[cfg(test)]
mod tests {
    use moldette_config::{Config, Registry};

    use super::{WIDE_SIZES, config_texts};

    #[test]
    fn every_configuration_builds_and_yaml_and_json_agree() {
        let texts = config_texts();
        assert_eq!(texts.len(), 4 + WIDE_SIZES.len());
        for text in &texts {
            let config = Config::from_yaml(&text.yaml).expect("valid YAML");
            let specs = config.build(&Registry::new()).expect("specs are built");
            assert!(specs.parser().is_some() && specs.mapper().is_some() && specs.resp().is_some());
            if let Some(json) = &text.json {
                let from_json = Config::from_json(json).expect("valid JSON");
                assert_eq!(
                    from_json.build(&Registry::new()).expect("built").digest(),
                    specs.digest()
                );
            }
        }
    }
}
