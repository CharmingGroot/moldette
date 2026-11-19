//! Parser specs of the benchmark workloads, built with the parser builder only.

use moldette_bench_support::Workload;
use moldette_parser::{Format, ParserSpec};

const ACCESS_LOG_PATTERN: &str = r#"^(?P<remote_addr>\S+) \S+ \S+ \[(?P<time_local>[^\]]+)\] "(?P<method>\S+) (?P<uri>\S+) (?P<protocol>[^"]+)" (?P<status>\d{3}) (?P<bytes>\d+|-) "(?P<referer>[^"]*)" "(?P<agent>[^"]*)" "(?P<forwarded_for>[^"]*)"$"#;

pub fn parser_spec(workload: Workload) -> ParserSpec {
    let (format, fields, empty_as_none): (Format, &[(&str, &str)], bool) = match workload {
        Workload::AccessLog => (
            Format::Regex {
                pattern: ACCESS_LOG_PATTERN.to_owned(),
            },
            &[
                ("remote_addr", "remote_addr"),
                ("time_local", "time_local"),
                ("method", "method"),
                ("uri", "uri"),
                ("protocol", "protocol"),
                ("status", "status"),
                ("bytes", "bytes"),
                ("referer", "referer"),
                ("agent", "agent"),
                ("forwarded_for", "forwarded_for"),
            ],
            true,
        ),
        Workload::OrderExport => (
            Format::Delimited { delimiter: ',' },
            &[
                ("id", "0"),
                ("placed_at", "1"),
                ("customer", "2"),
                ("product", "3"),
                ("quantity", "4"),
                ("unit_price", "5"),
                ("gift", "6"),
                ("note", "7"),
            ],
            true,
        ),
        Workload::OrderApi => (
            Format::Json,
            &[
                ("id", "/id"),
                ("status", "/status"),
                ("customer", "/customer/id"),
                ("city", "/customer/address/city"),
                ("first_sku", "/items/0/sku"),
                ("first_quantity", "/items/0/quantity"),
                ("total", "/total"),
                ("paid", "/paid"),
                ("created_at", "/created_at"),
                ("coupon", "/coupon"),
            ],
            false,
        ),
        Workload::OrderEvents => (
            Format::Kv {
                pair_delimiter: ' ',
                key_value_delimiter: '=',
            },
            &[
                ("id", "id"),
                ("time", "ts"),
                ("customer", "customer"),
                ("amount", "amount"),
                ("paid", "paid"),
                ("channel", "ch"),
                ("note", "note"),
            ],
            false,
        ),
    };
    let builder = fields
        .iter()
        .fold(ParserSpec::builder(format), |builder, &(name, path)| {
            builder.field(name, path)
        });
    let builder = if empty_as_none {
        builder.empty_as_none()
    } else {
        builder
    };
    builder.build().expect("valid parser spec")
}

#[cfg(test)]
mod tests {
    use moldette_bench_support::{Size, Workload, same_fields};

    use super::parser_spec;

    #[test]
    fn parses_raw_records_into_the_workload_fields() {
        for workload in Workload::ALL {
            let spec = parser_spec(workload);
            for size in Size::ALL {
                for i in size.indices() {
                    let raw = workload.raw(i, size);
                    let parsed = spec.parse(&raw).expect("record parses");
                    assert!(
                        same_fields(&parsed, &workload.fields(i, size)),
                        "{workload:?} {size:?} {i}: {parsed:?}"
                    );
                }
            }
        }
    }
}
