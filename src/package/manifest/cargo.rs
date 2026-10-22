//! The app's own Cargo package manifest in a polyglot `src/`.
//!
//! An app is one ordinary Cargo crate. When the project's `Cargo.toml` has a
//! `[package]`, its `[dependencies]` are the app's Rust dependencies (no
//! `[rust.dependencies]` duplicate), and its `.rs` modules are importable as
//! that crate.

use std::path::{Path, PathBuf};

use toml_edit::Document;

use super::{byte_span, item_span, rust, Error};
use crate::{Position, Span};

/// A validated Cargo package manifest at a Rawr project root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Cargo {
    pub(crate) path: PathBuf,
    pub(crate) text: String,
    /// `[package] name`.
    pub(crate) package: String,
    /// The library crate name Rust code and Rawr name it by.
    pub(crate) crate_name: String,
    pub(crate) version: Option<String>,
    pub(crate) edition: Option<String>,
    /// `[dependencies]`, normalized to Rawr's Rust dependency form.
    pub(crate) dependencies: Box<[rust::Dependency]>,
}

impl Cargo {
    /// Parse `Cargo.toml`: `None` when it is only a workspace (no `[package]`).
    pub(crate) fn parse(path: PathBuf, text: String, root: &Path) -> Result<Option<Self>, Error> {
        let document = Document::parse(text.as_str()).map_err(|error| Error {
            message: error.to_string().into(),
            span: error
                .span()
                .map(|range| byte_span(&text, range))
                .unwrap_or_else(|| Span::new(Position::START, Position::START)),
        })?;
        let table = document.as_table();
        let Some(package) = table.get("package").and_then(|item| item.as_table_like()) else {
            return Ok(None);
        };
        let name = package
            .get("name")
            .and_then(|item| item.as_str())
            .ok_or_else(|| Error {
                message: "Cargo `[package]` requires a string `name`".into(),
                span: table.get("package").map_or_else(
                    || Span::new(Position::START, Position::START),
                    |item| item_span(&text, item),
                ),
            })?
            .to_owned();
        let string = |key: &str| {
            package
                .get(key)
                .and_then(|item| item.as_str())
                .map(str::to_owned)
        };
        let version = string("version");
        let edition = string("edition");
        let crate_name = table
            .get("lib")
            .and_then(|item| item.get("name"))
            .and_then(|item| item.as_str())
            .map_or_else(|| name.replace('-', "_"), str::to_owned);
        let dependencies = rust::cargo_dependencies(&text, root, table.get("dependencies"))?;
        Ok(Some(Self {
            path,
            package: name,
            crate_name,
            version,
            edition,
            dependencies,
            text,
        }))
    }
}
