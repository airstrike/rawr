//! Cargo's `[env]` table, merged and resolved as the pinned Cargo gives it to
//! the processes it runs.
//!
//! Pinned Cargo `59800466c5c41c444d264b1010b4d57e85a7117f`:
//! `util/context/schema.rs` (`EnvConfigValue`), `util/context/mod.rs`
//! (`GlobalContext::env_config`, `walk_tree`, `merge_cli_args`),
//! `util/context/config_value.rs` (`ConfigValue::merge_helper`) and
//! `core/compiler/fingerprint/mod.rs` (`LocalFingerprint::from_env`).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use toml_edit::Item;

use crate::{Error, Result};

/// Variables Cargo refuses in `[env]`.
const DISALLOWED: [&str; 3] = ["CARGO_HOME", "RUSTUP_HOME", "RUSTUP_TOOLCHAIN"];

/// The merged `[env]` table of one config discovery, each value resolved.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Table {
    /// Sorted by key.
    entries: Box<[Entry]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Entry {
    key: Box<str>,
    /// `None` when a `relative` path is not Unicode: Cargo's fingerprint then
    /// records the variable as unset.
    value: Option<Box<str>>,
    force: bool,
}

/// Where a config file sits in Cargo's merge order. Higher wins.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum Rank {
    /// `$CARGO_HOME/config.toml`, walked last.
    Home,
    /// `<dir>/.cargo/config.toml` above the project; the depth of `<dir>`.
    Hierarchy(usize),
    /// A `--config <file>` argument; later arguments win.
    Cli(usize),
}

/// One file's `[env]` item, with the file that defines it.
pub(super) struct Source<'source> {
    pub(super) rank: Rank,
    pub(super) path: &'source Path,
    pub(super) item: Item,
}

/// A key's merged definition: Cargo merges each field of a table on its own.
enum Merged<'source> {
    Simple {
        value: &'source str,
    },
    Options {
        value: Option<(&'source str, &'source Path)>,
        force: Option<(bool, &'source Path)>,
        relative: Option<(bool, &'source Path)>,
        path: &'source Path,
    },
}

impl Table {
    /// Merge every file's `[env]` as Cargo does, then resolve each value.
    pub(super) fn merge(sources: &mut [Source<'_>]) -> Result<Self> {
        sources.sort_by_key(|source| std::cmp::Reverse(source.rank));
        let mut merged = BTreeMap::<&str, (Merged<'_>, &Path)>::new();
        for source in sources.iter() {
            let table = source
                .item
                .as_table_like()
                .ok_or_else(|| invalid(source.path, "env", "a table"))?;
            for (key, item) in table.iter() {
                let current = definition(source.path, key, item)?;
                match merged.get_mut(key) {
                    None => {
                        merged.insert(key, (current, source.path));
                    }
                    Some((kept, kept_path)) => absorb(kept, kept_path, current, source.path, key)?,
                }
            }
        }
        let mut entries = Vec::with_capacity(merged.len());
        for (key, (definition, _)) in merged {
            if DISALLOWED.contains(&key) {
                return Err(Error::Request {
                    reason: format!(
                        "setting the `{key}` environment variable is not supported in the \
                         `[env]` configuration table"
                    )
                    .into_boxed_str(),
                });
            }
            entries.push(resolve(key, definition)?);
        }
        Ok(Self {
            entries: entries.into_boxed_slice(),
        })
    }

    /// The value Cargo's fingerprint records for `key` when Cargo itself runs
    /// with `environment`: `[env]` first, when it is forced or the variable is
    /// unset, then the process environment.
    pub(crate) fn value<'value>(
        &'value self,
        key: &str,
        environment: &'value [(OsString, OsString)],
    ) -> Option<&'value str> {
        let mut process = environment.iter().filter(|(name, _)| name == key);
        match self.entry(key) {
            Some(entry) if entry.force || process.clone().next().is_none() => {
                entry.value.as_deref()
            }
            _ => process.find_map(|(_, value)| value.to_str()),
        }
    }

    /// A resolved table, sorted by key, for tests outside this module.
    #[cfg(test)]
    pub(crate) fn fixture(entries: &[(&str, &str, bool)]) -> Self {
        let mut entries = entries
            .iter()
            .map(|(key, value, force)| Entry {
                key: (*key).into(),
                value: Some((*value).into()),
                force: *force,
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.key.cmp(&right.key));
        Self {
            entries: entries.into_boxed_slice(),
        }
    }

    /// Apply this table to a process environment as Cargo does for a child.
    pub(crate) fn apply(&self, environment: &mut BTreeMap<String, String>) {
        for entry in &self.entries {
            if !entry.force && environment.contains_key(entry.key.as_ref()) {
                continue;
            }
            match &entry.value {
                Some(value) => {
                    environment.insert(entry.key.to_string(), value.to_string());
                }
                None => {
                    environment.remove(entry.key.as_ref());
                }
            }
        }
    }

    fn entry(&self, key: &str) -> Option<&Entry> {
        self.entries
            .binary_search_by(|entry| entry.key.as_ref().cmp(key))
            .ok()
            .map(|index| &self.entries[index])
    }
}

fn definition<'source>(
    path: &'source Path,
    key: &str,
    item: &'source Item,
) -> Result<Merged<'source>> {
    if let Some(value) = item.as_str() {
        return Ok(Merged::Simple { value });
    }
    let table = item
        .as_table_like()
        .ok_or_else(|| invalid(path, &format!("env.{key}"), "a string or a table"))?;
    let field = |name: &str| table.get(name).filter(|item| !item.is_none());
    let flag = |name: &str| -> Result<Option<(bool, &'source Path)>> {
        field(name)
            .map(|item| {
                item.as_bool()
                    .map(|flag| (flag, path))
                    .ok_or_else(|| invalid(path, &format!("env.{key}.{name}"), "a boolean"))
            })
            .transpose()
    };
    let value = field("value")
        .map(|item| {
            item.as_str()
                .map(|value| (value, path))
                .ok_or_else(|| invalid(path, &format!("env.{key}.value"), "a string"))
        })
        .transpose()?;
    Ok(Merged::Options {
        value,
        force: flag("force")?,
        relative: flag("relative")?,
        path,
    })
}

