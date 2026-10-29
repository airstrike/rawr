//! Crate-specific input witnesses required before shared API replay (D640).
//!
//! These are projections of the completed input baseline, not another parser
//! for Cargo declarations or rustc dep-info. Generated products stay local;
//! their agreed outputs are retained separately by `bindings::Script`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::{build, proof, Allocation, Baseline};
use crate::cargo;

const SCHEMA: u32 = 1;

/// Current request authority, captured before any shared API is consumed.
pub(crate) struct Current {
    project: PathBuf,
    compiler: String,
    config: [u8; 32],
    projection: Vec<(String, Option<String>)>,
    environment: BTreeMap<String, String>,
    limits: cargo::Limits,
}

impl Current {
    #[cfg(test)]
    pub(crate) fn fixture() -> Self {
        Self {
            project: PathBuf::from("/fixture-project"),
            compiler: "fixture compiler".to_owned(),
            config: [0; 32],
            projection: Vec::new(),
            environment: BTreeMap::new(),
            limits: cargo::Limits::standard(),
        }
    }

    pub(crate) fn capture(
        project: &Path,
        config: &cargo::config::Provenance,
        limits: &cargo::Limits,
    ) -> Option<Self> {
        config.revalidate(limits, "shared binding replay").ok()?;
        Some(Self {
            project: project.canonicalize().ok()?,
            compiler: crate::runner::runtime::compiler_identity(project, config).ok()?,
            config: *config.universe_context().as_bytes(),
            projection: config
                .projection()
                .entries()
                .map(|(key, value)| (key.to_owned(), value.map(ToOwned::to_owned)))
                .collect(),
            environment: config
                .environment()
                .entries()
                .map(|(key, value)| (key.to_owned(), value.to_owned()))
                .collect(),
            limits: *limits,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Inputs {
    schema: u32,
    compiler: String,
    config: [u8; 32],
    projection: Vec<(String, Option<String>)>,
    paths: Vec<File>,
    environment: BTreeMap<String, Option<String>>,
    fingerprint: [u8; 32],
}

/// The source path and evaluation mode already established by a baseline.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct File {
    package: String,
    path: PathBuf,
    tree: bool,
}

/// A historical witness whose current request and filesystem agree.
pub(crate) struct Validated<'inputs> {
    inputs: &'inputs Inputs,
    filesystem: proof::Proof,
}

impl Validated<'_> {
    pub(crate) fn inputs(&self) -> &Inputs {
        self.inputs
    }

    pub(crate) fn identity(&self) -> Option<[u8; 32]> {
        Some(Sha256::digest(serde_json::to_vec(self.inputs).ok()?).into())
    }

    /// Keep the same evaluated file and directory witnesses for live reload.
    pub(crate) fn into_filesystem(self) -> proof::Proof {
        self.filesystem
    }
}

impl Inputs {
    /// Retain only inputs belonging to the selected immutable crate closure.
    /// A project source or ambiguous generated environment cannot become a
    /// shared proof. Its table remains usable by the normal local owner.
    pub(crate) fn capture(
        baseline: &Baseline,
        build: &build::Build,
        packages: &[String],
        current: &Current,
    ) -> Option<Self> {
        let selected = packages.iter().map(String::as_str).collect::<BTreeSet<_>>();
        if selected.is_empty()
            || selected.iter().any(|id| {
                !(id.starts_with("registry+") || id.starts_with("git+"))
                    || !build.evidence.packages.contains_key(id)
            })
        {
            return None;
        }
        let mut paths = BTreeSet::new();
        let mut environment = BTreeMap::new();
        for recipe in &baseline.recipes {
            match recipe {
                proof::Recipe::File {
                    package_id: Some(package),
                    path,
                    ..
                }
                | proof::Recipe::Path {
                    package_id: Some(package),
                    path,
                    ..
                } if selected.contains(package.as_ref()) => {
                    let generated = build
                        .evidence
                        .scripts
                        .iter()
                        .any(|script| path.starts_with(&script.out_dir));
                    if generated {
                        continue;
                    }
                    if path.starts_with(&current.project) {
                        return None;
                    }
                    paths.insert(File {
                        package: package.to_string(),
                        path: path.clone(),
                        tree: matches!(recipe, proof::Recipe::Path { .. }),
                    });
                }
                proof::Recipe::Environment {
                    package_id,
                    execution,
                    key,
                    value,
                } if selected.contains(package_id.as_ref()) => {
                    let declaration = build.evidence.scripts.iter().any(|script| {
                        script.package_id == *package_id
                            && script.out_dir.as_os_str()
                                == Path::new(execution.as_ref()).as_os_str()
                    });
                    // Compiler environment supplied by Cargo or the package's
                    // build script is an output, bound by its immutable package
                    // and Script evidence. Explicit rerun-if-env-changed is
                    // always an input, even if it has the same variable name.
                    let generated = !declaration
                        && (cargo_environment(key)
                            || build.evidence.scripts.iter().any(|script| {
                                script.package_id == *package_id
                                    && script.environment.iter().any(|(name, output)| {
                                        name == key && Some(output.as_ref()) == value.as_deref()
                                    })
                            }));
                    if generated {
                        continue;
                    }
                    if current.environment.get(key.as_ref()).map(String::as_str) != value.as_deref()
                    {
                        // Unknown Cargo/config injection has no ambient input
                        // proof. Do not guess how another project obtains it.
                        return None;
                    }
                    let value = value.as_deref().map(ToOwned::to_owned);
                    if let Some(previous) = environment.insert(key.to_string(), value.clone())
                        && previous != value
                    {
                        return None;
                    }
                }
                _ => {}
            }
        }
        // Default rerun membership has already been authenticated by Cargo.
        // Registry extraction is immutable and witnessed by its marker. A git
        // fallback uses the existing bounded tree evaluator so additions, not
        // only changes to previously listed files, also invalidate it.
        for entry in baseline.proof.stored_parts().1 {
            let (package, path, tree) = match entry {
                proof::Entry::Registry {
                    package_id, root, ..
                } if selected.contains(package_id.as_ref()) => (
                    package_id,
                    Path::new(root.as_ref()).join(".cargo-ok"),
                    false,
                ),
                proof::Entry::Git {
                    package_id, root, ..
                } if selected.contains(package_id.as_ref()) => {
                    (package_id, PathBuf::from(root.as_ref()), true)
                }
                _ => continue,
            };
            if path.starts_with(&current.project) {
                return None;
            }
            paths.insert(File {
                package: package.to_string(),
                path,
                tree,
            });
        }
        let paths = paths.into_iter().collect::<Vec<_>>();
        let fingerprint = fingerprint(&paths, &current.limits)?;
        Some(Self {
            schema: SCHEMA,
            compiler: current.compiler.clone(),
            config: current.config,
            projection: current.projection.clone(),
            paths,
            environment,
            fingerprint,
        })
    }

