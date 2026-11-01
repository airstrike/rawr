//! Structured profile proof and lifecycle failures.

/// Failure to transform or execute one framework-neutral profile. Only
/// `Node` can come from the program; `Limit` and `Allocation` are resource
/// limits, and the rest are rawr bugs, said as rustc's ICEs say one.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Node(#[source] Box<crate::node::Error>),
    #[error(
        "internal error: rawr gave a framework an empty or oversized name. This is a rawr bug"
    )]
    Identity,
    #[error("internal error: rawr gave a framework version 0. This is a rawr bug")]
    Version,
    #[error(
        "internal error: rawr's framework support changed while the program was built. \
         This is a rawr bug"
    )]
    DefinitionDrift,
    #[error(
        "internal error: rawr could not match the framework to the program's dependencies. \
         This is a rawr bug"
    )]
    ResolutionUniverse,
    #[error(
        "internal error: rawr's framework support names an item outside the program's \
         dependencies. This is a rawr bug"
    )]
    ForeignEvidence,
    #[error(
        "internal error: rawr's framework support lists an item twice or out of order. \
         This is a rawr bug"
    )]
    Canonical,
    #[error("profile {resource} exceeded limit {limit} with observed size {actual}")]
    Limit {
        resource: &'static str,
        limit: usize,
        actual: usize,
    },
    #[error("out of memory while reading the program's framework")]
    Allocation,
    #[error("internal error: rawr could not encode the program's framework. This is a rawr bug")]
    Encoding,
    #[error(
        "internal error: rawr's framework support is out of date for this build. \
         This is a rawr bug"
    )]
    Stale,
}
