//! The runner's copy of a crate whose modules rawr reads generic code from
//! (`crate_source`): the crate as it is, but each such module's items and
//! fields public (`polyglot::module_items_public`) and its manifest's path
//! dependencies absolute, so the copy builds where rawr writes it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, TableLike};

use crate::ImmutableString;

/// The dependency tables of a manifest or of one `target.<cfg>` table.
const DEPENDENCIES: [&str; 3] = ["dependencies", "build-dependencies", "dev-dependencies"];

/// The files of the copy in `directory` of the crate at `crate_directory`,
/// each with its text: every file of the crate but its `target`, each of
/// `modules` (paths from the crate's root module) opened. A file that is
/// no text is left out.
///
/// # Errors
///
/// When a directory of the crate can't be read.
pub(crate) fn files(
    crate_directory: &Path,
    directory: &Path,
    modules: &[&[ImmutableString]],
) -> std::io::Result<Vec<(PathBuf, String)>> {
    let opened = modules
        .iter()
        .flat_map(|path| module_files(crate_directory, path))
        .collect::<BTreeSet<_>>();
    let mut files = Vec::new();
    let mut pending = vec![crate_directory.to_path_buf()];
    while let Some(at) = pending.pop() {
        for entry in std::fs::read_dir(&at)? {
            let path = entry?.path();
            let Ok(relative) = path.strip_prefix(crate_directory) else {
                continue;
            };
            if path.is_dir() {
                if relative != Path::new("target") {
                    pending.push(path);
                }
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let text = if relative == Path::new("Cargo.toml") {
                manifest(&text, crate_directory)
            } else if opened.contains(relative) {
                opened_module(&text)
            } else {
                text
            };
            files.push((directory.join(relative), text));
        }
    }
    Ok(files)
}

/// The copy at `destination` of the package source tree at `source`, a git
/// checkout's root or a registry package: its directories its own, each of
/// `opened`, files under `source`, opened, and every other file a link to
/// the original, so the copy is the package as Cargo reads it, its
/// workspace's manifest included, and costs no copy of it.
///
/// # Errors
///
/// When a directory can't be read, or a link or file written.
pub(crate) fn linked(source: &Path, destination: &Path, opened: &[PathBuf]) -> std::io::Result<()> {
    let mut pending = vec![(source.to_path_buf(), destination.to_path_buf())];
    while let Some((from, to)) = pending.pop() {
        std::fs::create_dir_all(&to)?;
        for entry in std::fs::read_dir(&from)? {
            let entry = entry?;
            let name = entry.file_name();
            if name == "target" || name == ".git" {
                continue;
            }
            let (original, copy) = (entry.path(), to.join(&name));
            if entry.file_type()?.is_dir() {
                pending.push((original, copy));
            } else if opened.contains(&original) {
                let text = std::fs::read_to_string(&original)?;
                let opened = opened_module(&text);
                if std::fs::read_to_string(&copy).ok().as_deref() != Some(opened.as_str()) {
                    if copy.symlink_metadata().is_ok() {
                        std::fs::remove_file(&copy)?;
                    }
                    std::fs::write(&copy, opened)?;
                }
            } else if name == "Cargo.toml" {
                // Cargo names a package by its manifest's real path.
                let text = std::fs::read(&original)?;
                if std::fs::read(&copy).ok().as_deref() != Some(text.as_slice()) {
                    if copy.symlink_metadata().is_ok() {
                        std::fs::remove_file(&copy)?;
                    }
                    std::fs::write(&copy, text)?;
                }
            } else if copy.symlink_metadata().is_err() {
                link(&original, &copy)?;
            }
        }
    }
    Ok(())
}

/// A link at `copy` to the file `original`, or a copy of it where links
/// need privileges.
fn link(original: &Path, copy: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    return std::os::unix::fs::symlink(original, copy);
    #[cfg(not(unix))]
    return std::fs::copy(original, copy).map(|_| ());
}

/// The opened text of a crate module's file `text`: its items and fields
/// public, and its lints allowed, since the crate's own `deny`s (such as
/// `missing_docs`) hold for a path package and its new public items break
/// them. The allow shares the file's first line, so its lines stay.
fn opened_module(text: &str) -> String {
    let opened = super::super::polyglot::module_items_public(text);
    format!("#![allow(warnings, missing_docs)] {opened}")
}

/// The files the module at `path` may be in, relative to the crate's
/// directory: `src/a/b.rs` and `src/a/b/mod.rs`.
fn module_files(crate_directory: &Path, path: &[ImmutableString]) -> Vec<PathBuf> {
    let mut file = PathBuf::from("src");
    file.extend(path.iter().map(ImmutableString::as_str));
    [file.with_extension("rs"), file.join("mod.rs")]
        .into_iter()
        .filter(|file| crate_directory.join(file).is_file())
        .collect()
}

/// `text`, the crate's manifest, with each relative `path` of a dependency
/// joined to `crate_directory`, the directory it is relative to.
fn manifest(text: &str, crate_directory: &Path) -> String {
    let Ok(mut document) = text.parse::<DocumentMut>() else {
        return text.to_owned();
    };
    let absolute = |table: &mut dyn TableLike| {
        for key in DEPENDENCIES {
            let Some(dependencies) = table.get_mut(key).and_then(Item::as_table_like_mut) else {
                continue;
            };
            for (_, dependency) in dependencies.iter_mut() {
                let Some(dependency) = dependency.as_table_like_mut() else {
                    continue;
                };
                let Some(path) = dependency.get("path").and_then(Item::as_str) else {
                    continue;
                };
                if Path::new(path).is_absolute() {
                    continue;
                }
                let joined = crate_directory.join(path).display().to_string();
                dependency.insert("path", toml_edit::value(joined));
            }
        }
    };
    absolute(document.as_table_mut());
    if let Some(targets) = document.get_mut("target").and_then(Item::as_table_like_mut) {
        for (_, target) in targets.iter_mut() {
            if let Some(target) = target.as_table_like_mut() {
                absolute(target);
            }
        }
    }
    document.to_string()
}
