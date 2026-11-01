//! Structured generated-bridge request failures.

/// Failure before a generated manifest can be sealed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Node(#[source] Box<crate::node::Error>),
    #[error("{0}")]
    Native(#[from] crate::Error),
    /// rawr wrote a runner Cargo.toml it can't read back: a rawr bug.
    #[error("internal error: rawr wrote an invalid Cargo.toml for the program: {reason}. This is a rawr bug")]
    Manifest { reason: Box<str> },
    /// The generated Rust disagrees with what rawr read of the crates: a
    /// rawr bug.
    #[error("internal error: rawr's generated Rust disagrees with the crates on {field}. This is a rawr bug")]
    Drift { field: &'static str },
    #[error("{resource} exceeded limit {limit} with observed size {actual}")]
    Limit {
        resource: &'static str,
        limit: usize,
        actual: usize,
    },
    #[error("out of memory while generating the program's Rust")]
    Allocation,
    #[error(
        "internal error: rawr could not encode the program's generated Rust. This is a rawr bug"
    )]
    Encoding,
}
