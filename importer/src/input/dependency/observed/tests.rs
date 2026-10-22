use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{acquire, read};
use crate::cargo::{toolchain, unit, Limits};
use crate::input::build;
use crate::input::dependency::Dependency;
use crate::input::stored::{self, observe, Budget, Recipe};
use crate::input::{Allocation, Usage};
use crate::node;

const PACKAGE: &str = "path+file:///fixture#package@1.0.0";
const PRIOR: Usage = Usage {
    items: 7,
    bytes: 31,
};

struct Fixture {
    root: PathBuf,
    path: PathBuf,
    recipe: Recipe,
    build: build::Retained,
}

impl Fixture {
    fn open() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "rawr-dependency-observed-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("build/a/out")).expect("first fixture output");
        fs::create_dir_all(root.join("build/b/out")).expect("second fixture output");
        let root = fs::canonicalize(root).expect("canonical fixture root");
        fs::write(root.join("a.rs"), b"pub fn a() {}\n").expect("first source");
        fs::write(root.join("b file.rs"), b"pub fn b() {}\n").expect("escaped source");
        let path = root.join("target.d");
        let directories = [root.join("build/a/out"), root.join("build/b/out")];
        let recipe = Recipe::DepInfo {
            package_id: PACKAGE.into(),
            path: path.clone(),
            package_root: root.clone(),
            out_directories: directories.to_vec().into_boxed_slice(),
        };
        let build = build::Retained {
            build: build::Build {
                evidence: build::Evidence {
                    usage: Usage {
                        items: 123,
                        bytes: 456,
                    },
                    packages: BTreeMap::from([(
                        PACKAGE.into(),
                        build::Package {
                            manifest: root.join("Cargo.toml"),
                            root: root.clone(),
                            build_script: Some(root.join("build.rs")),
                        },
                    )])
                    .into(),
                    scripts: directories
                        .into_iter()
                        .map(|out_dir| build::Script {
                            compiler: None,
                            package_id: PACKAGE.into(),
                            out_dir,
                            cfg: Box::default(),
                            environment: Box::default(),
                        })
                        .collect(),
                    artifacts: Box::from([
                        artifact(&root, &path, false),
                        artifact(&root, &path, true),
                    ]),
                    environment: Box::default(),
                },
                package_files: BTreeMap::new(),
            },
        };
        Self {
            root,
            path,
            recipe,
            build,
        }
    }

    fn write(&self, text: &str) {
        fs::write(&self.path, text).expect("fixture dep-info");
    }

    fn observe(
        &self,
        allocation: &mut Allocation,
        graph: &node::GraphBudget,
    ) -> stored::Result<Dependency> {
        read(
            self.recipe.view(),
            &self.build,
            &self.root,
            &mut Budget::borrow(&Limits::standard(), allocation, graph),
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).expect("retire dep-info observation fixture");
    }
}

fn artifact(root: &Path, path: &Path, fresh: bool) -> build::Artifact {
    build::Artifact {
        package_id: PACKAGE.into(),
        outputs: Box::default(),
        dep_info: path.into(),
        key: build::Key {
            package_id: PACKAGE.into(),
            target: root.join("a.rs"),
            target_kinds: Box::from([Box::from("lib")]),
            crate_types: Box::from([Box::from("lib")]),
            crate_name: "package".into(),
            features: Box::default(),
        },
        profile: unit::ArtifactProfile::from_build(
            &serde_json::json!({
                "opt_level": "0", "debuginfo": 2, "debug_assertions": true,
                "overflow_checks": true, "test": false,
            }),
            &Limits::standard(),
        )
        .expect("fixture artifact profile"),
        fresh,
    }
}

fn account() -> Allocation {
    Allocation::for_proof(PRIOR, &Limits::standard())
}

fn policy<T>(result: stored::Result<T>, expected: &str) {
    assert!(
        matches!(result, Err(stored::Error::Observation(observe::Error::Policy(actual)))
        if actual == expected),
        "expected policy {expected}"
    );
}

