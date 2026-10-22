//! Pinned Cargo scalar conversions; no source or membership authority.
//!
//! Borrowed transforms require the caller to admit input and retained slots.
//! The complete requirement transform separately admits its owned comparator
//! storage through Session. No transform grants unchecked spelling parsed status.
//! Source correspondence and work bounds are recorded in `scalar/PROVENANCE.md`.

mod name;
mod partial;
pub(super) mod requirement;
mod rust;
mod version;

#[cfg(test)]
mod tests;

pub(super) use name::{Kind, Name, Rejection as NameRejection};
pub(super) use partial::Partial;
pub(super) use rust::Rust;
pub(super) use version::{compare_build, compare_pre, Version};

/// Fixed semantic rejection; the enclosing visitor supplies its key and span.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum Error {
    #[error("invalid semantic version: {0:?}")]
    Version(version::Rejection),
    #[error("invalid partial version: {0:?}")]
    Partial(partial::Rejection),
    #[error("invalid Rust version: {0:?}")]
    Rust(rust::Rejection),
    #[error("invalid {kind:?} name: {reason:?}")]
    Name { kind: Kind, reason: name::Rejection },
}
