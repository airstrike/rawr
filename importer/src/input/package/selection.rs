//! The expected raw package selector and its complete imported owner join.
//!
//! This is capability-free data, not a workspace observation or a current
//! membership proof. The successful pinned command must select exactly one
//! member, and the complete before/command/output/after association must bind
//! that member to this owner before production activation.
//!
//! Exact Cargo 59800466c5c41c444d264b1010b4d57e85a7117f provenance:
//! - cargo-util-schemas/core/package_id_spec.rs:80,212 parses raw name@version.
//! - core/package_id_spec.rs:40 and schemas/core/partial_version.rs:42 define
//!   command matching; absent build metadata is weaker than owner equality.
//! - core/workspace.rs:865,1040 distinguishes the current-manifest singleton
//!   from a rooted workspace whose member names are validated as unique.
//! - ops/cargo_package/mod.rs:203 queries the explicit spec uniquely and filters
//!   output to matching members, independently of workspace default members.
//! - core/workspace.rs:1878 and core/source_id.rs:201 construct a command-local
//!   Path source, even when the authenticated imported package source is Git.
//! - bin/cargo/commands/package.rs:60 rejects an embedded root before selection.
//!   The future contained CLI computation must preserve that branch; deriving
//!   expected identity here neither performs it nor declares such input allowed.
//!
//! Name/version come from authenticated normalized metadata, including manifest
//! inheritance. No URL parser, raw manifest parser, normalization or fallback
//! convention reconstructs that owner. See the saved evidence for hashes.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::cargo::{toolchain, unit};
use crate::input::Allocation;
use crate::{Error, Result};

#[cfg(test)]
mod tests;

