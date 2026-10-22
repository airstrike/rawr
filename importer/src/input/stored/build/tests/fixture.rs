use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::json;

use crate::cargo::{toolchain, unit, Limits};
use crate::input::declaration::Declarations;
use crate::input::dependency::Dependency;
use crate::input::stored::Recipe;
use crate::input::{artifact, build, package, proof, script, Allocation, Usage};

pub(super) struct Fixture {
    pub(super) base: PathBuf,
    pub(super) home: PathBuf,
    pub(super) root: PathBuf,
    pub(super) id: String,
    pub(super) build: build::Retained,
    pub(super) metadata: unit::Metadata,
}

impl Fixture {
    pub(super) fn open(registry: bool, fallback: bool) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let base = std::env::temp_dir().join(format!(
            "rawr-stored-build-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(&base).expect("fixture base");
        let base = fs::canonicalize(base).expect("canonical fixture base");
        let home = base.join("home");
        let root = if registry {
            home.join("registry/src/fixture/package-1.0.0")
        } else {
            base.join("package")
        };
        fs::create_dir_all(&root).expect("package root");
        fs::create_dir_all(&home).expect("Cargo home");
        fs::write(
            root.join("Cargo.toml"),
            b"[package]\nname='package'\nversion='1.0.0'\n",
        )
        .expect("package manifest");
        fs::write(root.join(".cargo-ok"), b"{\"v\":1}").expect("registry marker");
        fs::write(root.join("source é.rs"), b"pub fn source() {}\n").expect("first source");
        fs::write(root.join("second.rs"), b"pub fn second() {}\n").expect("second source");
        fs::create_dir(root.join("declared")).expect("declared directory");
        fs::create_dir(root.join("declared/empty")).expect("declared empty child");
        fs::write(root.join("declared/data"), b"input").expect("declared file");
        let source = if registry {
            "registry+https://fixture.invalid/index"
        } else {
            "path+file:///fixture"
        };
        let id = format!("{source}#package@1.0.0");
        let source = if registry {
            unit::PackageSource::Registry {
                cargo_id: source.into(),
                checksum: Some([0x44; 32]),
            }
        } else {
            unit::PackageSource::Path {
                cargo_id: source.into(),
            }
        };
        let package = unit::Package::fixture(&id, "package", "1.0.0", root.clone(), source);
        let metadata = unit::Metadata::fixture_packages([package]);
        let deps = base.join("target/deps");
        fs::create_dir_all(&deps).expect("artifact directory");
        let mut messages = Vec::new();
        for suffix in ["a", "b"] {
            let output = deps.join(format!("libpackage-{suffix}.rlib"));
            fs::write(&output, b"artifact").expect("artifact file");
            fs::write(deps.join(format!("package-{suffix}.d")), b"target: source\\ \xc3\xa9.rs second.rs source\\ \xc3\xa9.rs\n# env-dep:DEP_SET=value=tail\n# env-dep:DEP_EMPTY=\n# env-dep:DEP_UNSET\n")
                .expect("dep-info source");
            messages.push(json!({
                "reason":"compiler-artifact", "package_id":id,
                "manifest_path":root.join("Cargo.toml"),
                "target":{"kind":["lib"],"crate_types":["lib"],"name":"package","src_path":root.join("source é.rs")},
                "features":[],
                "profile":{"opt_level":"0","debuginfo":2,"debug_assertions":true,"overflow_checks":true,"test":false},
                "fresh":false,"filenames":[output],"executable":null,
            }));
        }
        for suffix in ["a", "b"] {
            let execution = base.join(format!("target/build/{suffix}"));
            let out = execution.join("out");
            fs::create_dir_all(out.join("empty")).expect("output directory");
            fs::write(out.join("generated"), b"generated source").expect("generated file");
            let saved = if fallback {
                b"cargo:rustc-cfg=fixture\n".as_slice()
            } else {
                b"cargo:rerun-if-changed=declared\ncargo:rerun-if-changed=absent\ncargo:rerun-if-changed=declared\ncargo:rerun-if-env-changed=SET\ncargo:rerun-if-env-changed=EMPTY\ncargo:rerun-if-env-changed=MISSING\ncargo:rerun-if-env-changed=SET\ncargo:rustc-cfg=fixture\n".as_slice()
            };
            fs::write(execution.join("output"), saved).expect("saved output");
            messages.push(json!({"reason":"build-script-executed", "package_id":id,
                "out_dir":out,"cfgs":["zeta","alpha"],"env":[["OUTPUT","value"],["ZERO",""]]}));
        }
        messages.push(json!({"reason":"build-finished","success":true}));
        let mut bytes = Vec::new();
        for message in messages {
            serde_json::to_writer(&mut bytes, &message).expect("Cargo fixture JSON");
            bytes.push(b'\n');
        }
        let build = build::Build::parse(
            &bytes,
            Box::from([
                (OsString::from("SET"), OsString::from("sealed")),
                (OsString::from("EMPTY"), OsString::new()),
            ]),
            toolchain::supported_cargo_commit(),
            &Limits::standard(),
        )
        .expect("actual retained Build parser");
        Self {
            base,
            home,
            root,
            id,
            build: build::Retained { build },
            metadata,
        }
    }

    /// Retain fixtures through the actual cold recipe constructors and shared
    /// emitters, then use the legacy evaluator as the expected-entry oracle.
    /// This is section coverage; the parent cold fixture exercises Baseline.
    pub(super) fn cold(&self) -> (Vec<Recipe>, Box<[proof::Entry]>) {
        let limits = Limits::standard();
        let mut allocation = Allocation::for_proof(
            Usage {
                items: 7,
                bytes: 31,
            },
            &limits,
        );
        let mut recipes = Vec::new();
        let evidence = &self.build.build().evidence;
        for artifact in &evidence.artifacts {
            let package = evidence
                .packages
                .get(&artifact.package_id)
                .expect("artifact package");
            let directories = evidence
                .scripts
                .iter()
                .filter(|script| script.package_id == artifact.package_id)
                .map(|script| script.out_dir.clone())
                .collect::<Vec<_>>();
            recipes.push(
                proof::Recipe::dep_info(
                    &artifact.package_id,
                    &artifact.dep_info,
                    &package.root,
                    &directories,
                    &mut allocation,
                )
                .expect("cold header"),
            );
            let parsed = Dependency::read(
                toolchain::supported_cargo_commit(),
                &artifact.dep_info,
                &package.root,
                &limits,
                &mut allocation,
            )
            .expect("cold dep-info");
            artifact::derived(
                &parsed,
                &artifact.package_id,
                &artifact.dep_info,
                &mut allocation,
                |value, allocation| {
                    recipes.push(value.retain(allocation)?);
                    Ok(())
                },
            )
            .expect("cold artifact producer");
        }
        let mut fallback = BTreeSet::new();
        for script in &evidence.scripts {
            let package = evidence
                .packages
                .get(&script.package_id)
                .expect("script package");
            let declarations = Declarations::read_accounted(
                toolchain::supported_cargo_commit(),
                &script.package_id,
                &script.out_dir,
                &limits,
                &mut allocation,
            )
            .expect("cold declarations");
            let execution = script::declarations(
                script,
                package,
                &declarations,
                false,
                &evidence.environment,
                &mut allocation,
                |value, allocation| {
                    recipes.push(value.retain(allocation)?);
                    Ok(())
                },
            )
            .expect("cold declaration producer");
            if script::fallback(&declarations) && fallback.insert(script.package_id.as_ref()) {
                let package = self
                    .metadata
                    .package(&script.package_id)
                    .expect("fixture package");
                let registry = package::Set::capture_registry(
                    toolchain::supported_cargo_commit(),
                    package,
                    &self.home,
                    Usage { items: 0, bytes: 0 },
                    &limits,
                )
                .expect("actual cold Registry capture");
                recipes.push(proof::Recipe::Package(registry));
            }
            script::outputs(script, execution, &mut allocation, |value, allocation| {
                recipes.push(value.retain(allocation)?);
                Ok(())
            })
            .expect("cold output producer");
        }
        let expected = proof::evaluate(&recipes, false, &limits, &mut allocation)
            .expect("actual cold entry evaluator");
        let (_, entries, _) = expected.into_stored_parts();
        let recipes = recipes
            .into_iter()
            .map(|recipe| Recipe::capture(recipe).expect("historical recipe capture"))
            .collect();
        (recipes, entries)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}
