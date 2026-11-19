use std::borrow::Cow;
use std::ops::Range;

use moldette_core::{Field, FieldRecord, Mapped, OutputSchema, Timestamp, Type, Value};

const NOV_18_2026: i64 = 1_794_960_000;
const PLUS_NINE_HOURS: i32 = 9 * 3600;

/// Size of one record. `Small` is the natural size of a workload record,
/// between about 70 B and 250 B; the others pad one text field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Small,
    Medium,
    Large,
}

impl Size {
    pub const ALL: [Size; 3] = [Size::Small, Size::Medium, Size::Large];

    pub fn label(self) -> &'static str {
        match self {
            Size::Small => "100B",
            Size::Medium => "10KiB",
            Size::Large => "1MiB",
        }
    }

    /// Record indices a benchmark of this size runs on each iteration.
    pub fn indices(self) -> Range<usize> {
        match self {
            Size::Small | Size::Medium => 0..16,
            Size::Large => 0..4,
        }
    }

    fn bytes(self) -> Option<usize> {
        match self {
            Size::Small => None,
            Size::Medium => Some(10 << 10),
            Size::Large => Some(1 << 20),
        }
    }
}

/// Total sizes of a batch of `Small` records.
pub const BATCHES: [(&str, usize); 2] = [("1MiB", 1 << 20), ("16MiB", 16 << 20)];

/// One record shape per parser format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Workload {
    /// nginx access log line, read by `regex`.
    AccessLog,
    /// Order CSV row, read by `delimited`.
    OrderExport,
    /// Order API response body, read by `json`.
    OrderApi,
    /// Order event line, read by `kv`.
    OrderEvents,
}

struct Shape {
    raw: fn(usize, &str) -> String,
    fields: fn(usize, &str) -> FieldRecord<'static>,
    mapped: fn(usize, &str) -> Mapped<'static>,
}

impl Workload {
    pub const ALL: [Workload; 4] = [
        Workload::AccessLog,
        Workload::OrderExport,
        Workload::OrderApi,
        Workload::OrderEvents,
    ];

