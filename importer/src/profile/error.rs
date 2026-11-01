//! Structured profile proof and lifecycle failures.

/// Failure to transform or execute one framework-neutral profile. `Node`
/// comes from the build, and `Limit` and `Allocation` are resource limits.
/// A condition only a rawr bug can cause panics naming its invariant, never
/// one of these (#538, D664).
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Node(#[source] Box<crate::node::Error>),
    #[error("profile {resource} exceeded limit {limit} with observed size {actual}")]
    Limit {
        resource: &'static str,
        limit: usize,
        actual: usize,
    },
    #[error("out of memory while reading the program's framework")]
    Allocation,
    /// A stored profile entry that doesn't decode under the inputs it is
    /// replayed for. The replay that can rebuild it does (the warm launch
    /// falls back to the full build); see [`Error::fresh`] for the others.
    #[error("a cached profile entry was written for other inputs")]
    Stale,
}

impl Error {
    /// Whether a cache entry this step read went stale before it was used
    /// (`crate::Error::is_stale`), so the run prepares again (#613).
    #[must_use]
    pub fn is_stale(&self) -> bool {
        match self {
            Self::Node(error) => error.is_stale(),
            Self::Stale => true,
            Self::Limit { .. } | Self::Allocation => false,
        }
    }

    /// A failure from an entry whose key holds every input it decodes
    /// under: the profile's version and identity, its parent and the
    /// checked program. A fresh build writes such an entry and a hit reads
    /// it back, so neither can be stale, and `Stale` panics here (#538).
    #[track_caller]
    pub(crate) fn fresh(self) -> Self {
        if matches!(self, Self::Stale) {
            panic!("rawr's cached profile entries decode under the inputs that key them");
        }
        self
    }
}
