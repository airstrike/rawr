// Pinned toml_parser 1.1.2+spec-1.1.0; mapping and fixed cfgs: toml/provenance.json.
//! Private pinned TOML syntax APIs for the contained document computation.
//!
//! Configuration is fixed to std and alloc. Debug, unsafe, SIMD and ambient
//! capabilities are unavailable. Raw syntax callbacks do not prove admission,
//! complete input, document validity, or current package authority.
//!
//! Source and dependency lineage is retained in `toml/provenance.json`.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]
#![warn(clippy::std_instead_of_core)]
#![warn(clippy::std_instead_of_alloc)]
#![warn(clippy::print_stderr)]
#![warn(clippy::print_stdout)]

extern crate alloc;

#[macro_use]
mod macros;

#[cfg(any())]
pub(crate) mod debug;
pub mod document;
mod error;
mod source;

pub(crate) mod decoder;
pub(crate) mod lexer;
pub(crate) mod parser;

pub(crate) use error::ErrorSink;
pub(crate) use error::Expected;
pub(crate) use error::ParseError;
pub(crate) use source::Raw;
pub(crate) use source::Source;
pub(crate) use source::Span;

#[cfg(test)]
mod tests;
