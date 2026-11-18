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
//! formats. The outputs of the build scripts whose packages' libraries the
//! crate's compile reaches, through normal and proc-macro dependencies, are
//! compared before publication and execution (`bindings.rs`): macro
//! expansion can change the crate's API too. A package reached only through
//! a build script's own dependencies compiles that script alone, whose
//! outputs are the ones compared, so its own script is not.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::Deserialize;
use sha2::{Digest as _, Sha256};

use crate::cargo::unit::{Metadata, PackageSource};

/// A selected crate's shared key, and the packages in its closure that run a
/// build script.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Shared {
    pub(crate) key: Box<str>,
    /// The closure has a path package: the table stays with the project,
    /// and its files are checked before each replay (#398).
    pub(crate) local: bool,
    pub(crate) scripts: Box<[Box<str>]>,
    pub(crate) packages: Box<[Box<str>]>,
}

/// What identifies one selected unit in the unit graph.
#[derive(Clone, Copy)]
pub(crate) struct Selected<'a> {
    pub(crate) unit_key: &'a str,
    pub(crate) package_id: &'a str,
    pub(crate) target: &'a str,
    /// Cargo's compile mode for it: a library built for the target and as
    /// a host dependency of a build script or procedural macro has a unit
    /// of each, with the same features, and only the mode tells them apart.
    pub(crate) mode: &'a str,
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
    keys_with(
        units,
        metadata,
        selected,
        [compiler_commit, host, target],
        false,
    )
}

