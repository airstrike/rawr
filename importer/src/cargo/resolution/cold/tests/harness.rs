//! Synthetic Cargo documents traverse the real reserved Request preparation.

use std::fs;
use std::path::Path;

use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};

use super::{expected_reply, Command, Fixture};
use crate::cargo::{self, config, resolution, unit, universe, Limits, Request};
use crate::process::test_launch;

mod built;
mod complete;
mod migration;
mod package;

const SEED: &[u8] = b"version = 4\n\n[[package]]\nname = \"rawr-iced\"\nversion = \"0.1.0\"\n";

/// The one crates.io source every registry package in Gallery's lock names.
const REGISTRY: &str = "registry+https://github.com/rust-lang/crates.io-index";

/// A crates.io-shaped package whose manifest exists, so metadata canonicalizes it.
fn registry_package(registry: &Path, index: usize) -> Value {
    let name = format!("gallery-dep-{index:03}");
    let version = format!("1.{index}.0");
    let checksum = format!("{:x}", Sha256::digest(name.as_bytes()));
    let root = registry.join(format!("{name}-{version}"));
    fs::create_dir_all(root.join("src")).expect("registry package root creates");
    fs::write(
        root.join("Cargo.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\nedition = \"2021\"\n"),
    )
    .expect("registry package manifest writes");
    fs::write(root.join("src/lib.rs"), b"// synthetic registry crate\n")
        .expect("registry package source writes");
    json!({
        "id": format!("{REGISTRY}#{name}@{version}"),
        "name": name, "version": version, "source": REGISTRY, "checksum": checksum,
        "manifest_path": root.join("Cargo.toml"), "dependencies": [],
        "targets": [{"kind": ["lib"], "crate_types": ["lib"],
            "name": name.replace('-', "_"), "src_path": root.join("src/lib.rs"), "edition": "2021"}],
    })
}

/// Acyclic forward edges near Gallery's density: about two per package, with
/// roughly two thirds of the packages depending on anything at all.
fn registry_edges(index: usize, count: usize) -> Vec<usize> {
    [
        (3, 0, 1),
        (3, 1, 2),
        (2, 0, 5),
        (3, 0, 17),
        (4, 0, 29),
        (5, 0, 61),
    ]
    .into_iter()
    .filter(|(modulus, remainder, step)| index % modulus == *remainder && index + step < count)
    .map(|(_, _, step)| index + step)
    .collect()
}

/// Which lock a rendering stands for.
#[derive(Clone, Copy)]
pub(super) enum LockScope {
    /// Everything Cargo resolved, as the final-form lock the harness writes.
    Final,
    /// The pinned universe: the protected package and its registry closure,
    /// without the harness root or the request's path dependencies.
    Seed,
}

/// Render metadata's resolve graph as Cargo writes a lock: sorted packages,
/// registry source and checksum where present, and dependency names.
pub(super) fn render_lock(metadata: &[u8], scope: LockScope) -> Vec<u8> {
    let metadata: Value = serde_json::from_slice(metadata).expect("fixture metadata parses");
    let packages = metadata["packages"]
        .as_array()
        .expect("fixture packages exist");
    let find = |id: &Value| {
        packages
            .iter()
            .find(|package| &package["id"] == id)
            .expect("resolved package exists")
    };
    let name = |id: &Value| {
        find(id)["name"]
            .as_str()
            .expect("package name exists")
            .to_owned()
    };
    let mut rows: Vec<_> = metadata["resolve"]["nodes"]
        .as_array()
        .expect("resolve nodes exist")
        .iter()
        .map(|node| {
            let package = find(&node["id"]);
            let mut dependencies: Vec<_> = node["dependencies"]
                .as_array()
                .expect("resolved dependencies exist")
                .iter()
                .map(name)
                .collect();
            dependencies.sort();
            let text = |field: &str| package[field].as_str().map(str::to_owned);
            (
                text("name").expect("package name exists"),
                text("version").expect("package version exists"),
                text("source"),
                text("checksum"),
                dependencies,
            )
        })
        .filter(|(name, _, source, _, _)| match scope {
            LockScope::Final => true,
            LockScope::Seed => name == "rawr-iced" || source.is_some(),
        })
        .collect();
    rows.sort();
    let mut lock = String::from("version = 4\n");
    for (name, version, source, checksum, dependencies) in rows {
        lock.push_str(&format!(
            "\n[[package]]\nname = \"{name}\"\nversion = \"{version}\"\n"
        ));
        if let Some(source) = source {
            lock.push_str(&format!("source = \"{source}\"\n"));
        }
        if let Some(checksum) = checksum {
            lock.push_str(&format!("checksum = \"{checksum}\"\n"));
        }
        if !dependencies.is_empty() {
            lock.push_str("dependencies = [\n");
            for dependency in dependencies {
                lock.push_str(&format!(" \"{dependency}\",\n"));
            }
            lock.push_str("]\n");
        }
    }
    lock.into_bytes()
}

