//! Pinned-Cargo saved build-output declarations.

use std::path::{Path, PathBuf};

use crate::cargo::{self, toolchain};
use crate::input::Allocation;
use crate::{Error, Result};

pub(in crate::input) mod observed;
pub(in crate::input) mod scan;

const OUTPUT_FILE: &str = "output";
const OUT_DIRECTORY: &str = "out";
const BUILD_DIRECTORY: &str = "build";

/// Ordered declarations decoded from one pinned-Cargo saved build output.
pub struct Declarations {
    pub(super) package_id: Box<str>,
    pub(super) output: PathBuf,
    pub(super) paths: Box<[Box<str>]>,
    pub(super) environment: Box<[Box<str>]>,
    pub(super) directives: Box<[Box<str>]>,
}

impl Declarations {
    /// Decode the exact saved-output artifact anchored by public Cargo JSON.
    pub fn read(
        cargo_commit: &str,
        package_id: &str,
        out_dir: &Path,
        limits: &cargo::Limits,
    ) -> Result<Self> {
        let mut allocation = Allocation::new(limits);
        Self::read_accounted(cargo_commit, package_id, out_dir, limits, &mut allocation)
    }

    pub(super) fn read_accounted(
        cargo_commit: &str,
        package_id: &str,
        out_dir: &Path,
        limits: &cargo::Limits,
        allocation: &mut Allocation,
    ) -> Result<Self> {
        if cargo_commit != toolchain::supported_cargo_commit() {
            return Err(Error::Input {
                code: "cargo-commit-skew",
                reason: format!("unsupported Cargo commit `{cargo_commit}`").into_boxed_str(),
            });
        }
        // The PackageId length is an accounting target; record it.
        allocation.exceeded(
            "saved-output PackageId",
            limits.string_bytes,
            package_id.len(),
        );
        allocation.string(package_id)?;
        allocation.path(out_dir)?;
        let out_dir = out_directory(out_dir)?;
        let parent = out_dir
            .parent()
            .ok_or_else(|| super::input("saved-output-layout", out_dir.display()))?;
        allocation.items(1)?;
        allocation.bytes(
            parent
                .as_os_str()
                .len()
                .saturating_add(1)
                .saturating_add(OUTPUT_FILE.len()),
        )?;
        let candidate = parent.join(OUTPUT_FILE);
        let output = super::canonical_file("Cargo saved build output", &candidate)?;
        allocation.path(&output)?;
        // The saved output is an artifact-class read bound to the physical
        // capture bound; its size against the JSON target only records.
        let bytes = super::read_file_budgeted(
            "Cargo saved build output",
            &output,
            crate::process::CAPTURE_BYTES,
            allocation,
        )?;
        allocation.exceeded("Cargo saved build output", limits.json_bytes, bytes.len());
        let mut paths = Vec::new();
        let mut environment = Vec::new();
        let mut directives = Vec::new();
        scan::visit(&bytes, |event| -> Result<()> {
            let (values, value) = match event {
                scan::Event::Directive(value) => (&mut directives, value),
                scan::Event::Path(value) => (&mut paths, value),
                scan::Event::Environment(value) => (&mut environment, value),
            };
            allocation.items(1)?;
            allocation.string(value)?;
            values.try_reserve(1).map_err(|error| Error::Input {
                code: "saved-output-allocation",
                reason: error.to_string().into_boxed_str(),
            })?;
            values.push(Box::<str>::from(value));
            Ok(())
        })?;
        Ok(Self {
            package_id: package_id.into(),
            output,
            paths: paths.into_boxed_slice(),
            environment: environment.into_boxed_slice(),
            directives: directives.into_boxed_slice(),
        })
    }

    /// The exact number of ordered Cargo instructions retained.
    pub fn directive_count(&self) -> usize {
        self.directives.len()
    }

    /// Cargo's exact source-qualified package identity bound to this output.
    pub fn package_id(&self) -> &str {
        &self.package_id
    }

    /// The exact number of `rerun-if-changed` declarations retained.
    pub fn path_count(&self) -> usize {
        self.paths.len()
    }

    /// The exact number of `rerun-if-env-changed` declarations retained.
    pub fn environment_count(&self) -> usize {
        self.environment.len()
    }
}

impl From<scan::Error<'_>> for Error {
    fn from(error: scan::Error<'_>) -> Self {
        match error {
            scan::Error::Truncated => super::input(
                "truncated-saved-output",
                "saved build output does not end at a line boundary",
            ),
            scan::Error::Utf8(error) => Error::Input {
                code: "saved-output-utf8",
                reason: error.to_string().into_boxed_str(),
            },
            scan::Error::Malformed(reason) => super::input("malformed-saved-output", reason),
        }
    }
}

pub(super) fn out_directory(path: &Path) -> Result<PathBuf> {
    let path = super::canonical_directory("Cargo OUT_DIR", path)?;
    if path.file_name().and_then(|name| name.to_str()) != Some(OUT_DIRECTORY)
        || path
            .parent()
            .and_then(Path::parent)
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            != Some(BUILD_DIRECTORY)
    {
        return Err(super::input("saved-output-layout", path.display()));
    }
    Ok(path)
}

#[cfg(test)]
mod tests;
