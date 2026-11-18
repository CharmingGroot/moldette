mod common;

use std::sync::Arc;

use common::{
    FIXED_ORDERS, Upper, build_error, fixed_width, invalid_argument, location, pad, registry, spec,
};
use moldette_config::{Arguments, LoadErrorKind, Registry, RegistryError, ValueKind};
use moldette_mapper::Op;

#[test]
fn reads_caller_defined_arguments_with_key_path_and_position() {
    let registry = registry();
    let cases = [
        (
            FIXED_ORDERS.replace("width: 8", "widht: 8"),
            LoadErrorKind::MissingKey("width".into()),
            "mapper.rules[0].via[0].pad",
            Some((20, 16)),
        ),
        (
            FIXED_ORDERS.replace("width: 8", "width: wide"),
            LoadErrorKind::TypeMismatch(ValueKind::Integer),
            "mapper.rules[0].via[0].pad.width",
            Some((20, 24)),
        ),
        (
            FIXED_ORDERS.replace("width: 8", "width: 0"),
            spec(invalid_argument("pad.width", "0")),
            "mapper.rules[0].via[0].pad.width",
            Some((20, 24)),
        ),
        (
            FIXED_ORDERS.replace("fill: \"0\"", "fill: \"00\""),
            LoadErrorKind::TypeMismatch(ValueKind::Character),
            "mapper.rules[0].via[0].pad.fill",
            Some((20, 33)),
        ),
        (
            FIXED_ORDERS.replace("[6, 8, 8]", "[6, eight, 8]"),
            LoadErrorKind::TypeMismatch(ValueKind::Integer),
            "parser.widths[1]",
            Some((12, 15)),
        ),
        (
            FIXED_ORDERS.replace("[6, 8, 8]", "[6, 0, 8]"),
            spec(invalid_argument("fixed_width.widths", "0")),
            "parser.widths[1]",
            Some((12, 15)),
        ),
        (
            FIXED_ORDERS.replace("[6, 8, 8]", "6"),
            LoadErrorKind::TypeMismatch(ValueKind::Sequence),
            "parser.widths",
            Some((12, 11)),
        ),
        (
            FIXED_ORDERS.replace("paid: settled", "paid: [settled]"),
            LoadErrorKind::TypeMismatch(ValueKind::String),
            "mapper.rules[1].via[0].replace.pairs.paid",
            Some((24, 35)),
        ),
        (
            FIXED_ORDERS.replace("{pairs: {paid: settled, open: pending}}", "{pairs: [paid]}"),
            LoadErrorKind::TypeMismatch(ValueKind::Map),
            "mapper.rules[1].via[0].replace.pairs",
            Some((24, 28)),
        ),
    ];
    for (text, kind, path, position) in cases {
        let err = build_error(&text, &registry);
        assert_eq!(location(&err), (&kind, path, position), "{path}");
    }
}

#[test]
fn rejects_arguments_the_constructor_does_not_read() {
    let registry = registry();
    let err = build_error(
        &FIXED_ORDERS.replace("width: 8, fill", "width: 8, fil"),
        &registry,
    );
    assert_eq!(
        location(&err),
        (
            &LoadErrorKind::UnknownKey("fil".into()),
            "mapper.rules[0].via[0].pad",
            Some((20, 27))
        )
    );

    let err = build_error(&FIXED_ORDERS.replace("  padding:", "  paddng:"), &registry);
    assert_eq!(
        location(&err),
        (
            &LoadErrorKind::UnknownKey("paddng".into()),
            "parser",
            Some((11, 3))
        )
    );

    let err = build_error(&FIXED_ORDERS.replace("  widths:", "  width:"), &registry);
    assert_eq!(
        location(&err),
        (
            &LoadErrorKind::MissingKey("widths".into()),
            "parser",
            Some((10, 11))
        )
    );

    let err = build_error(
        &FIXED_ORDERS.replace("- upper", "- upper: {case: title}"),
        &registry,
    );
    assert_eq!(
        location(&err),
        (
            &LoadErrorKind::UnknownKey("case".into()),
            "mapper.rules[1].via[1].upper",
            Some((25, 19))
        )
    );
}

#[test]
fn rejects_names_missing_from_the_registry() {
    let err = build_error(FIXED_ORDERS, &Registry::new());
    assert_eq!(
        location(&err),
        (
            &LoadErrorKind::UnknownFormat("fixed_width".into()),
            "parser",
            Some((10, 11))
        )
    );

    let err = build_error(&FIXED_ORDERS.replace("- upper", "- uper"), &registry());
    assert_eq!(
        location(&err),
        (
            &LoadErrorKind::UnknownOp("uper".into()),
            "mapper.rules[1].via[1].uper",
            Some((25, 11))
        )
    );
}

#[test]
fn rejects_constructed_names_other_than_the_registered_name() {
    let mismatch = |registered: &str, reported: &str| LoadErrorKind::NameMismatch {
        registered: registered.into(),
        reported: reported.into(),
    };
    let formats = Registry::new()
        .format("fixed", fixed_width)
        .expect("name is free");
    let err = build_error(
        &FIXED_ORDERS.replace("format: fixed_width", "format: fixed"),
        &formats,
    );
    assert_eq!(
        location(&err),
        (
            &mismatch("fixed", "fixed_width"),
            "parser.format",
            Some((10, 11))
        )
    );
    assert_eq!(
        err.to_string(),
        "parser.format: registered as `fixed` but named `fixed_width` at line 10, column 11"
    );

    let ops = registry()
        .op("shout", |_: &mut Arguments| {
            Ok(Arc::new(Upper) as Arc<dyn Op>)
        })
        .expect("name is free");
    let err = build_error(&FIXED_ORDERS.replace("- upper", "- shout"), &ops);
    assert_eq!(
        location(&err),
        (
            &mismatch("shout", "upper"),
            "mapper.rules[1].via[1].shout",
            Some((25, 11))
        )
    );
}

#[test]
fn rejects_built_in_and_repeated_names() {
    assert_eq!(
        Registry::new().op("trim", pad).map(drop),
        Err(RegistryError::BuiltInName("trim".into()))
    );
    assert_eq!(
        Registry::new().format("json", fixed_width).map(drop),
        Err(RegistryError::BuiltInName("json".into()))
    );
    assert_eq!(
        registry().op("pad", pad).map(drop),
        Err(RegistryError::DuplicateName("pad".into()))
    );
    assert!(Registry::new().op("json", pad).is_ok());
}
