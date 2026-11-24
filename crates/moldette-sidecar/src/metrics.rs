use prometheus_client::encoding::EncodeLabelSet;
use prometheus_client::encoding::text::encode;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::metrics::histogram::{Histogram, exponential_buckets};
use prometheus_client::registry::{Registry, Unit};

use moldette::Response;

/// The statuses of a response, in the order of [`RecordCounts`].
const STATUSES: [&str; 4] = ["ok", "malformed", "missing_required", "emptied"];

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct RequestLabels {
    config: String,
    code: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct RecordLabels {
    config: String,
    status: String,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
struct ConfigLabels {
    config: String,
}

/// The metrics of the sidecar, served at `/metrics` (#124). The `config`
/// label holds a configuration name, empty for one given without a name, so
/// its values are the configurations given at start.
pub(crate) struct Metrics {
    registry: Registry,
    requests: Family<RequestLabels, Counter>,
    records: Family<RecordLabels, Counter>,
    received: Family<ConfigLabels, Counter>,
    durations: Family<ConfigLabels, Histogram>,
    in_progress: Gauge,
}

impl Metrics {
    pub(crate) fn new() -> Self {
        let requests = Family::<RequestLabels, Counter>::default();
        let records = Family::<RecordLabels, Counter>::default();
        let received = Family::<ConfigLabels, Counter>::default();
        let durations = Family::<ConfigLabels, Histogram>::new_with_constructor(|| {
            // 5 ms to about 41 s, past the default time limit of 30 s.
            Histogram::new(exponential_buckets(0.005, 2.0, 14))
        });
        let in_progress = Gauge::default();
        let mut registry = Registry::with_prefix("moldette");
        registry.register(
            "requests",
            "Requests to a processing path, by configuration and status code",
            requests.clone(),
        );
        registry.register(
            "records",
            "Records processed, by configuration and response status",
            records.clone(),
        );
        registry.register_with_unit(
            "received",
            "Request body bytes read, by configuration",
            Unit::Bytes,
            received.clone(),
        );
        registry.register_with_unit(
            "request_duration",
            "Time from the start of a request to its last response, by configuration",
            Unit::Seconds,
            durations.clone(),
        );
        registry.register(
            "requests_in_progress",
            "Requests being processed",
            in_progress.clone(),
        );
        Self {
            registry,
            requests,
            records,
            received,
            durations,
            in_progress,
        }
    }

    /// The handles of one configuration, made once at start.
    pub(crate) fn config(&self, name: &str) -> ConfigMetrics {
        let config = ConfigLabels {
            config: name.to_owned(),
        };
        ConfigMetrics {
            name: name.to_owned(),
            records: STATUSES.map(|status| {
                self.records
                    .get_or_create(&RecordLabels {
                        config: name.to_owned(),
                        status: status.to_owned(),
                    })
                    .clone()
            }),
            received: self.received.get_or_create(&config).clone(),
            duration: self.durations.get_or_create(&config).clone(),
        }
    }

    pub(crate) fn request(&self, config: &ConfigMetrics, code: u16) {
        self.requests
            .get_or_create(&RequestLabels {
                config: config.name.clone(),
                code: code.to_string(),
            })
            .inc();
    }

    pub(crate) fn in_progress(&self) -> &Gauge {
        &self.in_progress
    }

    /// The OpenMetrics text of every metric.
    pub(crate) fn text(&self) -> String {
        let mut text = String::new();
        // Writing to a `String` does not fail.
        let _ = encode(&mut text, &self.registry);
        text
    }
}

/// The metrics of one configuration.
pub(crate) struct ConfigMetrics {
    name: String,
    records: [Counter; 4],
    pub(crate) received: Counter,
    pub(crate) duration: Histogram,
}

impl ConfigMetrics {
    pub(crate) fn add_records(&self, counts: &RecordCounts) {
        for (counter, &count) in self.records.iter().zip(&counts.0) {
            if count > 0 {
                counter.inc_by(count);
            }
        }
    }
}

/// Records of one batch by response status, added to the metrics once per
/// batch.
#[derive(Default)]
pub(crate) struct RecordCounts([u64; 4]);

impl RecordCounts {
    pub(crate) fn count(&mut self, response: &Response<'_>) {
        let index = match response {
            Response::Ok(_) => 0,
            Response::Malformed(_) => 1,
            Response::MissingRequired(_) => 2,
            Response::Emptied(_) => 3,
        };
        self.0[index] += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_names_metrics_with_the_prefix_unit_and_total() {
        let metrics = Metrics::new();
        let orders = metrics.config("orders");
        metrics.request(&orders, 200);
        orders.received.inc_by(42);
        let text = metrics.text();
        for line in [
            "moldette_requests_total{config=\"orders\",code=\"200\"} 1",
            "moldette_records_total{config=\"orders\",status=\"missing_required\"} 0",
            "moldette_received_bytes_total{config=\"orders\"} 42",
            "moldette_request_duration_seconds_count{config=\"orders\"} 0",
            "moldette_requests_in_progress 0",
        ] {
            assert!(text.lines().any(|l| l == line), "{line} in\n{text}");
        }
        assert!(text.ends_with("# EOF\n"));
    }
}
