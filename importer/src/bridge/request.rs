//! Structured generated-bridge request failures.

/// Failure before a generated manifest can be sealed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("generated overlay node failed")]
    Node(#[source] Box<crate::node::Error>),
    #[error("normal-build proof changed during generation: {0}")]
    Native(#[from] crate::Error),
    #[error("generated runner manifest is invalid: {reason}")]
    Manifest { reason: Box<str> },
    #[error("generated bridge drifted in {field}")]
    Drift { field: &'static str },
    #[error("{resource} exceeded limit {limit} with observed size {actual}")]
    Limit {
        resource: &'static str,
        limit: usize,
        actual: usize,
    },
    #[error("cannot allocate generated bridge bytes")]
    Allocation,
    #[error("cannot encode generated bridge evidence")]
    Encoding,
}
