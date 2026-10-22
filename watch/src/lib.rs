//! Exact filesystem-event transport shared by Rawr reload hosts.
//!
//! Native callbacks normalize filesystem events and advance the existing neutral
//! admission gate. The transport does not compile candidates or own host state.

#![forbid(unsafe_code)]

mod adapter;
mod input;
pub mod native;
mod notice;

pub use adapter::{Adapter, Control, Error, Notice, StagedInputs};

pub use notice::Notices;
