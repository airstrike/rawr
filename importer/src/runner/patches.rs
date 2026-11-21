//! The project's patches that a runner's settled lock leaves unused.
//!
//! The runner's manifest carries every `[patch]` the project's workspace
//! root declares (D609.1). Cargo settles the runner's lock over every member
//! of the runner's workspace with all of their features on: the copy of the
//! program's crate, the project's path packages placed under the runner's
//! root, and rawr's generated crates. A package that only a feature reaches,
//! such as a dependency of a git crate behind `iced/test`, is in that lock
//! though no build of the harness reached it, and its patch must be there
//! when cargo settles it. Only the settle knows which patches the runner's
//! workspace uses.
//!
//! Cargo lists each patch it settled without under `[[patch.unused]]`, and
//! warns about each one on every build (#1635). The runner leaves those out
//! of its manifest and its lock, which then agree for its `--locked` build.

#[cfg(test)]
mod tests;

use std::collections::BTreeSet;

use toml_edit::{DocumentMut, Item};

/// The runner's `manifest` and the `lock` cargo settled it to, without each
/// patch the lock lists as unused under a name no locked package has. A
/// name that another registry's patch uses keeps every patch of that name,
/// as Cargo keeps them. `None`, with both as they were, when the lock lists
/// no such patch.
pub(crate) fn prune(manifest: &mut DocumentMut, lock: &mut DocumentMut) -> Option<()> {
    let locked = lock
        .get("package")
        .and_then(Item::as_array_of_tables)
        .into_iter()
        .flatten()
        .filter_map(|package| package.get("name").and_then(Item::as_str))
        .collect::<BTreeSet<_>>();
    let unused = lock
        .get("patch")?
        .get("unused")?
        .as_array_of_tables()?
        .iter()
        .filter_map(|entry| entry.get("name").and_then(Item::as_str))
        .filter(|name| !locked.contains(name))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    if unused.is_empty() {
        return None;
    }
    if let Some(patch) = manifest.get_mut("patch").and_then(Item::as_table_like_mut) {
        let mut emptied = Vec::new();
        for (source, registry) in patch.iter_mut() {
            let Some(registry) = registry.as_table_like_mut() else {
                continue;
            };
            let left = registry
                .iter()
                .filter(|(key, item)| unused.contains(package(key, item)))
                .map(|(key, _)| key.to_owned())
                .collect::<Vec<_>>();
            for key in left {
                registry.remove(&key);
            }
            if registry.is_empty() {
                emptied.push(source.get().to_owned());
            }
        }
        for source in emptied {
            patch.remove(&source);
        }
        if patch.is_empty() {
            manifest.remove("patch");
        }
    }
    let patch = lock.get_mut("patch")?.as_table_like_mut()?;
    if let Some(entries) = patch
        .get_mut("unused")
        .and_then(Item::as_array_of_tables_mut)
    {
        entries.retain(|entry| {
            entry
                .get("name")
                .and_then(Item::as_str)
                .is_none_or(|name| !unused.contains(name))
        });
        if entries.is_empty() {
            patch.remove("unused");
        }
    }
    if patch.is_empty() {
        lock.remove("patch");
    }
    Some(())
}

/// The runner's `manifest` and settled `lock` as [`prune`] leaves them,
/// written out. `None` when they can't be read or nothing changes.
pub(crate) fn pruned(manifest: &str, lock: &str) -> Option<(String, String)> {
    let mut manifest = manifest.parse::<DocumentMut>().ok()?;
    let mut lock = lock.parse::<DocumentMut>().ok()?;
    prune(&mut manifest, &mut lock)?;
    Some((manifest.to_string(), lock.to_string()))
}

/// The package a patch entry names: its `package`, or its key.
fn package<'a>(key: &'a str, item: &'a Item) -> &'a str {
    item.as_table_like()
        .and_then(|table| table.get("package"))
        .and_then(Item::as_str)
        .unwrap_or(key)
}