#[test]
fn actual_dep_info_matches_legacy_sources_and_raw_environment_with_shared_artifacts() {
    let fixture = Fixture::open();
    fixture.write("target: b\\ file.rs a\\\n.rs a.rs\n# env-dep:SET=v=more\n# env-dep:EMPTY=\n# env-dep:UNSET\n");
    let current = fixture
        .observe(&mut account(), &node::GraphBudget::new())
        .expect("current data");
    let legacy = Dependency::read(
        toolchain::supported_cargo_commit(),
        &fixture.path,
        &fixture.root,
        &Limits::standard(),
        &mut account(),
    )
    .expect("legacy data");
    assert_eq!(current.sources, legacy.sources);
    assert_eq!(current.environment, legacy.environment);
    assert_eq!(current.sources.len(), 2);
    fs::remove_file(&fixture.path).expect("remove observed source");
    assert_eq!(
        current.environment[0],
        (Box::from("EMPTY"), Some(Box::from("")))
    );
}

#[test]
fn exact_package_root_path_and_complete_output_inventory_precede_io() {
    for mismatch in 0..7 {
        let mut fixture = Fixture::open();
        let Recipe::DepInfo {
            package_id,
            path,
            package_root,
            out_directories,
        } = &mut fixture.recipe
        else {
            panic!("fixture recipe");
        };
        match mismatch {
            0 => *package_id = "other-package".into(),
            1 => *path = fixture.root.join("other.d"),
            2 => *package_root = package_root.join("."),
            3 => *out_directories = Box::default(),
            4 => out_directories.swap(0, 1),
            5 => out_directories[1] = out_directories[0].clone(),
            6 => *path = path.join("."),
            _ => unreachable!(),
        }
        let mut allocation = account();
        let graph = node::GraphBudget::new();
        graph
            .paths(node::MAX_PATH_CHECKS_PER_GRAPH)
            .expect("spent path account");
        graph
            .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
            .expect("spent storage account");
        assert!(matches!(
            fixture.observe(&mut allocation, &graph),
            Err(stored::Error::Observation(observe::Error::Policy(_)))
        ));
        assert_eq!(allocation.usage(), PRIOR);
        allocation
            .items(0)
            .expect("association rejection is not refusal");
    }
    let mut fixture = Fixture::open();
    fixture.recipe = Recipe::SavedOutput {
        package_id: PACKAGE.into(),
        out_dir: fixture.root.clone(),
    };
    policy(
        fixture.observe(&mut account(), &node::GraphBudget::new()),
        "recipe is not a dep-info observation",
    );
}

#[test]
fn opened_growth_and_shortening_use_the_json_limit_not_initial_length() {
    let fixture = Fixture::open();
    for (initial, opened) in [
        ("x", "target: a.rs\n"),
        ("target: b\\ file.rs a.rs\n", "target: a.rs\n"),
    ] {
        fixture.write(initial);
        let current = acquire(
            fixture.recipe.view(),
            &fixture.build,
            &fixture.root,
            &mut Budget::borrow(
                &Limits::standard(),
                &mut account(),
                &node::GraphBudget::new(),
            ),
            |path| {
                fs::write(path, opened).expect("replace after pathname metadata");
                Ok(())
            },
        )
        .expect("opened extent is admitted independently");
        assert_eq!(current.sources.as_ref(), [fixture.root.join("a.rs")]);
    }
    fixture.write("x");
    let mut limits = Limits::standard();
    limits.json_bytes = 2;
    let mut allocation = Allocation::for_proof(PRIOR, &limits);
    // The JSON bound on the opened extent is an accounting target: the
    // grown dep-info is read in full, its opened length is recorded against
    // that bound (not the initial pathname length), and the sources resolve.
    let current = acquire(
        fixture.recipe.view(),
        &fixture.build,
        &fixture.root,
        &mut Budget::borrow(&limits, &mut allocation, &node::GraphBudget::new()),
        |path| {
            fs::write(path, "target: a.rs\n").expect("grow over the JSON bound");
            Ok(())
        },
    )
    .expect("growth over the JSON bound records an exceedance, never a refusal");
    assert_eq!(current.sources.as_ref(), [fixture.root.join("a.rs")]);
    let opened = allocation
        .exceedances()
        .axis("rustc dep-info")
        .copied()
        .expect("the opened extent is recorded against the JSON bound");
    assert_eq!(opened.target, 2);
    assert_eq!(opened.high_water, "target: a.rs\n".len() as u64);
    allocation
        .items(0)
        .expect("an exceedance never latches the account");
}

