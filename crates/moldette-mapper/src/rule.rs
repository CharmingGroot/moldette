/// Where one output field takes its value from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub(crate) target: String,
    from: String,
    fallback: Option<String>,
}

impl Rule {
    /// Reads the output field `target` from the source field `from`.
    pub fn new(target: impl Into<String>, from: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            from: from.into(),
            fallback: None,
        }
    }

    /// Reads the source field `from` when the primary source yields no value.
    pub fn fallback(mut self, from: impl Into<String>) -> Self {
        self.fallback = Some(from.into());
        self
    }

    pub(crate) fn sources(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.from.as_str()).chain(self.fallback.as_deref())
    }
}
