//! Borrowed recipe payloads, with package data separated from live execution.

use std::path::{Path, PathBuf};

use crate::input::proof::Role;

/// A read of one recipe; constructing it neither allocates nor validates input.
///
/// The package parameter preserves the distinction between a live package set
/// and historical registry fields. Only the live set can carry package replay.
pub(in crate::input) enum View<'a, Package> {
    Workspace {
        roots: &'a [PathBuf],
    },
    File {
        role: Role,
        package_id: Option<&'a str>,
        path: &'a Path,
    },
    Path {
        role: Role,
        package_id: Option<&'a str>,
        path: &'a Path,
    },
    Directory {
        role: Role,
        package_id: Option<&'a str>,
        path: &'a Path,
    },
    DepInfo {
        package_id: &'a str,
        path: &'a Path,
        package_root: &'a Path,
        out_directories: &'a [PathBuf],
    },
    SavedOutput {
        package_id: &'a str,
        out_dir: &'a Path,
    },
    Package(&'a Package),
    Environment {
        package_id: &'a str,
        execution: &'a str,
        key: &'a str,
        value: Option<&'a str>,
    },
    EnvironmentProjection {
        schema: u32,
        key: &'a str,
        value: Option<&'a str>,
    },
    Fact {
        role: Role,
        package_id: Option<&'a str>,
        name: &'a str,
        value: &'a str,
    },
    BuildMessage {
        package_id: &'a str,
        execution: &'a str,
        name: &'a str,
        value: &'a str,
    },
    Directive {
        package_id: &'a str,
        execution: &'a str,
        index: usize,
        value: &'a str,
    },
}

#[cfg(test)]
mod tests;
