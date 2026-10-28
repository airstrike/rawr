//! Which selected crates' extraction tables are shared per machine, and by
//! what key (D608.4, D608.8).
//!
//! A crate is shared when every package its unit reaches, through normal,
//! build and proc-macro dependencies, comes from a registry or a git commit:
//! nothing of it can change under its key. Path and workspace packages, and
//! anything that reaches one, stay with their project. The key is the
//! crate's unit key from the resolution, which covers its features, profile,
//! target and its dependencies' unit keys; the checksum or commit of every
//! package it reaches; the extraction toolchain; and the extraction's
//! formats. A build script's outputs are compared before an entry is used or
//! published (`bindings.rs`).

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::Deserialize;
use sha2::{Digest as _, Sha256};

use crate::cargo::unit::{Metadata, PackageSource};

/// A selected crate's shared key, and the packages in its closure that run a
/// build script.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Shared {
    pub(crate) key: Box<str>,
    pub(crate) scripts: Box<[Box<str>]>,
}

/// What identifies one selected unit in the unit graph.
#[derive(Clone, Copy)]
pub(crate) struct Selected<'a> {
    pub(crate) unit_key: &'a str,
    pub(crate) package_id: &'a str,
    pub(crate) target: &'a str,
    pub(crate) features: &'a [Box<str>],
}

#[derive(Deserialize)]
struct Document {
    units: Vec<Unit>,
}

#[derive(Deserialize)]
struct Unit {
    pkg_id: String,
    target: Target,
    mode: String,
    features: Vec<String>,
    dependencies: Vec<Dependency>,
}

#[derive(Deserialize)]
struct Target {
    kind: Vec<String>,
    src_path: String,
}

#[derive(Deserialize)]
struct Dependency {
    index: usize,
}

