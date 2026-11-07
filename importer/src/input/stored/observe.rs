//! Child-free filesystem entries under the current owner's cumulative accounts.

use std::path::Path;

use super::view::View;
use super::{Budget, Result};
use crate::input::proof::{Entry, Role};

mod directory;
pub(in crate::input) mod path;
pub(in crate::input) mod read;
pub(in crate::input) mod slots;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("{operation}: {source}")]
    Io {
        operation: &'static str,
        source: std::io::Error,
    },
    #[error("input path policy: {0}")]
    Policy(&'static str),
    #[error("input changed during observation: {0}")]
    Drift(&'static str),
    #[error("filesystem observation storage cannot be allocated")]
    Allocation,
}

/// Observe data only. The owner must bind `base` to its original path semantics
/// and compare this entry with the complete cold proof before gaining authority.
pub(in crate::input) fn entry<Package>(
    view: View<'_, Package>,
    base: &Path,
    budget: &mut Budget<'_>,
) -> Result<Entry> {
    budget.check()?;
    match view {
        View::File {
            role,
            package_id,
            path,
        } => file(role, package_id, path, base, budget),
        View::Path {
            role,
            package_id,
            path,
        } => optional(role, package_id, path, base, budget),
        View::Directory {
            role,
            package_id,
            path,
        } => directory::entry(role, package_id, path, base, budget),
        View::Workspace { .. }
        | View::DepInfo { .. }
        | View::SavedOutput { .. }
        | View::Package(_)
        | View::Environment { .. }
        | View::EnvironmentProjection { .. }
        | View::Fact { .. }
        | View::BuildMessage { .. }
        | View::Directive { .. } => {
            Err(Error::Policy("recipe is not a filesystem observation").into())
        }
    }
}

/// A path or Git package's source tree, walked as its cold capture walked
/// it: a build script without `rerun-if` inputs reruns when any of it changes.
pub(in crate::input) fn package_tree(
    package_id: &str,
    root: &Path,
    base: &Path,
    budget: &mut Budget<'_>,
) -> Result<Entry> {
    directory::entry(
        Role::DefaultPackageSource,
        Some(package_id),
        root,
        base,
        budget,
    )
}

/// Record a file's length, then stream it into its digest.
pub(in crate::input) fn file(
    role: Role,
    package_id: Option<&str>,
    source: &Path,
    base: &Path,
    budget: &mut Budget<'_>,
) -> Result<Entry> {
    budget.check()?;
    // A config candidate keeps the path Cargo reads; Cargo follows a symlinked
    // `.cargo` directory or config file, and its content is read through it.
    let source = if follows(role) {
        path::normalized(source, base, budget)?
    } else {
        path::canonical(source, base, false, budget)?
    };
    let metadata = path::metadata(&source, false, budget)?;
    let bytes = read::length(metadata.len(), budget)?;
    let digest = read::digest(&source, bytes, budget)?;
    Ok(Entry::File {
        role,
        package_id: package(package_id, budget)?,
        path: path::text(&source, budget)?,
        bytes,
        digest,
    })
}

/// Configuration candidates are absent or regular files, never directory trees.
pub(in crate::input) fn optional_file(
    role: Role,
    package_id: Option<&str>,
    source: &Path,
    base: &Path,
    budget: &mut Budget<'_>,
) -> Result<Entry> {
    budget.check()?;
    let normalized = path::normalized(source, base, budget)?;
    match path::metadata(&normalized, false, budget) {
        Ok(metadata) if metadata.is_file() => file(role, package_id, &normalized, base, budget),
        Ok(_) => Err(Error::Policy("config-input-shape").into()),
        Err(super::Error::Observation(Error::Io { source, .. }))
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            Ok(Entry::Missing {
                role,
                package_id: package(package_id, budget)?,
                path: path::text(&normalized, budget)?,
            })
        }
        Err(error) => Err(error),
    }
}

fn optional(
    role: Role,
    package_id: Option<&str>,
    source: &Path,
    base: &Path,
    budget: &mut Budget<'_>,
) -> Result<Entry> {
    let normalized = path::normalized(source, base, budget)?;
    match path::metadata(&normalized, false, budget) {
        Ok(metadata) if metadata.is_file() => file(role, package_id, &normalized, base, budget),
        Ok(metadata) if metadata.is_dir() => {
            directory::entry(role, package_id, &normalized, base, budget)
        }
        Ok(_) => Err(Error::Policy("unsupported-input-shape").into()),
        Err(super::Error::Observation(Error::Io { source, .. }))
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            Ok(Entry::Missing {
                role,
                package_id: package(package_id, budget)?,
                path: path::text(&normalized, budget)?,
            })
        }
        Err(error) => Err(error),
    }
}

/// A config file keeps the path Cargo reads it at; every other input is
/// recorded at the canonical path its symlinks resolve to.
fn follows(role: Role) -> bool {
    matches!(role, Role::Config)
}

fn package(value: Option<&str>, budget: &mut Budget<'_>) -> Result<Option<Box<str>>> {
    value
        .map(|value| {
            budget.check()?;
            budget.owned_text(value)
        })
        .transpose()
}

fn io(operation: &'static str, source: std::io::Error) -> super::Error {
    Error::Io { operation, source }.into()
}

#[cfg(test)]
mod tests;
