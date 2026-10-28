//! The crate's own Cargo package manifest.
//!
//! A Rawr program is one ordinary Cargo crate. When its `Cargo.toml` has a
//! `[package]`, its `[dependencies]` are the program's Rust dependencies,
//! and its native modules are importable as that crate.

use std::path::{Path, PathBuf};

use toml_edit::{Document, Item};

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
    ///
    /// With `ask_cargo`, Cargo resolves what the package inherits from its
    /// workspace. Without it (a runner's snapshot) an inherited dependency
    /// keeps only its alias.
    pub(crate) fn parse(
        path: PathBuf,
        text: String,
        root: &Path,
        ask_cargo: bool,
    ) -> Result<Option<Self>, Error> {
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
        // `foo.workspace = true` inherits `foo` from the workspace root.
        let inherits = |item: &Item| {
            item.as_table_like()
                .and_then(|table| table.get("workspace"))
                .and_then(Item::as_bool)
                == Some(true)
        };
        let inheriting = ["version", "edition"]
            .iter()
            .any(|key| package.get(key).is_some_and(inherits))
            || table
                .get("dependencies")
                .and_then(Item::as_table_like)
                .is_some_and(|dependencies| dependencies.iter().any(|(_, item)| inherits(item)));
        let inherited = if inheriting && ask_cargo {
            Some(
                super::workspace::inherited(&path, &text).map_err(|message| Error {
                    message: message.into(),
                    span: table.get("package").map_or_else(
                        || Span::new(Position::START, Position::START),
                        |item| item_span(&text, item),
                    ),
                })?,
            )
        } else {
            None
        };
        let string = |key: &str| match package.get(key) {
            Some(item) if inherits(item) => inherited.as_ref().and_then(|inherited| match key {
                "version" => inherited.version.clone(),
                _ => inherited.edition.clone(),
            }),
            item => item.and_then(|item| item.as_str()).map(str::to_owned),
        };
        let version = string("version");
        let edition = string("edition");
        let crate_name = table
            .get("lib")
            .and_then(|item| item.get("name"))
            .and_then(|item| item.as_str())
            .map_or_else(|| name.replace('-', "_"), str::to_owned);
        let dependencies = dependencies(&text, root, table, inherited.as_ref())?;
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

/// A manifest's `[dependencies]`, then its
/// `[target.'cfg(..)'.dependencies]` whose `cfg` holds here, as Cargo builds
/// them for this target. `table` is the manifest parsed from `text`.
pub(super) fn dependencies(
    text: &str,
    root: &Path,
    table: &toml_edit::Table,
    inherited: Option<&super::workspace::Inherited>,
) -> Result<Box<[rust::Dependency]>, Error> {
    let mut dependencies =
        rust::cargo_dependencies(text, root, table.get("dependencies"), inherited)?.into_vec();
    for (key, target) in table
        .get("target")
        .and_then(Item::as_table_like)
        .into_iter()
        .flat_map(|targets| targets.iter())
    {
        if crate::resolve::cfg_text_holds(key) != Some(true) {
            continue;
        }
        let more = rust::cargo_dependencies(
            text,
            root,
            target
                .as_table_like()
                .and_then(|target| target.get("dependencies")),
            inherited,
        )?;
        for dependency in more {
            if !dependencies
                .iter()
                .any(|known| known.alias == dependency.alias)
            {
                dependencies.push(dependency);
            }
        }
    }
    Ok(dependencies.into_boxed_slice())
}

#[cfg(test)]
mod tests {
    use super::Cargo;
    use std::path::{Path, PathBuf};

    #[test]
    fn target_dependencies_join_when_their_cfg_holds_here() {
        let text = r#"
[package]
name = "tour"
version = "0.1.0"

[dependencies]
iced = "0.13"

[target."cfg(not(target_arch = \"wasm32\"))".dependencies]
tracing-subscriber = "0.3"

[target.'cfg(any(unix, windows))'.dependencies]
open = "5"

[target."cfg(target_arch = \"wasm32\")".dependencies]
console_log = "1.0"
"#;
        let cargo = Cargo::parse(
            PathBuf::from("/tour/Cargo.toml"),
            text.to_owned(),
            Path::new("/tour"),
            false,
        )
        .unwrap()
        .unwrap();
        let aliases = cargo
            .dependencies
            .iter()
            .map(|dependency| dependency.alias.as_str())
            .collect::<Vec<_>>();
        assert_eq!(aliases, ["iced", "tracing_subscriber", "open"]);
        assert_eq!(
            crate::resolve::cfg_text_holds("x86_64-pc-windows-msvc"),
            None
        );
    }
}
