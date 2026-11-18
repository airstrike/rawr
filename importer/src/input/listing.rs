//! The files Cargo lists for a path or Git package, as its path source
//! lists them to fingerprint a build script that declares no `rerun-if`
//! input.
//!
//! When a Git repository tracks the package's manifest and the manifest
//! declares no `include`, the listing is what Git tracks under the package
//! and what it would track there (untracked and not ignored), with the
//! package's own `target` keeping only what Git tracks. A directory Git
//! lists (a submodule, a nested repository, a link to a directory) is
//! listed by its own repository, or else walked. Otherwise the package's
//! directory is walked following links, leaving out its own `target` and,
//! with no `include`, every hidden name. Either way a nested package is
//! left out and the manifest's `include` or `exclude` selects from the
//! rest.

mod files;
mod pattern;
mod rules;

#[cfg(test)]
mod tests;

pub(in crate::input) use files::{files, Error};
