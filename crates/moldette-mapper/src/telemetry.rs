#[cfg(not(feature = "tracing"))]
pub(crate) use disabled::*;
#[cfg(feature = "tracing")]
pub(crate) use enabled::*;

#[cfg(feature = "tracing")]
mod enabled {
    use moldette_core::{OutputSchema, SpecError};
    pub(crate) use tracing::Span;
    use tracing::{Level, event, info_span, trace_span};

    pub(crate) fn build_span(schema: &OutputSchema, rules: usize, explicit_rules: bool) -> Span {
        info_span!(
            "moldette.mapper.build",
            moldette.schema.name = schema.name(),
            moldette.schema.version = schema.version(),
            moldette.mapper.rules = rules,
            moldette.mapper.explicit_rules = explicit_rules,
        )
    }

    pub(crate) fn spec_error(err: &SpecError) {
        event!(
            name: "moldette.mapper.spec_error",
            Level::DEBUG,
            moldette.error = %err,
        );
    }

    pub(crate) fn map_span(schema: &OutputSchema) -> Span {
        trace_span!(
            "moldette.mapper.map",
            moldette.schema.name = schema.name(),
            moldette.schema.version = schema.version(),
        )
    }

    pub(crate) fn emptied(field: &str) {
        event!(
            name: "moldette.mapper.emptied",
            Level::TRACE,
            moldette.field = field,
        );
    }
}

#[cfg(not(feature = "tracing"))]
mod disabled {
    use moldette_core::{OutputSchema, SpecError};

    pub(crate) struct Span;

    impl Span {
        pub(crate) fn in_scope<T>(&self, run: impl FnOnce() -> T) -> T {
            run()
        }
    }

    pub(crate) fn build_span(_schema: &OutputSchema, _rules: usize, _explicit_rules: bool) -> Span {
        Span
    }

    pub(crate) fn spec_error(_err: &SpecError) {}

    pub(crate) fn map_span(_schema: &OutputSchema) -> Span {
        Span
    }

    pub(crate) fn emptied(_field: &str) {}
}
