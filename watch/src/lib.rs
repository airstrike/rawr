//! Exact filesystem-event transport shared by Rawr reload hosts.
//!
//! Native callbacks send raw filesystem events to the owning thread, which
//! normalizes them and advances its neutral admission gate. The transport
//! does not compile candidates or own host state.

#![forbid(unsafe_code)]

mod adapter;
#[cfg(unix)]
mod exit;
mod input;
pub mod native;

pub use adapter::{derived_roots, Adapter, Error, Notice, StagedInputs, Waker};
