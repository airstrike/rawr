//! What a package inherits from its Cargo workspace, as Cargo resolves it.
//!
//! A member's `foo = { workspace = true }` takes `foo` from the workspace
//! root's `[workspace.dependencies]`, and `edition.workspace = true` takes
//! the edition from `[workspace.package]`. Cargo is the source of truth:
//! Rawr asks `cargo metadata --no-deps` once and keeps the answer while
//! neither manifest changes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::rust::{Dependency, Source};
use crate::{Position, Span};

/// A package's inherited fields, resolved by Cargo.
#[derive(Clone, Debug, Default)]
pub(crate) struct Inherited {
    pub(crate) version: Option<String>,
    pub(crate) edition: Option<String>,
    /// Normal dependencies by their `[dependencies]` key.
    pub(crate) dependencies: BTreeMap<String, Dependency>,
    /// Build dependencies by their `[build-dependencies]` key.
    pub(crate) build_dependencies: BTreeMap<String, Dependency>,
}

/// The fields `manifest` inherits from its workspace, from Cargo's
/// description of it ([`crate::project::cargo::Workspace`]).
pub(crate) fn inherited(manifest: &Path) -> Result<Inherited, String> {
    let workspace = crate::project::cargo::Workspace::of(manifest)
        .map_err(|error| format!("`cargo metadata` failed: {error}"))?;
    let package = workspace
        .package_json(manifest)
        .ok_or_else(|| format!("`cargo metadata` does not list {}", manifest.display()))?;
    let string = |value: &serde_json::Value| value.as_str().map(str::to_owned);
    let mut inherited = Inherited {
        version: string(&package["version"]),
        edition: string(&package["edition"]),
        dependencies: BTreeMap::new(),
        build_dependencies: BTreeMap::new(),
    };
    for dependency in package["dependencies"].as_array().into_iter().flatten() {
        // Normal and build dependencies, optional ones included, as Rawr
        // reads them. A key declared for every target and under a
        // `[target.'cfg(..)']` table is the former's.
        let entries = match dependency["kind"].as_str() {
            None => &mut inherited.dependencies,
            Some("build") => &mut inherited.build_dependencies,
            Some(_) => continue,
        };
        let targeted = !dependency["target"].is_null();
        let Some(name) = dependency["name"].as_str() else {
            continue;
        };
        let key = dependency["rename"].as_str().unwrap_or(name);
        if targeted && entries.contains_key(key) {
            continue;
        }
        let source = if let Some(path) = dependency["path"].as_str() {
            Source::Path(PathBuf::from(path))
        } else if let Some((url, reference)) = dependency["source"]
            .as_str()
            .and_then(super::rust::GitReference::parse_source)
        {
            Source::Git {
                url: url.to_owned(),
                reference,
                // Cargo prints `*` for a Git dependency with no `version`.
                version: dependency["req"]
                    .as_str()
                    .filter(|req| *req != "*")
                    .map(str::to_owned),
            }
        } else {
            Source::Registry {
                version: dependency["req"].as_str().unwrap_or("*").to_owned(),
            }
        };
        let alias = key.replace('-', "_");
        let features = dependency["features"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|feature| feature.as_str().map(str::to_owned))
            .collect();
        entries.insert(
            key.to_owned(),
            Dependency {
                key: key.to_owned(),
                package: (alias != name).then(|| name.to_owned()),
                alias: alias.into(),
                source,
                features,
                default_features: dependency["uses_default_features"].as_bool() != Some(false),
                optional: dependency["optional"].as_bool() == Some(true),
                span: Span::new(Position::START, Position::START),
            },
        );
    }
    Ok(inherited)
}
