//! What `rawr-import-driver --expand` reads and writes (#593), shared by
//! the library, which writes the request, and the driver.

use serde::{Deserialize, Serialize};

/// The small crate rustc expands: its source, and the crates its derives
/// come from, at the artifacts the project's check build made.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// The driver protocol the request is for.
    pub protocol: u32,
    /// The program crate's edition.
    pub edition: String,
    /// The target the artifacts were built for.
    pub selected_target: String,
    /// Where the derive crates' own dependencies are, as `-Ldependency`.
    pub dependency_directories: Vec<String>,
    /// Each crate the source names, by the alias the program uses.
    pub externs: Vec<Extern>,
    /// The source file to expand.
    pub source: String,
}

/// One `--extern alias=path`.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Extern {
    /// The name the program gives the crate, as `serde`.
    pub alias: String,
    /// The crate's `.rmeta` or `.rlib`.
    pub path: String,
}

/// The items one derive produced, at the derive's name in the source.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Expansion {
    /// The derive macro's name, as `Serialize`.
    pub derive: String,
    /// The byte offset in the source of the derive's name in its
    /// `#[derive(..)]`.
    pub at: u32,
    /// Each produced item, as rustc prints it.
    pub items: Vec<String>,
}

/// What `--expand` writes.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Expanded {
    /// The protocol the driver speaks.
    pub protocol: u32,
    /// Each derive's expansion, in source order.
    pub expansions: Vec<Expansion>,
}
