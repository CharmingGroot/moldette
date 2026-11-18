#[cfg(not(feature = "tracing"))]
pub(crate) use disabled::*;
#[cfg(feature = "tracing")]
pub(crate) use enabled::*;

#[cfg(feature = "tracing")]
mod enabled {
    use moldette_core::SpecError;
    pub(crate) use tracing::Span;
    use tracing::{Level, event, info_span, trace_span};

    pub(crate) fn build_span<'a>(
        format: impl FnOnce() -> &'a str,
        fields: usize,
        bodies: usize,
    ) -> Span {
        info_span!(
            "moldette.parser.build",
            moldette.parser.format = format(),
            moldette.parser.fields = fields,
            moldette.parser.bodies = bodies,
        )
    }

    pub(crate) fn spec_error(err: &SpecError) {
        event!(
            name: "moldette.parser.spec_error",
            Level::DEBUG,
            moldette.error = %err,
        );
    }

    pub(crate) fn parse_span<'a>(format: impl FnOnce() -> &'a str) -> Span {
        trace_span!("moldette.parser.parse", moldette.parser.format = format())
    }

    pub(crate) fn body_span<'a>(path: &str, format: impl FnOnce() -> &'a str) -> Span {
        trace_span!(
            "moldette.parser.body",
            moldette.parser.path = path,
            moldette.parser.format = format(),
        )
    }

    pub(crate) fn malformed<'a>(format: impl FnOnce() -> &'a str) {
        event!(
            name: "moldette.parser.malformed",
            Level::TRACE,
            moldette.parser.format = format(),
        );
    }
}

#[cfg(not(feature = "tracing"))]
mod disabled {
    use moldette_core::SpecError;

    pub(crate) struct Span;

    impl Span {
        pub(crate) fn in_scope<T>(&self, run: impl FnOnce() -> T) -> T {
            run()
        }
    }

    pub(crate) fn build_span<'a>(
        _format: impl FnOnce() -> &'a str,
        _fields: usize,
        _bodies: usize,
    ) -> Span {
        Span
    }

    pub(crate) fn spec_error(_err: &SpecError) {}

    pub(crate) fn parse_span<'a>(_format: impl FnOnce() -> &'a str) -> Span {
        Span
    }

    pub(crate) fn body_span<'a>(_path: &str, _format: impl FnOnce() -> &'a str) -> Span {
        Span
    }

    pub(crate) fn malformed<'a>(_format: impl FnOnce() -> &'a str) {}
}
