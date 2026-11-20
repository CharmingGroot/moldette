mod common;

use std::collections::HashSet;

use common::build_yaml;
use moldette_config::{Config, Registry};

const CSV_ORDERS: &str = r#"schemas:
  - name: order
    version: 1
    fields:
      - {name: id, type: string, required: true}
      - {name: amount, type: float}
parser:
  format: delimited
  delimiter: ","
  fields: {id: "0", amount: "1"}
mapper:
  schema: {name: order, version: 1}
  rules:
    - {target: amount, from: amount, via: [trim]}
resp:
  schema: {name: order, version: 1}
"#;

fn digest_yaml(text: &str) -> String {
    build_yaml(text, &Registry::new())
        .expect("specs are built")
        .digest()
        .to_owned()
}

#[test]
fn digests_equal_configurations_alike_in_yaml_and_json() {
    let digest = digest_yaml(CSV_ORDERS);
    assert_eq!(
        digest,
        "sha256:9d8d0ccb10f3886678daef777c57c53fdd6c944847528f7f2c24e7b3f4953589"
    );

    let rewritten = r#"# Orders exported as CSV.
resp: {strict: false, schema: {version: 1, name: order}}
mapper:
  rules:
    - via:
        - trim
      from: amount
      target: amount
  schema: {version: 1, name: order}
parser: {fields: {id: 0, amount: 1}, delimiter: ",", format: delimited}
schemas:
  - fields:
      - {type: string, required: true, name: id}
      - {name: amount, type: float, required: false}
    version: 1
    name: order
"#;
    assert_eq!(digest_yaml(rewritten), digest);

    let json = Config::from_json(
        r#"{"schemas":[{"name":"order","version":1,"fields":[{"name":"id","type":"string","required":true},{"name":"amount","type":"float"}]}],"parser":{"format":"delimited","delimiter":",","fields":{"id":"0","amount":"1"}},"mapper":{"schema":{"name":"order","version":1},"rules":[{"target":"amount","from":"amount","via":["trim"]}]},"resp":{"schema":{"name":"order","version":1}}}"#,
    )
    .expect("valid JSON");
    let specs = json.build(&Registry::new()).expect("specs are built");
    assert_eq!(specs.digest(), digest);

    let fixtures = [
        Config::from_yaml(include_str!("fixtures/nginx_access.yaml")).expect("valid YAML"),
        Config::from_json(include_str!("fixtures/nginx_access.json")).expect("valid JSON"),
    ]
    .map(|config| {
        let specs = config.build(&Registry::new()).expect("specs are built");
        specs.digest().to_owned()
    });
    assert_eq!(fixtures[0], fixtures[1]);
    assert_eq!(
        fixtures[0],
        "sha256:0e917a525079ecb2b61693a816a48e96c19a6240f772c052b2683dd681e8163f"
    );
    assert_ne!(fixtures[0], digest);
}

#[test]
fn digests_configurations_that_differ_differently() {
    let variants = [
        CSV_ORDERS.to_owned(),
        CSV_ORDERS.replace(r#"delimiter: ",""#, r#"delimiter: ";""#),
        CSV_ORDERS.replace("type: float", "type: string"),
        CSV_ORDERS.replace(", required: true", ""),
        CSV_ORDERS.replace("via: [trim]", "via: [trim, empty_as_none]"),
        CSV_ORDERS.replace(r#"{id: "0", amount: "1"}"#, r#"{amount: "1", id: "0"}"#),
        CSV_ORDERS.replace(
            "      - {name: id, type: string, required: true}\n      - {name: amount, type: float}\n",
            "      - {name: amount, type: float}\n      - {name: id, type: string, required: true}\n",
        ),
        format!("{CSV_ORDERS}  strict: true\n"),
        CSV_ORDERS.replace("via: [trim]", "via: [{timestamp: {formats: ['[day]']}}]"),
        CSV_ORDERS.replace("via: [trim]", "via: [{timestamp: {formats: ['[day]'], year: 2023}}]"),
        CSV_ORDERS.replace(
            "via: [trim]",
            "via: [{timestamp: {formats: ['[day]'], offset: '+09:00'}}]",
        ),
        CSV_ORDERS.replace("via: [trim]", "via: [{lookup: {table: {a: b}}}]"),
        CSV_ORDERS.replace("via: [trim]", "via: [{lookup: {table: {a: b}, default: c}}]"),
        CSV_ORDERS.replace(
            "via: [trim]",
            "via: [{lookup: {table: {a: b}, keep_unmatched: true}}]",
        ),
        CSV_ORDERS.replace("via: [trim]", "via: [trim], default: '0'"),
        CSV_ORDERS.replace("via: [trim]", "via: [trim], default: '1'"),
    ];
    let digests: HashSet<String> = variants.iter().map(|text| digest_yaml(text)).collect();
    assert_eq!(digests.len(), variants.len());
    for digest in &digests {
        let hex = digest.strip_prefix("sha256:").expect("algorithm prefix");
        assert_eq!(hex.len(), 64);
        assert!(hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')));
    }
}
