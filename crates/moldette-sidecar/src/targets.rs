use std::collections::HashMap;
use std::sync::Arc;

use hyper::header::HeaderValue;
use moldette::Pipeline;
use moldette::framing::Framer;

/// One configuration the sidecar runs. Processing is the library's; the
/// sidecar only routes requests to it (#122).
pub(crate) struct Target {
    pub(crate) pipeline: Arc<Pipeline>,
    pub(crate) framer: Framer,
    /// The digest of the configuration, sent with each response.
    pub(crate) digest: HeaderValue,
}

/// The configurations: one without a name at `/v1/process`, or named ones at
/// `/v1/process/{name}`.
pub(crate) enum Targets {
    Unnamed(Arc<Target>),
    Named(HashMap<String, Arc<Target>>),
}

impl Targets {
    /// The configuration of a request to `/v1/process` (`None`) or
    /// `/v1/process/{name}`.
    pub(crate) fn get(&self, name: Option<&str>) -> Option<&Arc<Target>> {
        match (self, name) {
            (Self::Unnamed(target), None) => Some(target),
            (Self::Named(targets), Some(name)) => targets.get(name),
            _ => None,
        }
    }
}