/// Each shared selected unit's key, by its unit key. A unit graph that does
/// not parse shares nothing.
pub(crate) fn keys(
    units: &[u8],
    metadata: &Metadata,
    selected: &[Selected<'_>],
    compiler_commit: &str,
    host: &str,
    target: &str,
) -> BTreeMap<Box<str>, Shared> {
    let Ok(document) = serde_json::from_slice::<Document>(units) else {
        return BTreeMap::new();
    };
    let mut keys = BTreeMap::new();
    for unit in selected {
        let mut matching = document.units.iter().enumerate().filter(|(_, candidate)| {
            candidate.pkg_id == unit.package_id
                && candidate.target.src_path == unit.target
                && matches!(candidate.mode.as_str(), "check" | "build")
                && candidate
                    .target
                    .kind
                    .iter()
                    .any(|kind| matches!(kind.as_str(), "lib" | "rlib"))
                && candidate.features.len() == unit.features.len()
                && candidate
                    .features
                    .iter()
                    .zip(unit.features)
                    .all(|(left, right)| left.as_str() == right.as_ref())
        });
        let (Some((root, _)), None) = (matching.next(), matching.next()) else {
            continue;
        };
        if let Some(shared) = key(
            &document,
            root,
            metadata,
            unit.unit_key,
            [compiler_commit, host, target],
        ) {
            keys.insert(Box::from(unit.unit_key), shared);
        }
    }
    keys
}

fn key(
    document: &Document,
    root: usize,
    metadata: &Metadata,
    unit_key: &str,
    toolchain: [&str; 3],
) -> Option<Shared> {
    let mut seen = BTreeSet::from([root]);
    let mut queue = VecDeque::from([root]);
    let mut sources = BTreeSet::new();
    let mut scripts = BTreeSet::new();
    while let Some(index) = queue.pop_front() {
        let unit = document.units.get(index)?;
        let source = match metadata.package(&unit.pkg_id)?.source() {
            PackageSource::Registry {
                checksum: Some(checksum),
                ..
            } => crate::extract::engine::hex(checksum),
            PackageSource::Git { oid, .. } => oid.to_string(),
            PackageSource::Registry { checksum: None, .. } | PackageSource::Path { .. } => {
                return None;
            }
        };
        sources.insert((unit.pkg_id.as_str(), source));
        if unit.mode == "run-custom-build" {
            scripts.insert(unit.pkg_id.as_str());
        }
        for dependency in &unit.dependencies {
            if seen.insert(dependency.index) {
                queue.push_back(dependency.index);
            }
        }
    }
    let mut digest = Sha256::new();
    let mut frame = |bytes: &[u8]| {
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(bytes);
    };
    frame(b"rawr.bindings/v1");
    frame(&crate::extract::engine::TABLE_SCHEMA.to_le_bytes());
    frame(&crate::extract::protocol::SCHEMA.to_le_bytes());
    for value in toolchain {
        frame(value.as_bytes());
    }
    frame(unit_key.as_bytes());
    frame(&(sources.len() as u64).to_le_bytes());
    for (package, source) in &sources {
        frame(package.as_bytes());
        frame(source.as_bytes());
    }
    Some(Shared {
        key: crate::extract::engine::hex(&digest.finalize()).into_boxed_str(),
        scripts: scripts.into_iter().map(Box::from).collect(),
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{keys, Selected};
    use crate::cargo::unit::{Metadata, Package, PackageSource};

    const SEMVER: &str = "registry+https://index#semver@1.0.0";
    const LIBC: &str = "registry+https://index#libc@0.2.0";
    const LOCAL: &str = "path+file:///project/local#0.1.0";

    fn registry(id: &str, checksum: u8) -> Package {
        let name = id.rsplit_once('#').unwrap().1.split_once('@').unwrap().0;
        Package::fixture(
            id,
            name,
            "1.0.0",
            PathBuf::from("/registry").join(name),
            PackageSource::Registry {
                cargo_id: id.into(),
                checksum: Some([checksum; 32]),
            },
        )
    }

    fn local() -> Package {
        Package::fixture(
            LOCAL,
            "local",
            "0.1.0",
            PathBuf::from("/project/local"),
            PackageSource::Path {
                cargo_id: LOCAL.into(),
            },
        )
    }

    /// `semver` uses `libc`, whose build script runs; `local` uses both.
    fn units() -> Vec<u8> {
        let unit = |pkg_id: &str, src: &str, mode: &str, dependencies: &[usize]| {
            serde_json::json!({
                "pkg_id": pkg_id,
                "target": { "kind": ["lib"], "src_path": src },
                "mode": mode,
                "features": [],
                "dependencies": dependencies
                    .iter()
                    .map(|index| serde_json::json!({ "index": index }))
                    .collect::<Vec<_>>(),
            })
        };
        serde_json::to_vec(&serde_json::json!({
            "units": [
                unit(LIBC, "/registry/libc/build.rs", "run-custom-build", &[]),
                unit(LIBC, "/registry/libc/src/lib.rs", "check", &[0]),
                unit(SEMVER, "/registry/semver/src/lib.rs", "check", &[1]),
                unit(LOCAL, "/project/local/src/lib.rs", "check", &[2]),
            ]
        }))
        .unwrap()
    }

    fn selected(package_id: &'static str, target: &'static str) -> Selected<'static> {
        Selected {
            unit_key: target,
            package_id,
            target,
            features: &[],
        }
    }

    #[test]
    fn a_crate_from_registries_alone_is_keyed_by_its_closure_and_a_path_crate_is_not_shared() {
        let semver = selected(SEMVER, "/registry/semver/src/lib.rs");
        let path_crate = selected(LOCAL, "/project/local/src/lib.rs");
        let metadata =
            Metadata::fixture_packages([registry(SEMVER, 1), registry(LIBC, 2), local()]);
        let shared = keys(
            &units(),
            &metadata,
            &[semver, path_crate],
            "commit",
            "host",
            "host",
        );
        assert_eq!(shared.len(), 1, "a path crate is never shared");
        let first = &shared[SEMVER_TARGET];
        assert_eq!(first.scripts.as_ref(), [LIBC.into()]);
        // A dependency's new checksum, or another toolchain, is another key.
        let changed = Metadata::fixture_packages([registry(SEMVER, 1), registry(LIBC, 3), local()]);
        let changed = keys(&units(), &changed, &[semver], "commit", "host", "host");
        assert_ne!(changed[SEMVER_TARGET].key, first.key);
        let toolchain = keys(&units(), &metadata, &[semver], "other", "host", "host");
        assert_ne!(toolchain[SEMVER_TARGET].key, first.key);
        let again = keys(&units(), &metadata, &[semver], "commit", "host", "host");
        assert_eq!(again[SEMVER_TARGET], *first);
    }

    const SEMVER_TARGET: &str = "/registry/semver/src/lib.rs";
}
