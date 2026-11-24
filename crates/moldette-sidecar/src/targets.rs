use std::collections::HashMap;
use std::sync::{Arc, PoisonError, RwLock};

use hyper::header::HeaderValue;
use moldette::Pipeline;
use moldette::framing::Framer;

use crate::load::Built;
use crate::metrics::ConfigMetrics;

/// One configuration the sidecar runs. Processing is the library's; the
/// sidecar only routes requests to it (#122).
pub(crate) struct Target {
    pub(crate) pipeline: Arc<Pipeline>,
    pub(crate) framer: Framer,
    /// The digest of the configuration, sent with each response.
    pub(crate) digest: HeaderValue,
    pub(crate) metrics: ConfigMetrics,
}

impl Target {
    pub(crate) fn new(built: Built, framer: Framer, metrics: ConfigMetrics) -> Self {
        Self {
            pipeline: Arc::new(built.pipeline),
            framer,
            digest: built.digest,
            metrics,
        }
    }
}

/// The configurations: one without a name at `/v1/process`, or named ones at
/// `/v1/process/{name}`.
pub(crate) enum Targets {
    Unnamed(Arc<Target>),
    Named(Named),
}

impl Targets {
    /// The configuration of a request to `/v1/process` (`None`) or
    /// `/v1/process/{name}`.
    pub(crate) fn get(&self, name: Option<&str>) -> Option<Arc<Target>> {
        match (self, name) {
            (Self::Unnamed(target), None) => Some(Arc::clone(target)),
            (Self::Named(named), Some(name)) => named.get(name),
            _ => None,
        }
    }
}

/// Named configurations, which the configuration source replaces one at a
/// time; a request keeps the configuration it started with (#135).
pub(crate) struct Named(RwLock<HashMap<String, Arc<Target>>>);

impl Named {
    pub(crate) fn new(targets: HashMap<String, Arc<Target>>) -> Self {
        Self(RwLock::new(targets))
    }

    fn get(&self, name: &str) -> Option<Arc<Target>> {
        let targets = self.0.read().unwrap_or_else(PoisonError::into_inner);
        targets.get(name).cloned()
    }

    pub(crate) fn insert(&self, name: String, target: Arc<Target>) {
        let mut targets = self.0.write().unwrap_or_else(PoisonError::into_inner);
        targets.insert(name, target);
    }

    pub(crate) fn remove(&self, name: &str) {
        let mut targets = self.0.write().unwrap_or_else(PoisonError::into_inner);
        targets.remove(name);
    }
}