    pub(crate) fn validate<'inputs>(
        &'inputs self,
        current: &Current,
    ) -> Option<Validated<'inputs>> {
        if self.schema != SCHEMA
            || self.compiler != current.compiler
            || self.config != current.config
            || self.projection != current.projection
            || !self.environment.iter().all(|(key, expected)| {
                current.environment.get(key).map(String::as_str) == expected.as_deref()
            })
        {
            return None;
        }
        let filesystem = evaluate(&self.paths, &current.limits)?;
        (*filesystem.fingerprint().as_bytes() == self.fingerprint).then_some(Validated {
            inputs: self,
            filesystem,
        })
    }

    #[cfg(test)]
    pub(crate) fn fixture() -> Self {
        Self {
            schema: SCHEMA,
            compiler: "fixture compiler".to_owned(),
            config: [0; 32],
            projection: Vec::new(),
            paths: Vec::new(),
            environment: BTreeMap::new(),
            fingerprint: fingerprint(&[], &cargo::Limits::standard()).expect("empty input proof"),
        }
    }

    #[cfg(test)]
    pub(crate) fn fixture_paths(files: &[PathBuf], trees: &[PathBuf]) -> Self {
        let mut inputs = Self::fixture();
        inputs.paths = files
            .iter()
            .map(|path| (path, false))
            .chain(trees.iter().map(|path| (path, true)))
            .map(|(path, tree)| File {
                package: "registry+https://example.invalid/index#fixture@1.0.0".to_owned(),
                path: path.clone(),
                tree,
            })
            .collect();
        inputs.fingerprint =
            fingerprint(&inputs.paths, &cargo::Limits::standard()).expect("fixture paths evaluate");
        inputs
    }
}

fn fingerprint(paths: &[File], limits: &cargo::Limits) -> Option<[u8; 32]> {
    Some(*evaluate(paths, limits)?.fingerprint().as_bytes())
}

