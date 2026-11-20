mod common;

use common::{FIXED_ORDERS, NO_RULES, ORDER_SCHEMA, build_yaml, registry};
use moldette_config::{Config, Lineage, LineageSource, Registry};

type Read<'l> = (&'l str, bool, Vec<(&'l str, &'l str)>);

fn traced<'l>(
    lineage: &'l Lineage,
    name: &str,
) -> (&'l LineageSource, Vec<Read<'l>>, Vec<&'l str>) {
    let field = lineage.field(name).expect("field has a lineage");
    let reads = field
        .reads()
        .iter()
        .map(|read| {
            let origin = read
                .origin()
                .iter()
                .map(|path| (path.format(), path.path()))
                .collect();
            (read.field(), read.is_fallback(), origin)
        })
        .collect();
    let ops = field.ops().iter().map(String::as_str).collect();
    (field.source(), reads, ops)
}

fn lineage_yaml(text: &str, registry: &Registry) -> Lineage {
    build_yaml(text, registry)
        .expect("specs are built")
        .lineage()
        .expect("mapper section")
        .clone()
}

#[test]
fn traces_access_log_fields_to_regex_groups() {
    let lineages: Vec<Lineage> = [
        Config::from_yaml(include_str!("fixtures/nginx_access.yaml")).expect("valid YAML"),
        Config::from_json(include_str!("fixtures/nginx_access.json")).expect("valid JSON"),
    ]
    .iter()
    .map(|config| {
        config
            .build(&Registry::new())
            .expect("specs are built")
            .lineage()
            .expect("mapper section")
            .clone()
    })
    .collect();
    assert_eq!(lineages[0], lineages[1]);
    let lineage = &lineages[0];
    assert_eq!((lineage.schema(), lineage.version()), ("access", 1));
    let names: Vec<_> = lineage.fields().iter().map(|field| field.name()).collect();
    assert_eq!(
        names,
        [
            "client",
            "time",
            "method",
            "path",
            "http_version",
            "status",
            "status_class",
            "bytes",
            "user_agent",
            "referer",
            "site"
        ]
    );
    let group = |name| vec![("regex", name)];
    assert_eq!(
        traced(lineage, "client"),
        (
            &LineageSource::From,
            vec![
                ("forwarded_for", false, group("forwarded_for")),
                ("remote_addr", true, group("remote_addr")),
            ],
            vec!["split", "trim"]
        )
    );
    assert_eq!(
        traced(lineage, "status_class"),
        (
            &LineageSource::From,
            vec![("status", false, group("status"))],
            vec!["extract", "lookup"]
        )
    );
    assert_eq!(
        traced(lineage, "referer"),
        (
            &LineageSource::SameName,
            vec![("referer", false, group("referer"))],
            vec![]
        )
    );
    assert_eq!(
        traced(lineage, "site"),
        (&LineageSource::Constant("web-01".into()), vec![], vec![])
    );
    assert!(lineage.field("host").is_none());
}

#[test]
fn traces_coalesce_candidates_through_bodies_to_the_header() {
    let lineage = lineage_yaml(
        r#"schemas:
  - name: order
    version: 1
    fields:
      - {name: id, type: string}
      - {name: item, type: string}
parser:
  format: delimited
  delimiter: "|"
  fields: {id: "0"}
  bodies:
    - path: "2"
      format: json
      fields: {sku: /sku, note: /note}
      bodies:
        - path: /items
          format: delimited
          delimiter: ";"
          fields: {first_item: "0"}
mapper:
  schema: {name: order, version: 1}
  rules:
    - {target: item, coalesce: [first_item, sku, note], fallback: id, via: [trim]}
"#,
        &Registry::new(),
    );
    let body = ("delimited", "2");
    assert_eq!(
        traced(&lineage, "item"),
        (
            &LineageSource::Coalesce,
            vec![
                (
                    "first_item",
                    false,
                    vec![body, ("json", "/items"), ("delimited", "0")]
                ),
                ("sku", false, vec![body, ("json", "/sku")]),
                ("note", false, vec![body, ("json", "/note")]),
                ("id", true, vec![("delimited", "0")]),
            ],
            vec!["trim"]
        )
    );
}

#[test]
fn traces_fields_without_rules_by_explicit_rules() {
    let optional = NO_RULES.replace(", required: true", "");
    let same_name = lineage_yaml(&optional, &Registry::new());
    assert_eq!(
        traced(&same_name, "id"),
        (
            &LineageSource::SameName,
            vec![("id", false, vec![])],
            vec![]
        )
    );
    assert_eq!(
        traced(&same_name, "amount"),
        (
            &LineageSource::SameName,
            vec![("amount", false, vec![("kv", "amount")])],
            vec![]
        )
    );

    let explicit =
        format!("{optional}  explicit_rules: true\n  rules: [{{target: amount, from: amount}}]\n");
    let lineage = lineage_yaml(&explicit, &Registry::new());
    let sources: Vec<_> = lineage
        .fields()
        .iter()
        .map(|field| field.source())
        .collect();
    assert_eq!(
        sources,
        [
            &LineageSource::Absent,
            &LineageSource::Absent,
            &LineageSource::From
        ]
    );
    assert!(lineage.field("id").expect("field").reads().is_empty());
}

#[test]
fn traces_caller_defined_ops_and_formats_by_their_configured_names() {
    let lineage = lineage_yaml(FIXED_ORDERS, &registry());
    let column = |index| vec![("fixed_width", index)];
    assert_eq!(
        traced(&lineage, "order_id"),
        (
            &LineageSource::From,
            vec![("order_id", false, column("0"))],
            vec!["pad"]
        )
    );
    assert_eq!(
        traced(&lineage, "status"),
        (
            &LineageSource::From,
            vec![("status", false, column("1"))],
            vec!["replace", "upper"]
        )
    );
    assert_eq!(
        traced(&lineage, "amount"),
        (
            &LineageSource::SameName,
            vec![("amount", false, column("2"))],
            vec![]
        )
    );
}

#[test]
fn traces_from_the_configuration_without_a_registry() {
    let config = Config::from_yaml(FIXED_ORDERS).expect("valid YAML");
    let built = lineage_yaml(FIXED_ORDERS, &registry());
    assert_eq!(config.lineage(), Some(built));
    assert!(config.build(&Registry::new()).is_err());
}

#[test]
fn traces_only_with_a_mapper_section() {
    let without_mapper = format!("{ORDER_SCHEMA}parser: {{format: json, fields: {{id: /id}}}}\n");
    let specs = build_yaml(&without_mapper, &Registry::new()).expect("specs are built");
    assert!(specs.lineage().is_none());

    let without_parser = format!(
        "{ORDER_SCHEMA}mapper:\n  schema: {{name: order, version: 1}}\n  rules:\n    - {{target: id, from: order_no}}\n"
    );
    let lineage = lineage_yaml(&without_parser, &Registry::new());
    assert_eq!(
        traced(&lineage, "id"),
        (
            &LineageSource::From,
            vec![("order_no", false, vec![])],
            vec![]
        )
    );
}

#[test]
fn records_the_default_of_a_rule() {
    let config = format!(
        "{ORDER_SCHEMA}mapper:\n  schema: {{name: order, version: 1}}\n  rules:\n    - {{target: amount, from: total, default: '0'}}\n"
    );
    let lineage = lineage_yaml(&config, &Registry::new());
    let field = |name| lineage.field(name).expect("field has a lineage");
    assert_eq!(field("amount").default(), Some("0"));
    assert_eq!(field("id").default(), None);
}
