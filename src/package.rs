//! Static Rawr source and local-package graphs.

mod cell;
pub(crate) mod compile;
mod entry;
mod error;
pub(crate) mod graph;
#[cfg(feature = "rust-import")]
pub mod import;
pub(crate) mod island;
mod manifest;
pub(crate) mod polyglot;
#[cfg(feature = "rust-import")]
mod rust;
pub mod snapshot;
pub mod source;

pub use cell::{Cells, Counts};
/// The shadow crate's module that holds a marked `async fn main`'s entry.
pub(crate) const ENTRY_MODULE: &str = entry::MODULE;
pub use error::{Category, Error};
pub use graph::{Graph, RustModule};
pub use island::Shape;
pub use manifest::script::{script_check, ScriptCheck};