    pub fn format(self) -> &'static str {
        match self {
            Workload::AccessLog => "regex",
            Workload::OrderExport => "delimited",
            Workload::OrderApi => "json",
            Workload::OrderEvents => "kv",
        }
    }

    pub fn schema(self) -> OutputSchema {
        let fields: &[(&str, Type, bool)] = match self {
            Workload::AccessLog => &[
                ("client", Type::Ip, true),
                ("time", Type::Timestamp, true),
                ("method", Type::String, false),
                ("path", Type::String, false),
                ("http_version", Type::String, false),
                ("status", Type::Int, true),
                ("status_class", Type::String, false),
                ("bytes", Type::Int, false),
                ("user_agent", Type::String, false),
                ("referer", Type::String, false),
                ("site", Type::String, false),
            ],
            Workload::OrderExport => &[
                ("id", Type::String, true),
                ("placed_at", Type::Timestamp, true),
                ("customer", Type::String, false),
                ("product", Type::String, false),
                ("quantity", Type::Int, true),
                ("unit_price", Type::Float, false),
                ("gift", Type::Bool, false),
                ("note", Type::String, false),
            ],
            Workload::OrderApi => &[
                ("id", Type::String, true),
                ("status", Type::String, false),
                ("customer", Type::String, false),
                ("city", Type::String, false),
                ("first_sku", Type::String, false),
                ("first_quantity", Type::Int, false),
                ("total", Type::Float, true),
                ("paid", Type::Bool, false),
                ("created_at", Type::Timestamp, true),
                ("coupon", Type::String, false),
            ],
            Workload::OrderEvents => &[
                ("id", Type::String, true),
                ("time", Type::Timestamp, true),
                ("customer", Type::String, false),
                ("amount", Type::Float, true),
                ("paid", Type::Bool, false),
                ("channel", Type::String, false),
                ("note", Type::String, false),
            ],
        };
        let (name, version) = match self {
            Workload::AccessLog => ("access", 1),
            Workload::OrderExport => ("order_line", 1),
            Workload::OrderApi => ("order", 1),
            Workload::OrderEvents => ("order_event", 1),
        };
        fields
            .iter()
            .fold(
                OutputSchema::builder(name, version),
                |builder, &(field, ty, required)| {
                    let field = Field::new(field, ty);
                    builder.field(if required { field.required() } else { field })
                },
            )
            .build()
            .expect("valid schema")
    }

    /// The configuration that builds the parser, mapper and resp specs of
    /// this workload.
    pub fn config_yaml(self) -> &'static str {
        match self {
            Workload::AccessLog => {
                include_str!("../../../crates/moldette-config/tests/fixtures/nginx_access.yaml")
            }
            Workload::OrderExport => {
                include_str!("../../../crates/moldette-config/tests/fixtures/order_export.yaml")
            }
            Workload::OrderApi => {
                include_str!("../../../crates/moldette-config/tests/fixtures/order_api.yaml")
            }
            Workload::OrderEvents => include_str!("../data/order_events.yaml"),
        }
    }

    /// Record `i` as raw text.
    pub fn raw(self, i: usize, size: Size) -> String {
        (self.shape().raw)(i, &self.padding(i, size))
    }

    /// Record `i` as the parser returns it.
    pub fn fields(self, i: usize, size: Size) -> FieldRecord<'static> {
        (self.shape().fields)(i, &self.padding(i, size))
    }

    /// Record `i` as the mapper returns it.
    pub fn mapped(self, i: usize, size: Size) -> Mapped<'static> {
        (self.shape().mapped)(i, &self.padding(i, size))
    }

    fn padding(self, i: usize, size: Size) -> String {
        size.bytes().map_or_else(String::new, |bytes| {
            "x".repeat(bytes.saturating_sub((self.shape().raw)(i, "").len()))
        })
    }

    fn shape(self) -> Shape {
        match self {
            Workload::AccessLog => Shape {
                raw: access_raw,
                fields: access_fields,
                mapped: access_mapped,
            },
            Workload::OrderExport => Shape {
                raw: export_raw,
                fields: export_fields,
                mapped: export_mapped,
            },
            Workload::OrderApi => Shape {
                raw: api_raw,
                fields: api_fields,
                mapped: api_mapped,
            },
            Workload::OrderEvents => Shape {
                raw: events_raw,
                fields: events_fields,
                mapped: events_mapped,
            },
        }
    }
}

/// `Small` records of `workload` from index 0 until their total length
/// reaches `bytes`.
pub fn batch(workload: Workload, bytes: usize) -> Vec<String> {
    let mut total = 0;
    (0..)
        .map(|i| workload.raw(i, Size::Small))
        .take_while(|record| {
            let more = total < bytes;
            total += record.len();
            more
        })
        .collect()
}

/// A view of `record` whose names and values borrow from it, as a parser
/// returns values borrowed from the raw record.
pub fn borrowed<'a>(record: &'a FieldRecord<'_>) -> FieldRecord<'a> {
    let mut view = FieldRecord::with_capacity(record.len());
    for (name, value) in record.iter() {
        view.insert(name, value);
    }
    view
}

/// Compares two field records regardless of field order.
pub fn same_fields(left: &FieldRecord<'_>, right: &FieldRecord<'_>) -> bool {
    let mut left: Vec<_> = left.iter().collect();
    let mut right: Vec<_> = right.iter().collect();
    left.sort_unstable();
    right.sort_unstable();
    left == right
}