/// The seeded universe lock for this metadata, retained for the fixture
/// definition's lifetime like the pinned product seed.
pub(super) fn seed_lock(metadata: &[u8]) -> &'static [u8] {
    Box::leak(render_lock(metadata, LockScope::Seed).into_boxed_slice())
}

fn package(root: &Path, name: &str, version: &str) -> Value {
    let root_text = root
        .to_str()
        .expect("fixture roots are Unicode")
        .replace('{', "%7B")
        .replace('}', "%7D");
    json!({
        "id": format!("path+file://{root_text}#{name}@{version}"),
        "name": name, "version": version, "source": null, "checksum": null,
        "manifest_path": root.join("Cargo.toml"), "dependencies": [],
        "targets": [{"kind": ["lib"], "crate_types": ["lib"],
            "name": name.replace('-', "_"), "src_path": root.join("src/lib.rs"), "edition": "2024"}],
    })
}

fn documents(request: &Request, paths: &resolution::layout::Cargo) -> (Vec<u8>, Vec<u8>) {
    documents_scaled(request, paths, 0)
}

/// The fixture documents plus `registry` crates.io packages reachable from the
/// protected `rawr-iced` unit, so metadata, units, seed and final lock all
/// carry a Gallery-shaped closure.
fn documents_scaled(
    request: &Request,
    paths: &resolution::layout::Cargo,
    registry: usize,
) -> (Vec<u8>, Vec<u8>) {
    let mut packages = vec![package(paths.root(), "rawr-resolution-harness", "0.0.0")];
    let mut nodes =
        vec![json!({"id": packages[0]["id"], "dependencies": [], "deps": [], "features": []})];
    let mut root_dependencies = Vec::new();
    for dependency in &request.dependencies {
        let config::Source::Path(root) = dependency.source() else {
            panic!("fixture dependencies are real path inputs");
        };
        let manifest: toml_edit::DocumentMut = fs::read_to_string(root.join("Cargo.toml"))
            .expect("actual dependency manifest reads")
            .parse()
            .expect("manifest parses");
        let name = manifest["package"]["name"]
            .as_str()
            .expect("package name exists");
        let version = manifest["package"]["version"]
            .as_str()
            .expect("package version exists");
        let value = package(root, name, version);
        let id = value["id"].clone();
        packages[0]["dependencies"].as_array_mut().expect("declarations array")
            .push(json!({"name": name, "rename": dependency.alias(), "path": root,
                "uses_default_features": dependency.default_features(), "features": dependency.features(),
                "kind": null, "target": null, "optional": false, "req": "*", "registry": null}));
        nodes[0]["deps"].as_array_mut().expect("root deps array")
            .push(json!({"name": dependency.alias(), "pkg": id, "dep_kinds": [{"kind": null, "target": null}]}));
        nodes[0]["dependencies"]
            .as_array_mut()
            .expect("root IDs array")
            .push(id.clone());
        root_dependencies.push(
            json!({"index": packages.len(), "extern_crate_name": dependency.alias(),
            "public": false, "noprelude": false}),
        );
        nodes.push(
            json!({"id": id, "dependencies": [], "deps": [], "features": dependency.features()}),
        );
        packages.push(value);
    }
    let fixture_root = request
        .project_root
        .parent()
        .expect("fixture project sits in the fixture root");
    let protected_root = fixture_root.join("rawr-iced");
    fs::create_dir_all(protected_root.join("src")).expect("protected package root creates");
    fs::write(
        protected_root.join("Cargo.toml"),
        "[package]\nname = \"rawr-iced\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("protected package manifest writes");
    fs::write(
        protected_root.join("src/lib.rs"),
        b"// synthetic protected crate\n",
    )
    .expect("protected package source writes");
    let protected = package(&protected_root, "rawr-iced", "0.1.0");
    let protected_index = packages.len();
    nodes.push(json!({"id": protected["id"], "dependencies": [], "deps": [], "features": []}));
    packages.push(protected);
    // Registry packages follow the protected package; package and unit indices
    // coincide, so one edge list serves resolve nodes and unit dependencies.
    let first = packages.len();
    let registry_root = fixture_root.join("registry");
    for index in 0..registry {
        let value = registry_package(&registry_root, index);
        nodes.push(json!({"id": value["id"], "dependencies": [], "deps": [], "features": []}));
        packages.push(value);
    }
    let mut unit_dependencies = vec![Vec::new(); packages.len()];
    unit_dependencies[0] = root_dependencies;
    let mut link = |from: usize, to: usize| {
        let target = &packages[to];
        let extern_name = target["targets"][0]["name"].clone();
        nodes[from]["dependencies"]
            .as_array_mut()
            .expect("resolved IDs array")
            .push(target["id"].clone());
        nodes[from]["deps"].as_array_mut().expect("resolved deps array").push(json!({
            "name": extern_name, "pkg": target["id"], "dep_kinds": [{"kind": null, "target": null}]}));
        unit_dependencies[from].push(json!({"index": to, "extern_crate_name": extern_name,
            "public": false, "noprelude": false}));
    };
    for entry in 0..registry.min(8) {
        link(protected_index, first + entry);
    }
    for index in 0..registry {
        for edge in registry_edges(index, registry) {
            link(first + index, first + edge);
        }
    }
    let units: Vec<_> = packages
        .iter()
        .zip(&nodes)
        .zip(unit_dependencies)
        .map(|((package, node), dependencies)| {
            json!({
                "pkg_id": package["id"], "target": package["targets"][0],
                "profile": {"name": "dev", "opt_level": "0", "debuginfo": 2,
                    "debug_assertions": true, "overflow_checks": true},
                "platform": request.target, "mode": "build", "features": node["features"],
                "dependencies": dependencies, "is_std": false,
            })
        })
        .collect();
    let root = packages[0]["id"].clone();
    let metadata = json!({"packages": packages, "workspace_members": [root], "workspace_default_members": [root],
        "resolve": {"root": root, "nodes": nodes}, "workspace_root": paths.root(),
        "target_directory": paths.target(), "build_directory": paths.build(), "version": 1, "metadata": null});
    (
        serde_json::to_vec(&metadata).expect("metadata encodes"),
        serde_json::to_vec(&json!({"version": 1, "units": units, "roots": [0]}))
            .expect("units encode"),
    )
}

fn bind_universe(request: &mut Request, cached: &cargo::CachedToolchain) {
    bind_universe_seeded(request, cached, SEED);
}

fn bind_universe_seeded(
    request: &mut Request,
    cached: &cargo::CachedToolchain,
    seed: &'static [u8],
) {
    let definition = universe::Definition::fixture_authenticated(seed).fixture_context(
        &request.target,
        &request.profile,
        cached.attestation.value().proof(),
    );
    request.universe = Some(universe::Catalog::new(definition));
}

#[test]
fn fresh_request_uses_one_reserved_sequence_without_clearing_targets() {
    let mut fixture = Fixture::open();
    let request = &mut fixture.request;
    Fixture::reserve_parts(&fixture.context, &fixture.store, |layout, native| {
        // The request owns its real manifest/config/internal-source authority;
        // the script supplies observations, never private parsed proof fields.
        let cached = cargo::CachedToolchain {
            context: fixture.context.clone(),
            attestation: fixture
                .context
                .cached_toolchain()
                .expect("current fixture attestation"),
        };
        let (final_bytes, units) = documents(request, layout.final_form());
        bind_universe(request, &cached);
        let (provisional_bytes, _) = documents(request, layout.provisional());
        let candidate_bytes = final_bytes;
        let source = layout.provisional().source().to_path_buf();
        let provisional_manifest = layout.provisional().manifest().to_path_buf();
        let final_lock = layout.final_form().lock().to_path_buf();
        let final_manifest = layout.final_form().manifest().to_path_buf();
        let steps = vec![
            expected_reply(
                request,
                native,
                layout.provisional(),
                Command::Metadata(false),
                true,
                move |_| {
                    assert_eq!(
                        fs::read(source).expect("provisional source exists"),
                        cargo::HARNESS_SOURCE.as_bytes()
                    );
                    let manifest = fs::read_to_string(provisional_manifest)
                        .expect("provisional manifest exists");
                    assert!(!manifest.contains("[profile.dev.package."));
                    test_launch::Reply::stdout(provisional_bytes)
                },
            ),
            expected_reply(
                request,
                native,
                layout.final_form(),
                Command::Metadata(false),
                true,
                move |_| {
                    assert_eq!(fs::read(final_lock).expect("seed lock exists"), SEED);
                    let manifest =
                        fs::read_to_string(final_manifest).expect("final manifest exists");
                    assert!(manifest.contains("[profile.dev.package."));
                    test_launch::Reply::stdout(candidate_bytes)
                },
            ),
            expected_reply(
                request,
                native,
                layout.final_form(),
                Command::Units,
                true,
                move |_| test_launch::Reply::stdout(units),
            ),
        ];
        let sentinel = layout.final_form().target().join("retained-sentinel");
        fs::write(&sentinel, b"unchanged").expect("preexisting output writes");
        let harness = super::script(layout, steps)
            .run(|| {
                request.prepare_fresh_harness_in(
                    layout,
                    &cached,
                    &Limits::standard(),
                    &Limits::standard(),
                    unit::Allocation::begin(&Limits::standard()),
                )
            })
            .expect("only three direct commands occur")?;
        assert_eq!(harness.harness_root, layout.final_form().root());
        assert_eq!(harness.command_target, layout.final_form().target());
        assert!(harness.dag.is_none());
        assert!(harness.universe.is_some());
        assert_eq!(fs::read(sentinel).expect("sentinel survives"), b"unchanged");
        assert_eq!(
            fs::read(layout.extraction().source()).expect("probe source publishes"),
            b"#![no_std]\npub struct RawrMetadataProbe;\n"
        );
        assert!(
            harness.documents.is_some(),
            "cold preparation owns both exact documents"
        );
        drop(harness);
        Ok(())
    });
    fixture.retire();
}

#[test]
fn reserved_cohort_preserves_legacy_identity_and_binds_config_contents() {
    let fixture = Fixture::open();
    fixture.reserve(|layout, _| {
        let request = &fixture.request;
        let legacy = cargo::final_command_cohort(
            request.toolchain.schemas().graph(),
            &request.target,
            &request.profile,
            request.parallelism,
            request.config.explicit_configs(),
        )?;
        let current = super::super::cohort(request, layout.paths())?;
        assert_ne!(legacy, current);
        let changed = cargo::command_cohort(
            request.toolchain.schemas().graph(),
            &request.target,
            &request.profile,
            request.parallelism,
            || super::super::prefix(request, Path::new("$RAWR_FINAL_BUILD_CONFIG")),
            Some(b"[build]\nbuild-dir = \"build/changed\"\n"),
        )?;
        assert_ne!(current, changed);
        Ok(())
    });
    fixture.retire();
}

#[test]
fn fresh_preparation_materializes_the_internal_profile_recipe() {
    let mut fixture = Fixture::open();
    let request = &mut fixture.request;
    Fixture::reserve_parts(&fixture.context, &fixture.store, |layout, native| {
        let cached = cargo::CachedToolchain {
            context: fixture.context.clone(),
            attestation: fixture
                .context
                .cached_toolchain()
                .expect("fixture attestation"),
        };
        let (metadata, units) = documents(request, layout.final_form());
        bind_universe(request, &cached);
        let steps = built::fresh_steps(request, native, layout, &metadata, &units);
        let harness = super::script(layout, steps)
            .run(|| {
                request.prepare_fresh_harness_in(
                    layout,
                    &cached,
                    &Limits::standard(),
                    &Limits::standard(),
                    unit::Allocation::begin(&Limits::standard()),
                )
            })
            .expect("preparation launches every candidate child")?;
        assert!(harness.universe.is_some());
        drop(harness);
        let manifest = fs::read_to_string(layout.final_form().manifest())
            .expect("the admitted recipe materializes the final manifest");
        assert!(manifest.contains("[profile.dev.package."));
        Ok(())
    });
    fixture.retire();
}

#[test]
fn provisional_decode_failure_releases_the_shared_checkpoint() {
    let fixture = Fixture::open();
    fixture.reserve(|layout, native| {
        let request = &fixture.request;
        let limits = Limits::standard();
        let definition = universe::Definition::fixture_authenticated(SEED);
        let production = native.production();
        let paths = layout.final_form();
        let mut epoch = unit::Allocation::begin(&limits);
        let steps = vec![expected_reply(
            request,
            native,
            layout.provisional(),
            Command::Metadata(false),
            true,
            |_| test_launch::Reply::stdout(b"{".to_vec()),
        )];
        let result = super::script(layout, steps)
            .run(|| {
                request.materialize_fresh(
                    &limits,
                    &mut epoch,
                    &definition,
                    (paths.manifest(), paths.source(), paths.lock()),
                    |phase| match phase {
                        cargo::MetadataPhase::Provisional => super::super::provisional_metadata(
                            &production,
                            request,
                            layout,
                            &limits,
                        ),
                        cargo::MetadataPhase::Candidate => {
                            panic!("a rejected provisional observation cannot advance")
                        }
                    },
                    |bytes, epoch| {
                        unit::Metadata::parse_in(
                            bytes,
                            &request.dependencies,
                            &limits,
                            request.toolchain.schemas().graph(),
                            layout.provisional(),
                            epoch,
                        )
                    },
                )
            })
            .expect("only provisional metadata runs");
        assert!(result.is_err());
        let mut checkpoint = epoch
            .begin_provisional()
            .expect("failed scope was released");
        epoch
            .release_provisional(&mut checkpoint)
            .expect("original epoch remains usable");
        assert!(!paths.manifest().exists());
        Ok(())
    });
    fixture.retire();
}
