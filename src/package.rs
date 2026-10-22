//! Static Rawr source and local-package graphs.

mod cell;
pub(crate) mod compile;
mod error;
pub(crate) mod graph;
#[cfg(feature = "rust-import")]
pub mod import;
mod manifest;
mod polyglot;
#[cfg(feature = "rust-import")]
mod rust;
#[cfg(feature = "rust-import")]
pub mod snapshot;
pub mod source;

pub use cell::{Cells, Counts};
pub use error::{Category, Error};
pub use graph::{Graph, RustModule};
