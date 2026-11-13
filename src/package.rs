//! Static Rawr source and local-package graphs.

#[cfg(all(feature = "rust-import", not(target_arch = "wasm32")))]
pub mod called;
mod cell;
pub(crate) mod compile;
mod entry;
mod error;
pub(crate) mod graph;
#[cfg(feature = "rust-import")]
pub mod import;
#[cfg(all(feature = "rust-import", not(target_arch = "wasm32")))]
pub mod instances;
pub(crate) mod island;
mod manifest;
pub(crate) mod polyglot;
#[cfg(all(feature = "rust-import", not(target_arch = "wasm32")))]
mod rust;
pub mod snapshot;
pub mod source;
pub mod test;

pub use cell::{Cells, Counts};
#[cfg(all(feature = "rust-import", not(target_arch = "wasm32")))]
#[doc(hidden)]
pub use rust::{compare_reads, Copies};
/// The shadow crate's module that holds a marked `async fn main`'s entry.
pub(crate) const ENTRY_MODULE: &str = entry::MODULE;
pub use error::{Category, Error};
pub(crate) use graph::expansion_path;
pub use graph::{Graph, RustModule};
pub use island::Shape;
pub use manifest::script::{script_check, ScriptCheck};
pub use test::Test;