/// Compares two mapped records regardless of field order.
pub fn same_mapped(left: &Mapped<'_>, right: &Mapped<'_>) -> bool {
    fn sorted<'m, 'a>(mapped: &'m Mapped<'a>) -> (Vec<(&'m str, &'m Value<'a>)>, Vec<&'m str>) {
        let mut values: Vec<_> = mapped.iter().collect();
        let mut emptied: Vec<_> = mapped.emptied().collect();
        values.sort_unstable_by_key(|&(name, _)| name);
        emptied.sort_unstable();
        (values, emptied)
    }
    sorted(left) == sorted(right)
}

struct Clock {
    hour: usize,
    minute: usize,
    second: usize,
}

impl Clock {
    fn of(i: usize) -> Self {
        Self {
            hour: i / 3600 % 24,
            minute: i / 60 % 60,
            second: i % 60,
        }
    }

    fn timestamp(&self, offset_seconds: i32) -> Value<'static> {
        let local = NOV_18_2026 + (self.hour * 3600 + self.minute * 60 + self.second) as i64;
        Value::Timestamp(
            Timestamp::new(local - i64::from(offset_seconds), 0, offset_seconds)
                .expect("valid timestamp"),
        )
    }

    fn time(&self) -> String {
        format!("{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }
}

fn record(fields: Vec<(&'static str, String)>) -> FieldRecord<'static> {
    let mut record = FieldRecord::with_capacity(fields.len());
    for (name, value) in fields {
        record.insert(name, value);
    }
    record
}

fn mapped(values: Vec<(&'static str, Value<'static>)>) -> Mapped<'static> {
    let mut mapped = Mapped::new();
    for (name, value) in values {
        mapped.insert(name, value);
    }
    mapped
}

fn text(value: impl Into<String>) -> Value<'static> {
    Value::Str(Cow::Owned(value.into()))
}

fn float(text: &str) -> Value<'static> {
    Value::Float(text.parse().expect("float"))
}

struct Access {
    remote_addr: String,
    time_local: String,
    method: &'static str,
    uri: String,
    protocol: &'static str,
    version: &'static str,
    status: u16,
    status_class: &'static str,
    bytes: usize,
    forwarded_for: String,
    client: String,
    clock: Clock,
}

const REFERER: &str = "https://shop.example/cart";
const AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64)";

impl Access {
    fn of(i: usize, padding: &str) -> Self {
        let clock = Clock::of(i);
        let (method, protocol, version) =
            [("GET", "HTTP/1.1", "1.1"), ("POST", "HTTP/2.0", "2.0")][i % 2];
        let (status, status_class) = [
            (200, "success"),
            (201, "success"),
            (304, "redirection"),
            (404, "client_error"),
            (500, "server_error"),
        ][i % 5];
        let remote_addr = format!("203.0.113.{}", i % 254 + 1);
        let (forwarded_for, client) = if i.is_multiple_of(3) {
            ("-".to_owned(), remote_addr.clone())
        } else {
            let client = format!("198.51.100.{}", i % 250);
            (format!("{client}, 203.0.113.{}", i % 200), client)
        };
        Self {
            remote_addr,
            time_local: format!("18/Nov/2026:{} +0000", clock.time()),
            method,
            uri: format!("/orders?id=A-{i}{padding}"),
            protocol,
            version,
            status,
            status_class,
            bytes: i * 37 % 8192,
            forwarded_for,
            client,
            clock,
        }
    }
}

fn access_raw(i: usize, padding: &str) -> String {
    let a = Access::of(i, padding);
    format!(
        "{} - - [{}] \"{} {} {}\" {} {} \"{REFERER}\" \"{AGENT}\" \"{}\"",
        a.remote_addr,
        a.time_local,
        a.method,
        a.uri,
        a.protocol,
        a.status,
        a.bytes,
        a.forwarded_for
    )
}

fn access_fields(i: usize, padding: &str) -> FieldRecord<'static> {
    let a = Access::of(i, padding);
    record(vec![
        ("remote_addr", a.remote_addr),
        ("time_local", a.time_local),
        ("method", a.method.to_owned()),
        ("uri", a.uri),
        ("protocol", a.protocol.to_owned()),
        ("status", a.status.to_string()),
        ("bytes", a.bytes.to_string()),
        ("referer", REFERER.to_owned()),
        ("agent", AGENT.to_owned()),
        ("forwarded_for", a.forwarded_for),
    ])
}

fn access_mapped(i: usize, padding: &str) -> Mapped<'static> {
    let a = Access::of(i, padding);
    mapped(vec![
        ("client", Value::Ip(a.client.parse().expect("ip"))),
        ("time", a.clock.timestamp(0)),
        ("method", text(a.method)),
        ("path", text("/orders")),
        ("http_version", text(a.version)),
        ("status", Value::Int(a.status.into())),
        ("status_class", text(a.status_class)),
        ("bytes", Value::Int(a.bytes as i64)),
        ("user_agent", text(AGENT)),
        ("referer", text(REFERER)),
        ("site", text("web-01")),
    ])
}

