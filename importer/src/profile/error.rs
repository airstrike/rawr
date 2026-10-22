//! Structured profile proof and lifecycle failures.

/// Failure to transform or execute one framework-neutral profile.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("content-addressed profile node failed: {0}")]
    Node(#[source] Box<crate::node::Error>),
    #[error("profile identity is empty or exceeds the bounded string limit")]
    Identity,
    #[error("profile version must be nonzero")]
    Version,
    #[error("profile definition identity or version changed during transformation")]
    DefinitionDrift,
    #[error("profile resolution universe could not be authenticated")]
    ResolutionUniverse,
    #[error("profile selected evidence outside its exact imported graph or contract")]
    ForeignEvidence,
    #[error("profile evidence or hooks are duplicate or not canonical")]
    Canonical,
    #[error("profile {resource} exceeded limit {limit} with observed size {actual}")]
    Limit {
        resource: &'static str,
        limit: usize,
        actual: usize,
    },
    #[error("cannot allocate profile proof storage")]
    Allocation,
    #[error("cannot encode canonical profile evidence")]
    Encoding,
    #[error("profile activation proof is stale for the current route")]
    Stale,
    #[error("profile runtime transition failed: {0}")]
    Runtime(Box<str>),
}
