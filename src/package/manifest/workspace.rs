//! What a package inherits from its Cargo workspace, as Cargo resolves it.
//!
//! A member's `foo = { workspace = true }` takes `foo` from the workspace
//! root's `[workspace.dependencies]`, and `edition.workspace = true` takes
//! the edition from `[workspace.package]`. Cargo is the source of truth:
//! Rawr asks `cargo metadata --no-deps` once and keeps the answer while
//! neither manifest changes.

use std::cell::RefCell;
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
}

struct Entry {
    manifest: PathBuf,
    text: String,
    root: PathBuf,
    root_text: String,
    inherited: Inherited,
}

thread_local! {
    /// One cell per member manifest. It is fresh while the member's text
    /// and the workspace root manifest's text are the ones Cargo read.
    static CACHE: RefCell<Vec<Entry>> = const { RefCell::new(Vec::new()) };
}

/// The fields `manifest` (whose text is `text`) inherits from its workspace.
pub(crate) fn inherited(manifest: &Path, text: &str) -> Result<Inherited, String> {
    let fresh = CACHE.with_borrow(|cache| {
        cache
            .iter()
            .find(|entry| entry.manifest == manifest && entry.text == text)
            .filter(|entry| {
                std::fs::read_to_string(&entry.root).ok().as_deref()
                    == Some(entry.root_text.as_str())
            })
            .map(|entry| entry.inherited.clone())
    });
    if let Some(inherited) = fresh {
        return Ok(inherited);
    }
    let (root, inherited) = metadata(manifest)?;
    let root_text = std::fs::read_to_string(&root).unwrap_or_default();
    CACHE.with_borrow_mut(|cache| {
        cache.retain(|entry| entry.manifest != manifest);
        cache.push(Entry {
            manifest: manifest.to_path_buf(),
            text: text.to_owned(),
            root,
            root_text,
            inherited: inherited.clone(),
        });
    });
    Ok(inherited)
}

/// Ask Cargo for the package `manifest` describes: the workspace root
/// manifest and the package's resolved fields.
fn metadata(manifest: &Path) -> Result<(PathBuf, Inherited), String> {
    let cargo = std::env::var_os("CARGO")
        .filter(|cargo| !cargo.is_empty())
        .unwrap_or_else(|| "cargo".into());
    let output = std::process::Command::new(&cargo)
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--offline",
        ])
        .arg("--manifest-path")
        .arg(manifest)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| format!("could not run `cargo metadata`: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("`cargo metadata` failed: {}", stderr.trim()));
    }
    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("`cargo metadata` printed invalid JSON: {error}"))?;
    let root = json["workspace_root"]
        .as_str()
        .map(|root| Path::new(root).join("Cargo.toml"))
        .ok_or("`cargo metadata` names no workspace root")?;
    let canonical = std::fs::canonicalize(manifest).unwrap_or_else(|_| manifest.to_path_buf());
    let package = json["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|package| {
            package["manifest_path"]
                .as_str()
                .is_some_and(|path| std::fs::canonicalize(path).ok().as_ref() == Some(&canonical))
        })
        .ok_or_else(|| format!("`cargo metadata` does not list {}", manifest.display()))?;
    let string = |value: &serde_json::Value| value.as_str().map(str::to_owned);
    let mut inherited = Inherited {
        version: string(&package["version"]),
        edition: string(&package["edition"]),
        dependencies: BTreeMap::new(),
    };
    for dependency in package["dependencies"].as_array().into_iter().flatten() {
        // Normal dependencies for every target, as Rawr reads them.
        if !dependency["kind"].is_null() || !dependency["target"].is_null() {
            continue;
        }
        if dependency["optional"].as_bool() == Some(true) {
            continue;
        }
        let Some(name) = dependency["name"].as_str() else {
            continue;
        };
        let key = dependency["rename"].as_str().unwrap_or(name);
        let source = if let Some(path) = dependency["path"].as_str() {
            Source::Path(PathBuf::from(path))
        } else if let Some(git) = dependency["source"]
            .as_str()
            .and_then(|source| source.strip_prefix("git+"))
        {
            let (url, query) = git.split_once('?').unwrap_or((git, ""));
            let rev = query
                .split('&')
                .find_map(|pair| pair.strip_prefix("rev="))
                .ok_or_else(|| format!("Rust dependency `{key}` with `git` also requires `rev`"))?;
            Source::Git {
                url: url.to_owned(),
                rev: rev.to_owned(),
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
        inherited.dependencies.insert(
            key.to_owned(),
            Dependency {
                package: (alias != name).then(|| name.to_owned()),
                alias: alias.into(),
                source,
                features,
                default_features: dependency["uses_default_features"].as_bool() != Some(false),
                span: Span::new(Position::START, Position::START),
            },
        );
    }
    Ok((root, inherited))
}
