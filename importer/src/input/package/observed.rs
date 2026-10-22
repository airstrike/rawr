//! Registry fallback observations under the current owner's original accounts.

use std::path::Path;

use sha2::{Digest as _, Sha256};

use super::{registry_relative_shape, Stored, REGISTRY_MARKER_SCHEMA};
use crate::cargo::unit;
use crate::input::proof::{Entry, Role};
use crate::input::stored::{self, observe, Budget};

mod parser;

/// Reobserve the extraction marker of an exactly associated registry package.
/// The entry is data only; historical usage grants no current work allowance.
pub(in crate::input) fn registry(
    retained: &Stored,
    current: &unit::Package,
    cargo_home: &Path,
    base: &Path,
    budget: &mut Budget<'_>,
) -> stored::Result<Entry> {
    acquire(retained, current, cargo_home, base, budget, |_| Ok(()))
}

fn acquire(
    retained: &Stored,
    current: &unit::Package,
    cargo_home: &Path,
    base: &Path,
    budget: &mut Budget<'_>,
    before_open: impl FnOnce(&Path) -> stored::Result<()>,
) -> stored::Result<Entry> {
    budget.check()?;
    let unit::PackageSource::Registry {
        cargo_id,
        checksum: Some(checksum),
    } = current.source()
    else {
        return Err(policy("registry source or checksum is not associated"));
    };
    let exact_id = current
        .id()
        .strip_prefix(cargo_id.as_ref())
        .and_then(|suffix| suffix.strip_prefix('#'))
        .and_then(|suffix| suffix.split_once('@'))
        .is_some_and(|(name, version)| name == current.name() && version == current.version());
    if !exact_id
        || current.id() != retained.package_id.as_ref()
        || cargo_id.as_ref() != retained.source.as_ref()
        || checksum != &retained.checksum
        || current.root().as_os_str() != retained.root.as_os_str()
        || !child(current.root(), "Cargo.toml", current.manifest())
        || !child(&retained.root, ".cargo-ok", &retained.marker)
    {
        return Err(policy("registry package identity is not associated"));
    }
    let cargo_home = observe::path::canonical(cargo_home, base, true, budget)?;
    let source_candidate = observe::path::joined(&cargo_home, Path::new("registry/src"), budget)?;
    let source_root = observe::path::canonical(&source_candidate, base, true, budget)?;
    let root = observe::path::canonical(current.root(), base, true, budget)?;
    if root.as_os_str() != retained.root.as_os_str() {
        return Err(policy("registry package root is not canonical"));
    }
    let relative = root
        .strip_prefix(&source_root)
        .map_err(|_| policy("registry-root-outside-cargo-home"))?;
    if !registry_relative_shape(relative, current.name(), current.version()) {
        return Err(policy("registry-root-shape"));
    }
    // The retained marker path only participates in the exact association
    // above. Acquire the child derived from the currently checked root.
    let candidate = observe::path::joined(&root, Path::new(".cargo-ok"), budget)?;
    let marker = observe::path::canonical(&candidate, base, false, budget)?;
    if marker.as_os_str() != retained.marker.as_os_str() {
        return Err(policy("registry extraction marker path changed"));
    }
    let metadata = observe::path::metadata(&marker, false, budget)?;
    let maximum = usize::try_from(metadata.len()).map_err(|_| budget.overflow())?;
    before_open(&marker)?;
    let bytes = observe::read::bounded(&marker, maximum, budget)?;
    budget.limited(
        "registry extraction marker bytes",
        budget.limits.string_bytes,
        bytes.len(),
    )?;
    parser::admit(&bytes, budget)?;
    let marker = super::marker(&bytes).map_err(|_| policy("registry-marker-malformed"))?;
    if marker.v != REGISTRY_MARKER_SCHEMA {
        return Err(policy("registry-marker-schema"));
    }
    budget.hash(bytes.len())?;
    let marker_digest: [u8; 32] = Sha256::digest(&bytes).into();
    if marker_digest != retained.marker_digest {
        return Err(observe::Error::Drift("registry-marker-changed").into());
    }
    let package_id = scalar(current.id(), budget)?;
    let source = scalar(cargo_id, budget)?;
    let root = root
        .to_str()
        .ok_or(stored::Error::Encoding("registry root is not Unicode"))?;
    budget.limited("input path bytes", crate::input::LINE_BYTES, root.len())?;
    let root = budget.owned_text(root)?;
    Ok(Entry::Registry {
        role: Role::DefaultPackageSource,
        package_id,
        source,
        checksum: *checksum,
        root,
        marker_schema: REGISTRY_MARKER_SCHEMA,
        marker_digest,
    })
}

/// Exact native spelling of the same direct-child join used by cold capture.
/// This borrows bytes so association rejection precedes storage and I/O.
fn child(root: &Path, name: &str, candidate: &Path) -> bool {
    let root = root.as_os_str().as_encoded_bytes();
    let Some(suffix) = candidate.as_os_str().as_encoded_bytes().strip_prefix(root) else {
        return false;
    };
    if root.ends_with(b"/") {
        suffix == name.as_bytes()
    } else {
        suffix.strip_prefix(b"/") == Some(name.as_bytes())
    }
}

fn scalar(value: &str, budget: &mut Budget<'_>) -> stored::Result<Box<str>> {
    budget.limited(
        "input owned string bytes",
        budget.limits.string_bytes,
        value.len(),
    )?;
    budget.owned_text(value)
}

fn policy(reason: &'static str) -> stored::Error {
    observe::Error::Policy(reason).into()
}

#[cfg(test)]
mod tests;
