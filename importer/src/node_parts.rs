//! What a model value names of the host's node store: the chain a value
//! keeps when the store evaluated it, and the store's failure.
//!
//! A contract, a graph or a profile the host's node store evaluated
//! carries its chain, so later joins can name their parents, and a model
//! error can carry the store's failure. A runner on wasm has no node store,
//! so there both types have no values: every `Option<Chain>` is `None`, and
//! no error holds a node failure.

#[cfg(not(target_arch = "wasm32"))]
pub use crate::node::{Chain, Error};

/// No value has a node chain on wasm.
#[cfg(target_arch = "wasm32")]
#[derive(Clone)]
pub enum Chain {}

/// No node store fails on wasm.
#[cfg(target_arch = "wasm32")]
#[derive(Debug)]
pub enum Error {}

#[cfg(target_arch = "wasm32")]
impl Error {
    /// Whether this failure is an entry that went stale after it was read.
    #[must_use]
    pub fn is_stale(&self) -> bool {
        match *self {}
    }

    /// The invariant this failure breaks, when only a rawr bug can cause it.
    pub(crate) fn invariant(&self) -> Option<String> {
        match *self {}
    }
}

#[cfg(target_arch = "wasm32")]
impl std::fmt::Display for Error {
    fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {}
    }
}

#[cfg(target_arch = "wasm32")]
impl std::error::Error for Error {}