#[test]
fn dep_info_symlinks_keep_legacy_read_semantics_but_source_symlinks_reject() {
    let fixture = Fixture::open();
    let actual = fixture.root.join("actual.d");
    fs::write(&actual, "target: a.rs\n").expect("actual dep-info");
    std::os::unix::fs::symlink(&actual, &fixture.path).expect("dep-info link");
    fixture
        .observe(&mut account(), &node::GraphBudget::new())
        .expect("legacy follows the associated dep-info path");
    std::os::unix::fs::symlink(fixture.root.join("a.rs"), fixture.root.join("alias.rs"))
        .expect("source link");
    fixture.write("target: alias.rs\n");
    assert!(matches!(
        fixture.observe(&mut account(), &node::GraphBudget::new()),
        Err(stored::Error::Observation(observe::Error::Policy(_)))
    ));
}

#[test]
fn environment_and_source_failures_keep_order_and_distinct_refusal() {
    let fixture = Fixture::open();
    for (text, expected) in [
        ("missing rule\n# env-dep:\n\\\n", "trailing line escape"),
        ("missing rule\n# env-dep:\n", "malformed-env-dep"),
        (
            "missing rule\n# env-dep:K\n# env-dep:K\n# env-dep:\n",
            "duplicate-env-dep",
        ),
        ("target: missing.rs a\\", "missing-dep-info-source"),
        (
            "target: a.rs\n# env-dep:OUT_DIR\n",
            "unproved-out-dir-env-dep",
        ),
        (
            "target: missing.rs\n# env-dep:OUT_DIR\n",
            "missing-dep-info-source",
        ),
    ] {
        fixture.write(text);
        let mut allocation = account();
        policy(
            fixture.observe(&mut allocation, &node::GraphBudget::new()),
            expected,
        );
        allocation
            .items(0)
            .expect("semantic rejection does not refuse");
    }
    // The token byte target records the oversized token before the
    // trailing syntax is judged; the semantic rejection still follows in
    // order and, like every exceedance, leaves the account unlatched.
    fixture.write("target: aa\\");
    let mut allocation = account();
    allocation.string_limit = 1;
    policy(
        fixture.observe(&mut allocation, &node::GraphBudget::new()),
        "trailing escape",
    );
    let token = allocation
        .exceedances()
        .axis("dep-info token bytes")
        .copied()
        .expect("the oversized token is recorded before the trailing escape");
    assert_eq!((token.target, token.high_water), (1, 2));
    allocation
        .bytes(0)
        .expect("semantic rejection after an exceedance does not refuse");
    // Only a genuine failure latches, and that refusal is distinct from every
    // semantic rejection above: the latched account does no further work.
    allocation.refuse();
    assert!(matches!(
        fixture.observe(&mut allocation, &node::GraphBudget::new()),
        Err(stored::Error::Refused)
    ));
    assert!(matches!(
        allocation.bytes(0),
        Err(crate::Error::InputAccountRefused)
    ));
}

#[test]
fn out_dir_is_bound_after_source_parsing_without_ambient_environment() {
    let fixture = Fixture::open();
    for output in [
        fixture.root.join("build/a/out"),
        fixture.root.join("build/b/out"),
    ] {
        fixture.write(&format!(
            "target: a.rs\n# env-dep:OUT_DIR={}\n# env-dep:HOME=from-the-file\n",
            output.display()
        ));
        let dependency = fixture
            .observe(&mut account(), &node::GraphBudget::new())
            .expect("either associated execution");
        assert_eq!(
            dependency.environment[0],
            (Box::from("HOME"), Some(Box::from("from-the-file")))
        );
    }
    fixture.write(&format!(
        "target: a.rs\n# env-dep:OUT_DIR={}\n",
        fixture.root.display()
    ));
    policy(
        fixture.observe(&mut account(), &node::GraphBudget::new()),
        "unproved-out-dir-env-dep",
    );
}