/// As [`keys`], and also each selected unit whose closure reaches a path
/// package, keyed for the project alone. Its sources can change under its
/// key, so its table is replayed only after its files are checked.
pub(crate) fn keys_local(
    units: &[u8],
    metadata: &Metadata,
    selected: &[Selected<'_>],
    compiler_commit: &str,
    host: &str,
    target: &str,
) -> BTreeMap<Box<str>, Shared> {
    keys_with(
        units,
        metadata,
        selected,
        [compiler_commit, host, target],
        true,
    )
}

/// Each registry or git package with a unit whose closure reaches a path
/// package, as a `[patch]` or `[replace]` to a path makes it. Whatever such
/// a crate keeps depends on the project's files, so it stays with the
/// project (D608.4), whether or not a selected crate's key names it. A unit
/// graph that does not parse names none.
pub(crate) fn local_closures(units: &[u8], metadata: &Metadata) -> Box<[Box<str>]> {
    let Ok(document) = serde_json::from_slice::<Document>(units) else {
        return Box::default();
    };
    let path = |unit: &Unit| {
        metadata
            .package(&unit.pkg_id)
            .is_some_and(|package| matches!(package.source(), PackageSource::Path { .. }))
    };
    let mut dependents = vec![Vec::new(); document.units.len()];
    for (index, unit) in document.units.iter().enumerate() {
        for dependency in &unit.dependencies {
            if let Some(list) = dependents.get_mut(dependency.index) {
                list.push(index);
            }
        }
    }
    let mut reaching = vec![false; document.units.len()];
    let mut queue = VecDeque::new();
    for (index, unit) in document.units.iter().enumerate() {
        if path(unit) {
            reaching[index] = true;
            queue.push_back(index);
        }
    }
    while let Some(index) = queue.pop_front() {
        for &dependent in &dependents[index] {
            if !reaching[dependent] {
                reaching[dependent] = true;
                queue.push_back(dependent);
            }
        }
    }
    document
        .units
        .iter()
        .zip(reaching)
        .filter(|(unit, reaching)| *reaching && !path(unit))
        .map(|(unit, _)| unit.pkg_id.as_str())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(Box::from)
        .collect()
}

fn keys_with(
    units: &[u8],
    metadata: &Metadata,
    selected: &[Selected<'_>],
    [compiler_commit, host, target]: [&str; 3],
    local: bool,
) -> BTreeMap<Box<str>, Shared> {
    let Ok(document) = serde_json::from_slice::<Document>(units) else {
        return BTreeMap::new();
    };
    let mut keys = BTreeMap::new();
    for unit in selected {
        let mut matching = document.units.iter().enumerate().filter(|(_, candidate)| {
            candidate.pkg_id == unit.package_id
                && candidate.target.src_path == unit.target
                && candidate.mode == unit.mode
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
            local,
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
    local: bool,
) -> Option<Shared> {
    let mut path = false;
    // Each unit reached, with whether a library edge reaches it; one first
    // reached beneath a build script's compile is walked again when a
    // library edge reaches it later.
    let mut seen = BTreeMap::from([(root, true)]);
    let mut queue = VecDeque::from([(root, true)]);
    let mut sources = BTreeSet::new();
    let mut scripts = BTreeSet::new();
    while let Some((index, compared)) = queue.pop_front() {
        let unit = document.units.get(index)?;
        let source = match metadata.package(&unit.pkg_id)?.source() {
            PackageSource::Registry {
                checksum: Some(checksum),
                ..
            } => crate::extract::engine::hex(checksum),
            PackageSource::Git { oid, .. } => oid.to_string(),
            // A path package's files are checked before replay, not named
            // by the key; only the project keeps such a table.
            PackageSource::Path { .. } if local => {
                path = true;
                "path".to_owned()
            }
            PackageSource::Registry { checksum: None, .. } | PackageSource::Path { .. } => {
                return None;
            }
        };
        sources.insert((unit.pkg_id.as_str(), source));
        if compared && unit.mode == "run-custom-build" {
            scripts.insert(unit.pkg_id.as_str());
        }
        // A build script's compile depends on its package's
        // build-dependencies, which reach the crate only through that
        // script's outputs.
        let compared = compared
            && !(unit.mode == "build"
                && unit.target.kind.iter().any(|kind| kind == "custom-build"));
        for dependency in &unit.dependencies {
            match seen.get(&dependency.index) {
                Some(&walked) if walked || !compared => {}
                _ => {
                    seen.insert(dependency.index, compared);
                    queue.push_back((dependency.index, compared));
                }
            }
        }
    }
    let mut digest = Sha256::new();
    let mut frame = |bytes: &[u8]| {
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(bytes);
    };
    frame(if path {
        b"rawr.bindings.local/v1".as_slice()
    } else {
        b"rawr.bindings/v1".as_slice()
    });
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
        local: path,
        scripts: scripts.into_iter().map(Box::from).collect(),
        packages: sources
            .into_iter()
            .map(|(package, _)| Box::from(package))
            .collect(),
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
    const PM2: &str = "registry+https://index#pm2@1.0.0";
    const MACROS: &str = "registry+https://index#macros@1.0.0";

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

    /// `semver` uses `libc`, whose build script runs, and the procedural
    /// macros of `macros`, which use `pm2`, whose build script runs too;
    /// `local` uses `semver`.
    fn units() -> Vec<u8> {
        let unit = |pkg_id: &str, src: &str, mode: &str, dependencies: &[usize]| {
            let kind = if pkg_id == MACROS {
                "proc-macro"
            } else {
                "lib"
            };
            serde_json::json!({
                "pkg_id": pkg_id,
                "target": { "kind": [kind], "src_path": src },
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
                unit(SEMVER, "/registry/semver/src/lib.rs", "check", &[1, 6]),
                unit(LOCAL, "/project/local/src/lib.rs", "check", &[2]),
                unit(PM2, "/registry/pm2/build.rs", "run-custom-build", &[]),
                unit(PM2, "/registry/pm2/src/lib.rs", "build", &[4]),
                unit(MACROS, "/registry/macros/src/lib.rs", "build", &[5]),
            ]
        }))
        .unwrap()
    }

    fn selected(package_id: &'static str, target: &'static str) -> Selected<'static> {
        Selected {
            unit_key: target,
            package_id,
            target,
            mode: "check",
            features: &[],
        }
    }

    fn packages(libc: u8, pm2: u8) -> Metadata {
        Metadata::fixture_packages([
            registry(SEMVER, 1),
            registry(LIBC, libc),
            registry(PM2, pm2),
            registry(MACROS, 4),
            local(),
        ])
    }

    #[test]
    fn a_crate_from_registries_alone_is_keyed_by_its_closure_and_a_path_crate_is_not_shared() {
        let semver = selected(SEMVER, "/registry/semver/src/lib.rs");
        let path_crate = selected(LOCAL, "/project/local/src/lib.rs");
        let metadata = packages(2, 3);
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
        // A procedural macro's build-script output can change the API it
        // emits, so pm2's script belongs to the comparison too.
        assert_eq!(first.scripts.as_ref(), [LIBC.into(), PM2.into()]);
        // A dependency's new checksum, a procedural macro's among them, or
        // another toolchain, is another key.
        for changed in [packages(5, 3), packages(2, 5)] {
            let changed = keys(&units(), &changed, &[semver], "commit", "host", "host");
            assert_ne!(changed[SEMVER_TARGET].key, first.key);
        }
        let toolchain = keys(&units(), &metadata, &[semver], "other", "host", "host");
        assert_ne!(toolchain[SEMVER_TARGET].key, first.key);
        let again = keys(&units(), &metadata, &[semver], "commit", "host", "host");
        assert_eq!(again[SEMVER_TARGET], *first);
    }

    const SEMVER_TARGET: &str = "/registry/semver/src/lib.rs";

    /// `libc` is a library of the target and, with the same features, a
    /// host dependency of `pm2`'s build script, as a check build for a
    /// named target compiles it: the target's unit is the selected one, and
    /// it is shared (#471).
    #[test]
    fn a_library_also_built_for_a_host_script_is_keyed_by_its_target_unit() {
        let unit = |src: &str, mode: &str, dependencies: &[usize]| {
            serde_json::json!({
                "pkg_id": if src.contains("pm2") { PM2 } else { LIBC },
                "target": {
                    "kind": [if src.ends_with("build.rs") { "custom-build" } else { "lib" }],
                    "src_path": src,
                },
                "mode": mode,
                "features": [],
                "dependencies": dependencies
                    .iter()
                    .map(|index| serde_json::json!({ "index": index }))
                    .collect::<Vec<_>>(),
            })
        };
        const LIBC_TARGET: &str = "/registry/libc/src/lib.rs";
        let units = serde_json::to_vec(&serde_json::json!({
            "units": [
                unit(LIBC_TARGET, "check", &[]),
                unit(LIBC_TARGET, "build", &[]),
                unit("/registry/pm2/build.rs", "build", &[1]),
            ]
        }))
        .unwrap();
        let libc = Selected {
            unit_key: "libc",
            package_id: LIBC,
            target: LIBC_TARGET,
            mode: "check",
            features: &[],
        };
        let shared = keys(&units, &packages(2, 3), &[libc], "commit", "host", "host");
        assert!(shared.contains_key("libc"), "the target's unit is shared");
    }

    /// `semver` reaches the path package `local` (as a `[patch]` makes a
    /// registry crate do) through `libc`; `pm2` and `macros` reach none,
    /// and `local` is a path package itself.
    #[test]
    fn the_registry_packages_reaching_a_path_one_are_local_closures() {
        let unit = |pkg_id: &str, dependencies: &[usize]| {
            serde_json::json!({
                "pkg_id": pkg_id,
                "target": { "kind": ["lib"], "src_path": format!("/{pkg_id}/lib.rs") },
                "mode": "check",
                "features": [],
                "dependencies": dependencies
                    .iter()
                    .map(|index| serde_json::json!({ "index": index }))
                    .collect::<Vec<_>>(),
            })
        };
        let units = serde_json::to_vec(&serde_json::json!({
            "units": [
                unit(LOCAL, &[]),
                unit(LIBC, &[0]),
                unit(SEMVER, &[1, 3]),
                unit(PM2, &[]),
                unit(MACROS, &[3]),
            ]
        }))
        .unwrap();
        let local = super::local_closures(&units, &packages(2, 3));
        assert_eq!(local.as_ref(), [LIBC.into(), SEMVER.into()]);
    }

    /// `libc`'s build script depends on `semver`, which uses `pm2`, whose
    /// build script runs. Feature unification may give `pm2`'s script other
    /// outputs in another graph, and they reach `libc` only through its own
    /// script's outputs, so only that one is compared, until a library edge
    /// (`macros` through `extra`) reaches `pm2` too, later than the build
    /// script's dependencies do.
    #[test]
    fn a_package_reached_only_beneath_a_build_script_s_compile_is_not_compared() {
        const EXTRA: &str = "registry+https://index#extra@1.0.0";
        let unit = |pkg_id: &str, kind: &str, mode: &str, dependencies: &[usize]| {
            serde_json::json!({
                "pkg_id": pkg_id,
                "target": { "kind": [kind], "src_path": format!("/registry/{pkg_id}/{kind}.rs") },
                "mode": mode,
                "features": [],
                "dependencies": dependencies
                    .iter()
                    .map(|index| serde_json::json!({ "index": index }))
                    .collect::<Vec<_>>(),
            })
        };
        let graph = |libc: &[usize]| {
            serde_json::to_vec(&serde_json::json!({
                "units": [
                    unit(PM2, "custom-build", "run-custom-build", &[]),
                    unit(PM2, "lib", "build", &[0]),
                    unit(SEMVER, "lib", "build", &[1]),
                    unit(LIBC, "custom-build", "build", &[2]),
                    unit(LIBC, "custom-build", "run-custom-build", &[3]),
                    unit(LIBC, "lib", "check", libc),
                    unit(MACROS, "proc-macro", "build", &[7]),
                    unit(EXTRA, "lib", "build", &[8]),
                    unit(EXTRA, "lib", "build", &[1]),
                ]
            }))
            .unwrap()
        };
        let target = format!("/registry/{LIBC}/lib.rs");
        let libc = Selected {
            unit_key: "libc",
            package_id: LIBC,
            target: &target,
            mode: "check",
            features: &[],
        };
        let metadata = Metadata::fixture_packages([
            registry(SEMVER, 1),
            registry(LIBC, 2),
            registry(PM2, 3),
            registry(MACROS, 4),
            registry(EXTRA, 5),
        ]);
        let alone = keys(&graph(&[4]), &metadata, &[libc], "commit", "host", "host");
        assert_eq!(alone["libc"].scripts.as_ref(), [LIBC.into()]);
        assert!(alone["libc"].packages.contains(&PM2.into()));
        let reached = keys(
            &graph(&[4, 6]),
            &metadata,
            &[libc],
            "commit",
            "host",
            "host",
        );
        assert_eq!(reached["libc"].scripts.as_ref(), [LIBC.into(), PM2.into()]);
    }
}
