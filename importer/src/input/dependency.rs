//! Pinned rustc dep-info source and environment evidence.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::cargo::{self, toolchain};
use crate::input::Allocation;
use crate::{Error, Result};

const BUILD_SCRIPT_EXECUTABLE: &str = "build-script-build";

pub(in crate::input) mod observed;
pub(in crate::input) mod scan;

#[cfg(test)]
mod tests;

pub(super) struct Dependency {
    pub(super) sources: Box<[PathBuf]>,
    #[allow(
        clippy::type_complexity,
        reason = "dep-info records an exact ordered key and optional-value wire tuple"
    )]
    pub(super) environment: Box<[(Box<str>, Option<Box<str>>)]>,
}

impl Dependency {
    pub(super) fn read(
        cargo_commit: &str,
        path: &Path,
        package_root: &Path,
        limits: &cargo::Limits,
        allocation: &mut Allocation,
    ) -> Result<Self> {
        if cargo_commit != toolchain::supported_cargo_commit() {
            return Err(super::input("cargo-commit-skew", cargo_commit));
        }
        let metadata = path.metadata().map_err(|source| Error::Io {
            operation: "inspect rustc dep-info",
            path: path.to_path_buf(),
            source,
        })?;
        let length = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
        allocation.items(1)?;
        allocation.bytes(length)?;
        // Dep-info is an artifact-class read bound to the physical capture
        // bound; its size against the JSON target only records.
        allocation.exceeded("rustc dep-info", limits.json_bytes, length);
        let bytes = super::read_file("rustc dep-info", path, crate::process::CAPTURE_BYTES)?;
        let text = std::str::from_utf8(&bytes).map_err(|error| Error::Input {
            code: "dep-info-utf8",
            reason: error.to_string().into_boxed_str(),
        })?;
        scan::complete(text).map_err(|error| syntax(error, path))?;
        let environment = environment(text, limits, allocation)?;
        allocation.items(1)?;
        allocation.bytes(text.len())?;
        let normalized = scan::normalize(text);
        let tokens = scan::Tokens::normalized(&normalized).map_err(|error| syntax(error, path))?;
        let mut sources = Vec::new();
        let mut token = String::new();
        allocation.items(1)?;
        for event in tokens {
            match event.map_err(|error| syntax(error, path))? {
                scan::Event::Character(character) => {
                    allocation.exceeded(
                        "dep-info token bytes",
                        limits.string_bytes,
                        token.len().saturating_add(character.len_utf8()),
                    );
                    allocation.bytes(character.len_utf8())?;
                    token
                        .try_reserve_exact(character.len_utf8())
                        .map_err(|error| Error::Input {
                            code: "dep-info-allocation",
                            reason: error.to_string().into_boxed_str(),
                        })?;
                    token.push(character);
                }
                scan::Event::Boundary => {
                    append_source(&mut sources, &token, package_root, limits, allocation)?;
                    token.clear();
                }
            }
        }
        sources.sort_unstable();
        if sources.is_empty() {
            return Err(super::input("missing-dep-info-source", path.display()));
        }
        Ok(Self {
            sources: sources.into_boxed_slice(),
            environment,
        })
    }
}

pub(super) fn rustc_path(cargo_commit: &str, output: &Path) -> Result<Option<PathBuf>> {
    pinned(cargo_commit)?;
    let Some(extension) = output.extension().and_then(|extension| extension.to_str()) else {
        return Ok(None);
    };
    if !matches!(extension, "rlib" | "rmeta" | "dylib" | "so" | "a") {
        return Ok(None);
    }
    let Some(stem) = output
        .file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(|stem| stem.strip_prefix("lib"))
    else {
        return Ok(None);
    };
    Ok(Some(output.with_file_name(format!("{stem}.d"))))
}

pub(super) fn build_script_path(cargo_commit: &str, output: &Path) -> Result<Option<PathBuf>> {
    pinned(cargo_commit)?;
    if output.extension().is_some() || !output.is_file() {
        return Ok(None);
    }
    if output.file_name().and_then(|name| name.to_str()) != Some(BUILD_SCRIPT_EXECUTABLE) {
        return Err(super::input(
            "custom-build-dep-info-layout",
            output.display(),
        ));
    }
    let directory = output
        .parent()
        .ok_or_else(|| super::input("custom-build-dep-info-layout", output.display()))?;
    let hash = directory
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.rsplit_once('-').map(|(_, hash)| hash))
        .filter(|hash| !hash.is_empty())
        .ok_or_else(|| super::input("custom-build-dep-info-layout", output.display()))?;
    Ok(Some(directory.join(format!("build_script_build-{hash}.d"))))
}

#[allow(
    clippy::type_complexity,
    reason = "dep-info records an exact ordered key and optional-value wire tuple"
)]
pub(super) fn environment(
    text: &str,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> Result<Box<[(Box<str>, Option<Box<str>>)]>> {
    let mut environment = BTreeMap::new();
    for record in scan::Environment::read(text) {
        let scan::Record { key, value } = record.map_err(|error| match error {
            scan::Error::Malformed(line) => super::input("malformed-env-dep", line),
        })?;
        allocation.exceeded("rustc env-dep key", limits.string_bytes, key.len());
        allocation.items(1)?;
        allocation.string(key)?;
        if let Some(value) = value {
            allocation.exceeded("rustc env-dep value", limits.string_bytes, value.len());
            allocation.string(value)?;
        }
        let value = value.map(Box::from);
        if environment.insert(Box::from(key), value).is_some() {
            return Err(super::input("duplicate-env-dep", key));
        }
    }
    allocation.items(environment.len())?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(environment.len())
        .map_err(|error| Error::Input {
            code: "dep-info-allocation",
            reason: error.to_string().into_boxed_str(),
        })?;
    values.extend(environment);
    Ok(values.into_boxed_slice())
}

fn syntax(error: scan::Syntax, path: &Path) -> Error {
    match error {
        scan::Syntax::TrailingLineEscape => {
            super::input("malformed-dep-info", "trailing line escape")
        }
        scan::Syntax::MissingRule => super::input("malformed-dep-info", path.display()),
        scan::Syntax::TrailingEscape => super::input("malformed-dep-info", "trailing escape"),
    }
}

fn append_source(
    sources: &mut Vec<PathBuf>,
    token: &str,
    package_root: &Path,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> Result<()> {
    allocation.exceeded(
        "dep-info source paths",
        limits.items,
        sources.len().saturating_add(1),
    );
    let raw = Path::new(token);
    let candidate = if raw.is_absolute() {
        allocation.path(raw)?;
        raw.to_path_buf()
    } else {
        allocation.items(1)?;
        allocation.bytes(
            package_root
                .as_os_str()
                .len()
                .saturating_add(1)
                .saturating_add(raw.as_os_str().len()),
        )?;
        package_root.join(raw)
    };
    if !candidate.is_file() {
        return Err(super::input("missing-dep-info-source", candidate.display()));
    }
    let canonical = super::canonical_file("dep-info source", &candidate)?;
    if !sources.contains(&canonical) {
        allocation.items(1)?;
        allocation.path(&canonical)?;
        sources.try_reserve(1).map_err(|error| Error::Input {
            code: "dep-info-allocation",
            reason: error.to_string().into_boxed_str(),
        })?;
        sources.push(canonical);
    }
    Ok(())
}

fn pinned(cargo_commit: &str) -> Result<()> {
    if cargo_commit == toolchain::supported_cargo_commit() {
        Ok(())
    } else {
        Err(super::input("cargo-commit-skew", cargo_commit))
    }
}