/// `merge_helper` without `force`: the definition already kept is nearer and
/// keeps every field it has; a table and a string never merge.
fn absorb<'source>(
    kept: &mut Merged<'source>,
    kept_path: &Path,
    lower: Merged<'source>,
    lower_path: &Path,
    key: &str,
) -> Result<()> {
    match (kept, lower) {
        (Merged::Simple { .. }, Merged::Simple { .. }) => Ok(()),
        (
            Merged::Options {
                value,
                force,
                relative,
                ..
            },
            Merged::Options {
                value: lower_value,
                force: lower_force,
                relative: lower_relative,
                ..
            },
        ) => {
            *value = value.or(lower_value);
            *force = force.or(lower_force);
            *relative = relative.or(lower_relative);
            Ok(())
        }
        (kept, _) => {
            let (expected, found) = match kept {
                Merged::Simple { .. } => ("a string", "table"),
                Merged::Options { .. } => ("table", "a string"),
            };
            Err(Error::Request {
                reason: format!(
                    "failed to merge key `{key}` between {} and {}: failed to merge config \
                     value from `{}` into `{}`: expected {expected}, but found {found}",
                    kept_path.display(),
                    lower_path.display(),
                    lower_path.display(),
                    kept_path.display(),
                )
                .into_boxed_str(),
            })
        }
    }
}

fn resolve(key: &str, definition: Merged<'_>) -> Result<Entry> {
    let (value, force) = match definition {
        Merged::Simple { value } => (Some(Box::from(value)), false),
        Merged::Options {
            value,
            force,
            relative,
            path,
        } => {
            let (value, defined) =
                value.ok_or_else(|| missing(path, &format!("env.{key}"), "value"))?;
            let force = force.is_some_and(|(force, _)| force);
            let value = if relative.is_some_and(|(relative, _)| relative) {
                // `Definition::root`: the directory above the defining file's `.cargo`.
                let root = defined
                    .parent()
                    .and_then(Path::parent)
                    .expect("a Cargo config file sits two directories deep");
                root.join(value).to_str().map(Box::from)
            } else {
                Some(Box::from(value))
            };
            (value, force)
        }
    };
    Ok(Entry {
        key: key.into(),
        value,
        force,
    })
}

fn invalid(path: &Path, key: &str, expected: &str) -> Error {
    Error::Request {
        reason: format!(
            "invalid configuration for key `{key}`: expected {expected}, in `{}`",
            path.display()
        )
        .into_boxed_str(),
    }
}

fn missing(path: &Path, key: &str, field: &str) -> Error {
    Error::Request {
        reason: format!(
            "could not load config key `{key}`: missing field `{field}`, in `{}`",
            path.display()
        )
        .into_boxed_str(),
    }
}

#[cfg(test)]
mod tests;
