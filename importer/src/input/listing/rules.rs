//! Which of a package's files its manifest's `include` and `exclude` keep.
//!
//! The lists are read from the package's own manifest, or, when it inherits
//! one, from `[workspace.package]` of the workspace it belongs to: the one
//! `package.workspace` names, else the nearest manifest above declaring
//! `[workspace]`. The manifest and the lock beside it are always kept.

use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item};

use super::files::Error;
use super::pattern;

/// The selection a package's manifest makes from the files listed for it.
pub(super) struct Filter {
    root: PathBuf,
    selection: Selection,
}

/// The two lists are exclusive, as Cargo reads them: with `include`, only
/// files it names are kept and every directory is entered.
enum Selection {
    Exclude(pattern::Set),
    Include(pattern::Set),
}

/// The listing's mode, which decides whether hidden names are left out.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    /// Git lists the package; it leaves out what Git ignores.
    Git,
    /// The directory is walked; with no `include`, hidden names are left out.
    Walk,
}

/// The `include` and `exclude` of a package's manifest, as written.
pub(super) struct Lists {
    include: Vec<String>,
    exclude: Vec<String>,
}

impl Lists {
    /// The lists of the package whose manifest is `root/Cargo.toml`.
    pub(super) fn read(root: &Path) -> Result<Self, Error> {
        let manifest = root.join("Cargo.toml");
        let document = document(&manifest)?;
        let package = document.get("package");
        let mut inherited = None;
        let mut list = |name: &str| -> Result<Vec<String>, Error> {
            match package.and_then(|package| package.get(name)) {
                None => Ok(Vec::new()),
                Some(item) if item.get("workspace").and_then(Item::as_bool) == Some(true) => {
                    if inherited.is_none() {
                        inherited = Some(workspace(root, package)?);
                    }
                    let (path, workspace) = inherited
                        .as_ref()
                        .expect("the inherited workspace was just read");
                    let declared = workspace
                        .get("workspace")
                        .and_then(|workspace| workspace.get("package"))
                        .and_then(|package| package.get(name));
                    match declared {
                        Some(item) => strings(item, path, name),
                        None => Err(Error::manifest(
                            path,
                            format!("`workspace.package.{name}` is not declared"),
                        )),
                    }
                }
                Some(item) => strings(item, &manifest, name),
            }
        };
        let include = list("include")?;
        let exclude = list("exclude")?;
        Ok(Self { include, exclude })
    }

    /// Whether `include` decides the listing, which Git then never guides.
    pub(super) fn includes(&self) -> bool {
        !self.include.is_empty()
    }

    /// The filter these lists make for a listing in `mode` of the package
    /// at `root`.
    pub(super) fn filter(&self, root: &Path, mode: Mode) -> Result<Filter, Error> {
        let invalid = |error: pattern::Invalid| Error::manifest(&root.join("Cargo.toml"), error);
        let selection = if self.includes() {
            Selection::Include(
                pattern::Set::new(self.include.iter().map(String::as_str)).map_err(invalid)?,
            )
        } else {
            let hidden = (mode == Mode::Walk).then_some(".*");
            let rules = hidden
                .into_iter()
                .chain(self.exclude.iter().map(String::as_str));
            Selection::Exclude(pattern::Set::new(rules).map_err(invalid)?)
        };
        Ok(Filter {
            root: root.to_path_buf(),
            selection,
        })
    }
}

impl Filter {
    /// Whether the listing keeps `path`, a path under the package root.
    pub(super) fn keeps(&self, path: &Path, directory: bool) -> bool {
        let Ok(relative) = path.strip_prefix(&self.root) else {
            return false;
        };
        if relative == Path::new("Cargo.toml") || relative == Path::new("Cargo.lock") {
            return true;
        }
        match &self.selection {
            Selection::Exclude(rules) => !rules.ignores(relative, directory),
            Selection::Include(rules) => directory || rules.ignores(relative, false),
        }
    }
}

fn document(manifest: &Path) -> Result<DocumentMut, Error> {
    let text = std::fs::read_to_string(manifest).map_err(|source| Error::Io {
        operation: "read package manifest",
        path: manifest.to_path_buf(),
        source,
    })?;
    text.parse()
        .map_err(|error: toml_edit::TomlError| Error::manifest(manifest, error))
}

/// The manifest and document of the workspace the package at `root`
/// inherits from.
fn workspace(root: &Path, package: Option<&Item>) -> Result<(PathBuf, DocumentMut), Error> {
    let named = package.and_then(|package| package.get("workspace"));
    if let Some(path) = named.and_then(Item::as_str) {
        let manifest = root.join(path).join("Cargo.toml");
        let document = document(&manifest)?;
        return Ok((manifest, document));
    }
    for directory in root.ancestors().skip(1) {
        let manifest = directory.join("Cargo.toml");
        if !manifest.is_file() {
            continue;
        }
        let document = document(&manifest)?;
        if document.contains_key("workspace") {
            return Ok((manifest, document));
        }
    }
    Err(Error::manifest(
        &root.join("Cargo.toml"),
        "inherits from a workspace, but none is above it",
    ))
}

fn strings(item: &Item, manifest: &Path, name: &str) -> Result<Vec<String>, Error> {
    let not_a_list = || Error::manifest(manifest, format!("`{name}` is not a list of strings"));
    let array = item.as_array().ok_or_else(not_a_list)?;
    array
        .iter()
        .map(|value| value.as_str().map(str::to_owned).ok_or_else(not_a_list))
        .collect()
}