fn evaluate(paths: &[File], limits: &cargo::Limits) -> Option<proof::Proof> {
    let recipes = paths
        .iter()
        .map(|file| {
            if file.tree {
                proof::Recipe::Path {
                    role: proof::Role::DeclaredInput,
                    package_id: Some(file.package.as_str().into()),
                    path: file.path.clone(),
                }
            } else {
                proof::Recipe::File {
                    role: proof::Role::DependencySource,
                    package_id: Some(file.package.as_str().into()),
                    path: file.path.clone(),
                }
            }
        })
        .collect::<Vec<_>>();
    let mut allocation = Allocation::for_replay(limits);
    proof::evaluate(&recipes, false, limits, &mut allocation).ok()
}

/// Cargo owns these compile-time variables for every invocation. Their value
/// is derived from the immutable package/target or the captured OUT_DIR; an
/// unrelated ambient variable with the same name cannot override them.
fn cargo_environment(key: &str) -> bool {
    matches!(
        key,
        "OUT_DIR"
            | "CARGO_CRATE_NAME"
            | "CARGO_BIN_NAME"
            | "CARGO_MANIFEST_DIR"
            | "CARGO_MANIFEST_PATH"
            | "CARGO_PKG_VERSION"
            | "CARGO_PKG_VERSION_MAJOR"
            | "CARGO_PKG_VERSION_MINOR"
            | "CARGO_PKG_VERSION_PATCH"
            | "CARGO_PKG_VERSION_PRE"
            | "CARGO_PKG_AUTHORS"
            | "CARGO_PKG_NAME"
            | "CARGO_PKG_DESCRIPTION"
            | "CARGO_PKG_HOMEPAGE"
            | "CARGO_PKG_REPOSITORY"
            | "CARGO_PKG_LICENSE"
            | "CARGO_PKG_LICENSE_FILE"
            | "CARGO_PKG_RUST_VERSION"
            | "CARGO_PKG_README"
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{build, proof, Allocation, Baseline, Current, Inputs};

    static NEXT: AtomicUsize = AtomicUsize::new(0);
    const PACKAGE: &str = "registry+https://example.invalid/index#demo@1.0.0";

    fn fixture() -> (PathBuf, Baseline, build::Build, Current) {
        let root = std::env::temp_dir().join(format!(
            "rawr-binding-inputs-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(root.join("source")).unwrap();
        fs::create_dir_all(root.join("project/generated")).unwrap();
        let root = root.canonicalize().unwrap();
        let package = root.join("source");
        let out_dir = root.join("project/generated");
        fs::write(
            package.join("lib.rs"),
            "pub type Width = [u8; env!(\"WIDTH\").len()];",
        )
        .unwrap();
        fs::write(root.join("declared.txt"), "original").unwrap();
        fs::write(out_dir.join("generated.rs"), "// generated").unwrap();
        let mut current = Current::fixture();
        current.project = root.join("project");
        current
            .environment
            .insert("DECLARED_WIDTH".to_owned(), "small".to_owned());
        let recipes = vec![
            proof::Recipe::File {
                role: proof::Role::DependencySource,
                package_id: Some(PACKAGE.into()),
                path: package.join("lib.rs"),
            },
            proof::Recipe::File {
                role: proof::Role::DependencySource,
                package_id: Some(PACKAGE.into()),
                path: out_dir.join("generated.rs"),
            },
            proof::Recipe::Path {
                role: proof::Role::DeclaredInput,
                package_id: Some(PACKAGE.into()),
                path: root.join("declared.txt"),
            },
            proof::Recipe::Environment {
                package_id: PACKAGE.into(),
                execution: out_dir.to_str().unwrap().into(),
                key: "DECLARED_WIDTH".into(),
                value: Some("small".into()),
            },
            proof::Recipe::Environment {
                package_id: PACKAGE.into(),
                execution: "library.d".into(),
                key: "WIDTH".into(),
                value: Some("generated width".into()),
            },
        ]
        .into_boxed_slice();
        let mut allocation = Allocation::for_replay(&current.limits);
        let proof = proof::evaluate(&recipes, false, &current.limits, &mut allocation).unwrap();
        let baseline = Baseline {
            proof,
            recipes,
            recipe_allocation: super::super::Usage { items: 0, bytes: 0 },
            allocation,
            universe_build: None,
            package_replay: None,
        };
        let build = build::Build {
            evidence: build::Evidence {
                usage: super::super::Usage { items: 0, bytes: 0 },
                packages: [(
                    PACKAGE.into(),
                    build::Package {
                        manifest: package.join("Cargo.toml"),
                        root: package,
                        build_script: None,
                    },
                )]
                .into(),
                scripts: Box::from([build::Script {
                    compiler: None,
                    package_id: PACKAGE.into(),
                    out_dir,
                    cfg: Box::default(),
                    environment: Box::from([("WIDTH".into(), "generated width".into())]),
                }]),
                artifacts: Box::default(),
                environment: Box::default(),
            },
            package_files: BTreeMap::new(),
        };
        (root, baseline, build, current)
    }

    #[test]
    fn declared_inputs_are_rechecked_before_api_replay_across_projects() {
        let (root, baseline, build, mut current) = fixture();
        let inputs = Inputs::capture(&baseline, &build, &[PACKAGE.to_owned()], &current).unwrap();
        assert!(inputs.validate(&current).is_some());
        // Relocating the consuming project or removing the publisher's build
        // products does not change the crate's input proof.
        current.project = root.join("second-project");
        fs::remove_dir_all(root.join("project/generated")).unwrap();
        assert!(inputs.validate(&current).is_some());
        current
            .environment
            .insert("DECLARED_WIDTH".to_owned(), "large".to_owned());
        assert!(inputs.validate(&current).is_none());
        current
            .environment
            .insert("DECLARED_WIDTH".to_owned(), "small".to_owned());
        fs::write(root.join("declared.txt"), "changed").unwrap();
        assert!(inputs.validate(&current).is_none());
        fs::write(root.join("declared.txt"), "original").unwrap();
        assert!(inputs.validate(&current).is_some());
        current.compiler.push_str(" changed");
        assert!(inputs.validate(&current).is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_sources_and_unknown_compiler_environment_cannot_publish() {
        let (root, mut baseline, build, mut current) = fixture();
        current.project = root.clone();
        assert!(Inputs::capture(&baseline, &build, &[PACKAGE.to_owned()], &current).is_none());
        current.project = root.join("project");
        let proof::Recipe::Environment { key, .. } = &mut baseline.recipes[4] else {
            panic!("fixture compiler environment");
        };
        *key = "UNEXPLAINED_OUTPUT".into();
        assert!(Inputs::capture(&baseline, &build, &[PACKAGE.to_owned()], &current).is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn declared_directory_additions_and_absent_inputs_invalidate() {
        let (root, mut baseline, build, current) = fixture();
        let proof::Recipe::Path { path, .. } = &mut baseline.recipes[2] else {
            panic!("fixture declared path");
        };
        *path = root.join("missing");
        let inputs = Inputs::capture(&baseline, &build, &[PACKAGE.to_owned()], &current).unwrap();
        assert!(inputs.validate(&current).is_some());
        fs::create_dir(root.join("missing")).unwrap();
        assert!(inputs.validate(&current).is_none());
        let inputs = Inputs::capture(&baseline, &build, &[PACKAGE.to_owned()], &current).unwrap();
        fs::write(root.join("missing/new.txt"), "new").unwrap();
        assert!(inputs.validate(&current).is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(feature = "rustc-driver")]
    #[test]
    fn shared_proof_keeps_exact_missing_and_tree_watches() {
        let (root, _, _, current) = fixture();
        let file = root.join("declared.txt");
        let missing = root.join("not-created");
        let tree = root.join("source");
        let inputs = Inputs::fixture_paths(
            std::slice::from_ref(&file),
            &[missing.clone(), tree.clone()],
        );
        let filesystem = inputs
            .validate(&Current::fixture())
            .unwrap()
            .into_filesystem();
        let shared = proof::Proof::shared(Box::default(), [9; 32], [&filesystem]);
        let watches = shared.watch().collect::<Vec<_>>();
        assert!(watches.contains(&proof::Watch::Exact(file)));
        assert!(watches.contains(&proof::Watch::Exact(missing)));
        assert!(watches.contains(&proof::Watch::Tree(tree)));
        assert!(!watches.iter().any(|watch| match watch {
            proof::Watch::Exact(path) | proof::Watch::Tree(path) =>
                path.starts_with(&current.project),
        }));
        fs::remove_dir_all(root).unwrap();
    }
}
