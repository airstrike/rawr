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
                super::super::polyglot::module_items_public(&text)
            } else {
                text
            };
            files.push((directory.join(relative), text));
        }
    }
    Ok(files)
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