struct Export {
    id: String,
    placed_at: String,
    customer: String,
    product: &'static str,
    quantity: usize,
    unit_price: String,
    gift: bool,
    note: String,
    clock: Clock,
}

impl Export {
    fn of(i: usize, padding: &str) -> Self {
        let clock = Clock::of(i);
        Self {
            id: format!("A-{i}"),
            placed_at: format!("2026-11-18 {}", clock.time()),
            customer: format!("C-{}", i % 500),
            product: ["Notebook", "Desk lamp, brass", "Pencil set"][i % 3],
            quantity: i % 9 + 1,
            unit_price: format!("{}.{:02}", i % 90 + 1, i % 100),
            gift: i.is_multiple_of(2),
            note: format!(
                "{}{padding}",
                ["", "Leave at the \"side\" door", "gift wrap"][i % 3]
            ),
            clock,
        }
    }
}

fn csv_field(value: &str) -> Cow<'_, str> {
    if value.contains([',', '"']) {
        Cow::Owned(format!("\"{}\"", value.replace('"', "\"\"")))
    } else {
        Cow::Borrowed(value)
    }
}

fn export_raw(i: usize, padding: &str) -> String {
    let e = Export::of(i, padding);
    format!(
        "{},{},{},{},{},{},{},{}",
        e.id,
        e.placed_at,
        e.customer,
        csv_field(e.product),
        e.quantity,
        e.unit_price,
        if e.gift { "yes" } else { "no" },
        csv_field(&e.note)
    )
}

fn export_fields(i: usize, padding: &str) -> FieldRecord<'static> {
    let e = Export::of(i, padding);
    let mut fields = vec![
        ("id", e.id),
        ("placed_at", e.placed_at),
        ("customer", e.customer),
        ("product", e.product.to_owned()),
        ("quantity", e.quantity.to_string()),
        ("unit_price", e.unit_price),
        ("gift", if e.gift { "yes" } else { "no" }.to_owned()),
    ];
    if !e.note.is_empty() {
        fields.push(("note", e.note));
    }
    record(fields)
}

fn export_mapped(i: usize, padding: &str) -> Mapped<'static> {
    let e = Export::of(i, padding);
    let mut values = vec![
        ("id", text(e.id)),
        ("placed_at", e.clock.timestamp(0)),
        ("customer", text(e.customer)),
        ("product", text(e.product)),
        ("quantity", Value::Int(e.quantity as i64)),
        ("unit_price", float(&e.unit_price)),
        ("gift", Value::Bool(e.gift)),
    ];
    if !e.note.is_empty() {
        values.push(("note", text(e.note)));
    }
    mapped(values)
}

struct Api {
    id: String,
    status: &'static str,
    customer: String,
    city: Option<String>,
    first_sku: String,
    first_quantity: usize,
    second_sku: String,
    total: String,
    paid: bool,
    created_at: String,
    coupon: String,
    page: usize,
    clock: Clock,
}

impl Api {
    fn of(i: usize, padding: &str) -> Self {
        let clock = Clock::of(i);
        Self {
            id: format!("A-{i}"),
            status: ["paid", "pending", "on hold"][i % 3],
            customer: format!("C-{}", i % 500),
            city: (!i.is_multiple_of(4)).then(|| format!("City {}", i % 40)),
            first_sku: format!("S-{}", i % 64),
            first_quantity: i % 5 + 1,
            second_sku: format!("S-{}", i % 32),
            total: format!("{}.{:02}", i % 200 + 1, i % 100),
            paid: i.is_multiple_of(2),
            created_at: format!("2026-11-18T{}Z", clock.time()),
            coupon: format!("C{}{padding}", i % 10),
            page: i % 7,
            clock,
        }
    }
}

fn api_raw(i: usize, padding: &str) -> String {
    let a = Api::of(i, padding);
    let address = a.city.as_ref().map_or_else(
        || "null".to_owned(),
        |city| format!("{{\"city\":\"{city}\"}}"),
    );
    format!(
        "{{\"id\":\"{}\",\"status\":\"{}\",\"customer\":{{\"id\":\"{}\",\"address\":{address}}},\"items\":[{{\"sku\":\"{}\",\"quantity\":{}}},{{\"sku\":\"{}\",\"quantity\":1}}],\"total\":{},\"paid\":{},\"created_at\":\"{}\",\"coupon\":\"{}\",\"meta\":{{\"page\":{}}}}}",
        a.id,
        a.status,
        a.customer,
        a.first_sku,
        a.first_quantity,
        a.second_sku,
        a.total,
        a.paid,
        a.created_at,
        a.coupon,
        a.page
    )
}

