#[cfg(not(feature = "tracing"))]
pub(crate) use disabled::*;
#[cfg(feature = "tracing")]
pub(crate) use enabled::*;

#[cfg(feature = "tracing")]
mod enabled {
    use moldette_core::{OutputSchema, StatusCounts};
    pub(crate) use tracing::Span;
    use tracing::field::Empty;
    use tracing::info_span;

    pub(crate) fn batch_span(schema: &OutputSchema) -> Span {
        info_span!(
            "moldette.resp.respond_batch",
            moldette.schema.name = schema.name(),
            moldette.schema.version = schema.version(),
            moldette.resp.ok = Empty,
            moldette.resp.missing_required = Empty,
            moldette.resp.emptied = Empty,
        )
    }

    pub(crate) fn record_counts(span: &Span, counts: &StatusCounts) {
        span.record("moldette.resp.ok", counts.ok());
        span.record("moldette.resp.missing_required", counts.missing_required());
        span.record("moldette.resp.emptied", counts.emptied());
    }
}

#[cfg(not(feature = "tracing"))]
mod disabled {
    use moldette_core::{OutputSchema, StatusCounts};

    pub(crate) struct Span;

    impl Span {
        pub(crate) fn in_scope<T>(&self, run: impl FnOnce() -> T) -> T {
            run()
        }
    }

    pub(crate) fn batch_span(_schema: &OutputSchema) -> Span {
        Span
    }

    pub(crate) fn record_counts(_span: &Span, _counts: &StatusCounts) {}
}