#[cfg(test)]
std::thread_local! {
    static OWNED_BUFFERS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Version of the uniform raw name@full_version convention, independent of the
/// human archive-output representation. No earlier draft acquired authority.
const SCHEMA: u32 = 1;

/// An owned expectation, never evidence that the named command has succeeded.
#[derive(Debug)]
pub(in crate::input) struct Selection {
    package_id: String,
    source: Source,
    local: Local,
    selector: String,
}

/// Structural expected Path PackageId, sharing the retained owner fields.
/// This is deliberately not a serialized PackageIdSpec or an observed member.
#[derive(Debug)]
pub(in crate::input) struct Local {
    name: String,
    version: String,
    root: PathBuf,
    manifest: PathBuf,
    basis: Basis,
}

#[derive(Debug)]
enum Source {
    Path { cargo_id: String },
    Git { cargo_id: String, oid: String },
}

#[derive(Debug)]
enum Basis {
    Root,
    Manifest,
}

impl Selection {
    /// Transform one authenticated metadata package into its owned expectation.
    ///
    /// Metadata already joins exact name/version/source to the canonical file
    /// and its parent. The actual command-local member and applicable workspace
    /// branch still require the complete observation host. Registry packages
    /// retain their separate proof and have no package-list selector.
    pub(in crate::input) fn derive(
        cargo_commit: &str,
        package: &unit::Package,
        manifest: &Path,
        allocation: &mut Allocation,
    ) -> Result<Self> {
        if cargo_commit != toolchain::supported_cargo_commit() {
            return Err(error(
                "package-selector-cargo",
                "Cargo commit differs from the selector pin",
            ));
        }
        if manifest.as_os_str() != package.manifest().as_os_str()
            || manifest.parent() != Some(package.root())
            || !package.root().is_absolute()
        {
            return Err(error(
                "package-selector-manifest",
                "selector manifest differs from the authenticated canonical package manifest",
            ));
        }
        let source = match package.source() {
            unit::PackageSource::Path { cargo_id } => (cargo_id.as_ref(), None),
            unit::PackageSource::Git { cargo_id, oid } => (cargo_id.as_ref(), Some(oid.as_ref())),
            unit::PackageSource::Registry { .. } => {
                return Err(error(
                    "package-selector-source",
                    "registry packages do not use package-list selection",
                ));
            }
        };
        let selector_len = package
            .name()
            .len()
            .checked_add(1)
            .and_then(|length| length.checked_add(package.version().len()))
            .ok_or_else(|| error("package-selector-length", "selector length overflowed"))?;

        // Charge every copy and the complete type before the first variable-size
        // allocation. Raw rendering needs no intermediate or parser scratch.
        allocation.items(1)?;
        allocation.bytes(std::mem::size_of::<Self>())?;
        for value in [package.id(), package.name(), package.version(), source.0] {
            allocation.string(value)?;
        }
        if let Some(oid) = source.1 {
            allocation.string(oid)?;
        }
        allocation.path(package.root())?;
        allocation.path(manifest)?;
        allocation.string_exceeded("input owned string bytes", selector_len);
        allocation.items(1)?;
        allocation.bytes(selector_len)?;

        let mut selector = buffer(selector_len)?;
        selector.push_str(package.name());
        selector.push('@');
        selector.push_str(package.version());
        let source = match source {
            (cargo_id, None) => Source::Path {
                cargo_id: copy(cargo_id)?,
            },
            (cargo_id, Some(oid)) => Source::Git {
                cargo_id: copy(cargo_id)?,
                oid: copy(oid)?,
            },
        };
        Ok(Self {
            package_id: copy(package.id())?,
            source,
            local: Local {
                name: copy(package.name())?,
                version: copy(package.version())?,
                root: copy_path(package.root())?,
                manifest: copy_path(manifest)?,
                basis: Basis::from_manifest(manifest),
            },
            selector,
        })
    }

    pub(in crate::input) const fn schema(&self) -> u32 {
        SCHEMA
    }

    pub(in crate::input) fn selector(&self) -> &str {
        &self.selector
    }

    pub(in crate::input) fn package_id(&self) -> &str {
        &self.package_id
    }

    pub(in crate::input) const fn local(&self) -> &Local {
        &self.local
    }

    /// Compare retained data only; equality supplies no current observation.
    pub(in crate::input) fn matches_package(&self, package: &unit::Package) -> bool {
        self.package_id == package.id()
            && self.local.name == package.name()
            && self.local.version == package.version()
            && self.local.root.as_os_str() == package.root().as_os_str()
            && self.local.manifest.as_os_str() == package.manifest().as_os_str()
            && match (&self.source, package.source()) {
                (Source::Path { cargo_id: expected }, unit::PackageSource::Path { cargo_id }) => {
                    expected == cargo_id.as_ref()
                }
                (
                    Source::Git {
                        cargo_id: expected,
                        oid: expected_oid,
                    },
                    unit::PackageSource::Git { cargo_id, oid },
                ) => expected == cargo_id.as_ref() && expected_oid == oid.as_ref(),
                _ => false,
            }
    }
}

impl Local {
    pub(in crate::input) fn name(&self) -> &str {
        &self.name
    }

    pub(in crate::input) fn version(&self) -> &str {
        &self.version
    }

    pub(in crate::input) fn root(&self) -> &Path {
        &self.root
    }

    pub(in crate::input) fn manifest(&self) -> &Path {
        &self.manifest
    }

    /// Native path from which Cargo would construct this expected Path SourceId.
    /// Imported Git source/OID never supplies this command-local Path identity.
    pub(in crate::input) fn source_path(&self) -> &Path {
        match self.basis {
            Basis::Root => &self.root,
            Basis::Manifest => &self.manifest,
        }
    }
}

impl Basis {
    fn from_manifest(manifest: &Path) -> Self {
        // Cargo util/toml/mod.rs:52 and core/source_id.rs:208. The imported
        // canonical manifest was authenticated as a file; this only derives
        // that observation's expected source, not a current is_file result.
        // CLI rejection depends on the workspace root and is a separate branch.
        match manifest.extension() {
            None => Self::Manifest,
            Some(extension) if extension == OsStr::new("rs") => Self::Manifest,
            Some(_) => Self::Root,
        }
    }
}

fn buffer(length: usize) -> Result<String> {
    #[cfg(test)]
    OWNED_BUFFERS.with(|owned| owned.set(owned.get().saturating_add(1)));
    let mut value = String::new();
    value.try_reserve_exact(length).map_err(|_| {
        error(
            "package-selector-allocation",
            "selector data allocation failed",
        )
    })?;
    Ok(value)
}

fn copy(value: &str) -> Result<String> {
    let mut owned = buffer(value.len())?;
    owned.push_str(value);
    Ok(owned)
}

fn copy_path(value: &Path) -> Result<PathBuf> {
    #[cfg(test)]
    OWNED_BUFFERS.with(|owned| owned.set(owned.get().saturating_add(1)));
    let mut owned = PathBuf::new();
    owned
        .try_reserve_exact(value.as_os_str().len())
        .map_err(|_| {
            error(
                "package-selector-allocation",
                "selector path allocation failed",
            )
        })?;
    owned.push(value);
    Ok(owned)
}

fn error(code: &'static str, reason: &'static str) -> Error {
    Error::Input {
        code,
        reason: reason.into(),
    }
}