#[test]
fn cumulative_original_floors_include_token_growth_indexing_and_boxing() {
    let fixture = Fixture::open();
    fixture.write("target: b\\ file.rs a.rs a.rs\n# env-dep:Z=v\n# env-dep:A=x\n# env-dep:M\n");
    let mut measured = account();
    fixture
        .observe(&mut measured, &node::GraphBudget::new())
        .expect("measure actual storage");
    let exact = measured.usage();
    assert!(!measured.exceedances().is_exceeded());
    for floor in 0..3 {
        let mut allocation = account();
        allocation.item_limit = exact.items - usize::from(floor == 1);
        allocation.byte_limit = exact.bytes - usize::from(floor == 2);
        // The cumulative floors (token growth, indexing and boxing) are
        // accounting targets: one less on either axis records the exact total
        // and the observation completes on the original account.
        fixture
            .observe(&mut allocation, &node::GraphBudget::new())
            .expect("accounting targets never refuse");
        assert_eq!(allocation.usage(), exact);
        let record = allocation.exceedances();
        match floor {
            0 => assert!(!record.is_exceeded()),
            1 => {
                let axis = record
                    .axis(crate::input::INPUT_ITEMS_RESOURCE)
                    .expect("one less item records the item axis");
                assert_eq!(axis.target, (exact.items - 1) as u64);
                assert_eq!(axis.high_water, exact.items as u64);
                assert_eq!(record.axes().count(), 1);
            }
            _ => {
                let axis = record
                    .axis(crate::input::INPUT_BYTES_RESOURCE)
                    .expect("one less byte records the byte axis");
                assert_eq!(axis.target, (exact.bytes - 1) as u64);
                assert_eq!(axis.high_water, exact.bytes as u64);
                assert_eq!(record.axes().count(), 1);
            }
        }
        allocation
            .items(0)
            .expect("an exceedance never latches the account");
    }
    // Only a genuine failure latches, and the latch is terminal.
    let mut allocation = account();
    allocation.refuse();
    let spent = allocation.usage();
    assert!(matches!(
        fixture.observe(&mut allocation, &node::GraphBudget::new()),
        Err(stored::Error::Refused)
    ));
    assert_eq!(allocation.usage(), spent);
}

#[test]
fn graph_charges_path_attempts_not_storage_copies_or_content_hashing() {
    let fixture = Fixture::open();
    fixture.write("target: a.rs\n");
    let source = fixture.root.join("a.rs");
    assert!(fixture.path.as_os_str().len() < 384 && source.as_os_str().len() < 384);
    // Conservation: the storage the graph's decode axis no longer measures
    // (path copies, the canonical output, the retained index node) is owned by
    // the original input allocation alone.
    let stored = fixture.path.as_os_str().len()
        + 1
        + 7
        + 3 * source.as_os_str().len()
        + crate::store::path::canonical_allocation()
        - std::mem::size_of::<PathBuf>()
        + super::index::node_bytes();
    let mut measured = account();
    fixture
        .observe(&mut measured, &node::GraphBudget::new())
        .expect("measure original storage");
    assert!(measured.usage().bytes - PRIOR.bytes >= stored);
    for kind in 0..2 {
        for floor in 0..=1 {
            if kind == 1 && floor == 1 {
                // No decode charge exists here for a decode floor to refuse.
                continue;
            }
            let graph = node::GraphBudget::new();
            graph
                .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
                .expect("hashing already spent");
            graph.exhaust_comparisons();
            if kind == 0 {
                graph
                    .paths(node::MAX_PATH_CHECKS_PER_GRAPH - 9 + floor)
                    .expect("prior paths");
            } else {
                graph
                    .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
                    .expect("all decoding already spent");
            }
            let mut allocation = account();
            // Graph targets never refuse. The exact remainder records
            // nothing, which conserves the figures: exactly 9 path attempts, no
            // decode of storage copies, no content hashing (hashing and
            // comparisons were already spent to their targets). One less path
            // records the path axis alone, one past its target.
            fixture
                .observe(&mut allocation, &graph)
                .expect("graph targets never refuse");
            let record = graph.exceedances();
            let (decoded, paths, _) = graph.census_usage().expect("graph census");
            if floor == 0 {
                assert!(!record.is_exceeded());
                if kind == 0 {
                    assert_eq!(paths, node::MAX_PATH_CHECKS_PER_GRAPH);
                } else {
                    assert_eq!(
                        decoded,
                        node::MAX_DECODED_BYTES_PER_GRAPH,
                        "storage copies add no decode"
                    );
                }
            } else {
                let axis = record
                    .axis(node::PATHS_RESOURCE)
                    .expect("one less path attempt records the path axis");
                assert_eq!(axis.target, node::MAX_PATH_CHECKS_PER_GRAPH as u64);
                assert_eq!(axis.first_requested, axis.target + 1);
                assert_eq!(axis.high_water, axis.target + 1);
                assert_eq!(paths, node::MAX_PATH_CHECKS_PER_GRAPH + 1);
                assert_eq!(record.axes().count(), 1, "no hashing or decode axis");
            }
            allocation
                .items(0)
                .expect("a graph exceedance never latches the input account");
        }
    }
}