fn api_fields(i: usize, padding: &str) -> FieldRecord<'static> {
    let a = Api::of(i, padding);
    let mut fields = vec![
        ("id", a.id),
        ("status", a.status.to_owned()),
        ("customer", a.customer),
        ("first_sku", a.first_sku),
        ("first_quantity", a.first_quantity.to_string()),
        ("total", a.total),
        ("paid", a.paid.to_string()),
        ("created_at", a.created_at),
        ("coupon", a.coupon),
    ];
    if let Some(city) = a.city {
        fields.push(("city", city));
    }
    record(fields)
}

fn api_mapped(i: usize, padding: &str) -> Mapped<'static> {
    let a = Api::of(i, padding);
    let mut values = vec![
        ("id", text(a.id)),
        ("status", text(a.status)),
        ("customer", text(a.customer)),
        ("first_sku", text(a.first_sku)),
        ("first_quantity", Value::Int(a.first_quantity as i64)),
        ("total", float(&a.total)),
        ("paid", Value::Bool(a.paid)),
        ("created_at", a.clock.timestamp(0)),
        ("coupon", text(a.coupon)),
    ];
    if let Some(city) = a.city {
        values.push(("city", text(city)));
    }
    mapped(values)
}

struct Event {
    id: String,
    time: String,
    customer: String,
    amount: String,
    paid: bool,
    channel: (&'static str, &'static str),
    note: String,
    clock: Clock,
}

impl Event {
    fn of(i: usize, padding: &str) -> Self {
        let clock = Clock::of(i);
        Self {
            id: format!("E-{i}"),
            time: format!("2026-11-18T{}+09:00", clock.time()),
            customer: format!("C-{}", i % 500),
            amount: format!("{}.{:02}", i % 300 + 1, i % 100),
            paid: i.is_multiple_of(2),
            channel: [("w", "web"), ("s", "store")][i % 2],
            note: format!("n{padding}"),
            clock,
        }
    }
}

fn events_raw(i: usize, padding: &str) -> String {
    let e = Event::of(i, padding);
    format!(
        "id={} ts={} customer={} amount={} paid={} ch={} note={}",
        e.id, e.time, e.customer, e.amount, e.paid, e.channel.0, e.note
    )
}

fn events_fields(i: usize, padding: &str) -> FieldRecord<'static> {
    let e = Event::of(i, padding);
    record(vec![
        ("id", e.id),
        ("time", e.time),
        ("customer", e.customer),
        ("amount", e.amount),
        ("paid", e.paid.to_string()),
        ("channel", e.channel.0.to_owned()),
        ("note", e.note),
    ])
}

fn events_mapped(i: usize, padding: &str) -> Mapped<'static> {
    let e = Event::of(i, padding);
    mapped(vec![
        ("id", text(e.id)),
        ("time", e.clock.timestamp(PLUS_NINE_HOURS)),
        ("customer", text(e.customer)),
        ("amount", float(&e.amount)),
        ("paid", Value::Bool(e.paid)),
        ("channel", text(e.channel.1)),
        ("note", text(e.note)),
    ])
}

#[cfg(test)]
mod tests {
    use super::{BATCHES, Size, Workload, batch};

    #[test]
    fn padded_records_reach_their_size() {
        for workload in Workload::ALL {
            assert!(workload.raw(7, Size::Small).len() < 300);
            assert_eq!(workload.raw(7, Size::Medium).len(), 10 << 10);
            assert_eq!(workload.raw(7, Size::Large).len(), 1 << 20);
        }
    }

    #[test]
    fn batches_reach_their_total_size() {
        for workload in Workload::ALL {
            let (_, bytes) = BATCHES[0];
            let records = batch(workload, bytes);
            let total: usize = records.iter().map(String::len).sum();
            assert!(total >= bytes && total - records.last().map_or(0, String::len) < bytes);
        }
    }
}
