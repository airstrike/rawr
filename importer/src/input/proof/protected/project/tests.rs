use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::{current, legacy};
use crate::cargo::{unit, universe, Limits};
use crate::input::build::{Artifact, Evidence, Key, Package, Script};
use crate::input::proof::{Entry, Role, TreeDirectory, TreeFile};
use crate::input::stored::{Budget, Error};
use crate::input::{Allocation, Usage};
use crate::node;

const ID: &str = "path+file:///fixture/package#package@1.0.0";
const ROOT: &str = "/fixture/package";
const OUT: &str = "/fixture/target/build/package-run/out";
const TARGET: &str = "aarch64-apple-darwin";
const PRIOR: Usage = Usage {
    items: 7,
    bytes: 31,
};

fn artifact(kind: &str, dep: &str) -> Artifact {
    let custom = kind == "custom-build";
    Artifact {
        package_id: ID.into(),
        outputs: Box::default(),
        dep_info: dep.into(),
        fresh: false,
        key: Key {
            package_id: ID.into(),
            target: Path::new(ROOT).join(if custom { "build.rs" } else { "lib.rs" }),
            target_kinds: Box::from([Box::from(kind)]),
            crate_types: if custom {
                Box::from([Box::from("bin")])
            } else {
                Box::from([Box::from(kind)])
            },
            crate_name: "package".into(),
            features: Box::from([Box::from("first"), Box::from("second")]),
        },
        profile: unit::ArtifactProfile::from_build(
            &serde_json::json!({
                "opt_level":"0", "debuginfo":2, "debug_assertions":true,
                "overflow_checks":true, "test":false,
            }),
            &Limits::standard(),
        )
        .expect("actual artifact profile"),
    }
}

fn script() -> Script {
    Script {
        compiler: Some((1, false)),
        package_id: ID.into(),
        out_dir: OUT.into(),
        cfg: Box::default(),
        environment: Box::default(),
    }
}

fn fixture() -> (Evidence, universe::MetadataProof, Vec<Entry>) {
    let evidence = Evidence {
        usage: Usage {
            items: usize::MAX,
            bytes: usize::MAX,
        },
        packages: BTreeMap::from([(
            ID.into(),
            Package {
                manifest: Path::new(ROOT).join("Cargo.toml"),
                root: ROOT.into(),
                build_script: Some(Path::new(ROOT).join("build.rs")),
            },
        )])
        .into(),
        scripts: Box::from([script()]),
        artifacts: Box::from([
            artifact("lib", "/fixture/target/aarch64-apple-darwin/package.d"),
            artifact(
                "custom-build",
                "/fixture/target/build/package-bin/build_script.d",
            ),
            artifact("proc-macro", "/fixture/target/macro.d"),
        ]),
        environment: Box::default(),
    };
    let protected = universe::MetadataProof::fixture_package(
        ID,
        "package",
        "1.0.0",
        "path",
        PathBuf::from(ROOT),
    );
    let directories = || {
        Box::from([
            TreeDirectory {
                path: "empty".into(),
            },
            TreeDirectory {
                path: "nested".into(),
            },
        ])
    };
    let files = || {
        Box::from([
            TreeFile {
                path: "nested/é.rs".into(),
                bytes: 7,
                digest: [1; 32],
            },
            TreeFile {
                path: "zero".into(),
                bytes: 0,
                digest: [2; 32],
            },
        ])
    };
    let mut entries = Vec::new();
    for (role, path) in [
        (Role::PackageManifest, "Cargo.toml"),
        (Role::DependencySource, "lib.rs"),
        (Role::BuildScript, "build.rs"),
    ] {
        entries.push(Entry::File {
            role,
            package_id: Some(ID.into()),
            path: format!("{ROOT}/{path}").into(),
            bytes: 9,
            digest: [3; 32],
        });
    }
    entries.extend([
        Entry::Directory {
            role: Role::OutDirectory,
            package_id: Some(ID.into()),
            path: OUT.into(),
            directories: directories(),
            files: files(),
        },
        Entry::Directory {
            role: Role::DefaultPackageSource,
            package_id: Some(ID.into()),
            path: ROOT.into(),
            directories: directories(),
            files: files(),
        },
        Entry::Registry {
            role: Role::DefaultPackageSource,
            package_id: ID.into(),
            source: "registry+fixture".into(),
            checksum: [4; 32],
            root: ROOT.into(),
            marker_schema: 1,
            marker_digest: [5; 32],
        },
        Entry::Git {
            role: Role::DefaultPackageSource,
            package_id: ID.into(),
            source: "git+fixture".into(),
            oid: "0123456789012345678901234567890123456789".into(),
            root: ROOT.into(),
            files: files(),
        },
        Entry::Missing {
            role: Role::DeclaredInput,
            package_id: Some(ID.into()),
            path: format!("{ROOT}/missing").into(),
        },
        Entry::Environment {
            package_id: ID.into(),
            execution: evidence.artifacts[0]
                .dep_info
                .to_str()
                .expect("path")
                .into(),
            key: "SET".into(),
            value: Some(format!("{ROOT}/lib.rs {OUT}/generated").into()),
        },
        Entry::Environment {
            package_id: ID.into(),
            execution: OUT.into(),
            key: "EMPTY".into(),
            value: Some("".into()),
        },
        Entry::Environment {
            package_id: ID.into(),
            execution: evidence.artifacts[2]
                .dep_info
                .to_str()
                .expect("path")
                .into(),
            key: "ABSENT".into(),
            value: None,
        },
        Entry::EnvironmentProjection {
            schema: 1,
            key: "ignored-config".into(),
            value: None,
        },
        Entry::Fact {
            role: Role::DependencySource,
            package_id: Some(ID.into()),
            name: format!("package:{ID}").into(),
            value: ROOT.into(),
        },
        Entry::Fact {
            role: Role::DepInfo,
            package_id: Some(ID.into()),
            name: "ignored-structured-fact".into(),
            value: "digest".into(),
        },
        Entry::BuildMessage {
            package_id: ID.into(),
            execution: OUT.into(),
            name: "saved-output".into(),
            source: Some("ignored-transport-spelling".into()),
            value: {
                use sha2::{Digest as _, Sha256};
                #[derive(serde::Serialize)]
                struct Saved {
                    directives: [String; 1],
                    paths: [String; 1],
                    environment: [String; 0],
                }
                let bytes = serde_json::to_vec(&Saved {
                    directives: [format!("rerun-if-changed={ROOT}/lib.rs")],
                    paths: [format!("{ROOT}/lib.rs")],
                    environment: [],
                })
                .expect("fixture JSON");
                format!("{:x}", Sha256::digest(&bytes)).into()
            },
        },
        Entry::Directive {
            package_id: ID.into(),
            execution: OUT.into(),
            index: 0,
            value: format!("rerun-if-changed={ROOT}/lib.rs").into(),
        },
        Entry::File {
            role: Role::Authority,
            package_id: None,
            path: "/ignored/authority".into(),
            bytes: 17,
            digest: [6; 32],
        },
    ]);
    (evidence, protected, entries)
}

fn observed(
    entries: &[Entry],
    evidence: &Evidence,
    protected: &universe::MetadataProof,
    allocation: &mut Allocation,
) -> crate::input::stored::Result<universe::BuildProjection> {
    current(
        entries,
        evidence,
        protected,
        TARGET,
        &Limits::standard(),
        &mut Budget::borrow(&Limits::standard(), allocation, &node::GraphBudget::new()),
    )
}

#[test]
fn generated_sources_bind_execution_and_content_without_physical_output_roots() {
    let (mut evidence, protected, _) = fixture();
    let limits = Limits::standard();
    let mut reference = None;
    for root in [OUT, "/relocated/target/build/package-run/out"] {
        evidence.scripts[0].out_dir = root.into();
        let entries = [Entry::File {
            role: Role::DependencySource,
            package_id: Some(ID.into()),
            path: format!("{root}/nested/generated.rs").into(),
            bytes: 17,
            digest: [42; 32],
        }];
        let cold = legacy(
            &entries,
            &evidence,
            &protected,
            TARGET,
            &limits,
            &mut Allocation::for_proof(PRIOR, &limits),
        )
        .expect("generated cold input");
        let current = observed(
            &entries,
            &evidence,
            &protected,
            &mut Allocation::for_proof(PRIOR, &limits),
        )
        .expect("generated current input");
        assert_eq!(cold.bytes(), current.bytes());
        let value: serde_json::Value = serde_json::from_slice(current.bytes()).expect("projection");
        assert_eq!(value[1][0]["kind"], "generated_file");
        assert_eq!(value[1][0]["path"], "nested/generated.rs");
        assert_eq!(value[1][0]["bytes"], 17);
        assert_eq!(value[1][0]["digest"], serde_json::json!(&[42; 32][..]));
        assert_eq!(value[1][0]["execution"]["kind"], "build_script");
        if let Some(reference) = &reference {
            assert_eq!(current.bytes(), reference);
        } else {
            reference = Some(current.bytes().to_vec());
        }
    }
}

#[test]
fn generated_sources_reject_ambiguous_or_unassociated_output_owners() {
    for ambiguous in [false, true] {
        let (mut evidence, protected, _) = fixture();
        let path = if ambiguous {
            evidence.scripts = Box::from([script(), script()]);
            format!("{OUT}/generated.rs")
        } else {
            format!("{OUT}-unowned/generated.rs")
        };
        let entries = [Entry::File {
            role: Role::DependencySource,
            package_id: Some(ID.into()),
            path: path.into(),
            bytes: 1,
            digest: [1; 32],
        }];
        let result = observed(
            &entries,
            &evidence,
            &protected,
            &mut Allocation::for_proof(PRIOR, &Limits::standard()),
        );
        assert!(
            matches!(result, Err(Error::Encoding(code)) if code == if ambiguous {
            "protected-build-generated-owner-ambiguous"
        } else { "protected-build-package-path-unmapped" })
        );
    }
}

#[test]
fn target_generated_output_uses_host_compiled_script_without_erasing_execution_platform() {
    let (mut evidence, protected, _) = fixture();
    let output = format!("/fixture/target/{TARGET}/debug/build/package-run/out");
    evidence.scripts[0].compiler = Some((1, true));
    evidence.scripts[0].out_dir = output.clone().into();
    let entries = [Entry::File {
        role: Role::DependencySource,
        package_id: Some(ID.into()),
        path: format!("{output}/generated.rs").into(),
        bytes: 1,
        digest: [1; 32],
    }];
    let limits = Limits::standard();
    let projection = observed(
        &entries,
        &evidence,
        &protected,
        &mut Allocation::for_proof(PRIOR, &limits),
    )
    .expect("target output joins host binary");
    let cold = legacy(
        &entries,
        &evidence,
        &protected,
        TARGET,
        &limits,
        &mut Allocation::for_proof(PRIOR, &limits),
    )
    .expect("same cold execution join");
    assert_eq!(projection.bytes(), cold.bytes());
    let value: serde_json::Value = serde_json::from_slice(projection.bytes()).expect("projection");
    assert_eq!(value[1][0]["execution"]["platform"], "target");

    let mut artifacts = std::mem::take(&mut evidence.artifacts).into_vec();
    artifacts.push(artifact(
        "custom-build",
        "/fixture/target/build/other/build_script.d",
    ));
    evidence.artifacts = artifacts.into_boxed_slice();
    let unchanged = observed(
        &entries,
        &evidence,
        &protected,
        &mut Allocation::for_proof(PRIOR, &limits),
    )
    .expect("additional compiler does not change the exact bound artifact");
    assert_eq!(unchanged.bytes(), projection.bytes());
}

#[test]
fn generated_source_comparison_overflow_latches_the_original_account() {
    use super::super::account::{Account as _, Current};
    let limits = Limits::standard();
    let graph = node::GraphBudget::new();
    graph
        .decode(usize::try_from(node::MAX_DECODED_BYTES_PER_GRAPH).expect("test range"))
        .expect("consume comparison work allowance");
    let mut allocation = Allocation::for_proof(PRIOR, &limits);
    let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
    let mut current = Current(&mut budget);
    // Comparison work past the decode target is recorded on the graph
    // with the exact requested amount; it is not refused and does not latch.
    current
        .comparison(1)
        .expect("the comparison target is recorded, not refused");
    let axis = *graph
        .exceedances()
        .axis(node::DECODED_RESOURCE)
        .expect("comparison work records the decode axis");
    assert_eq!(
        (axis.target, axis.first_requested, axis.high_water),
        (
            node::MAX_DECODED_BYTES_PER_GRAPH,
            node::MAX_DECODED_BYTES_PER_GRAPH + 1,
            node::MAX_DECODED_BYTES_PER_GRAPH + 1
        )
    );
    current
        .check()
        .expect("an accounting exceedance never latches the account");
    // Checked-arithmetic overflow of the decode account is a genuine failure
    // that stays terminal and latches the original input account.
    let decoded = graph.census_usage().expect("graph census").0;
    graph
        .decode(usize::try_from(u64::MAX - decoded).expect("addressable remainder"))
        .expect("saturate the decode account");
    assert!(matches!(
        current.comparison(1),
        Err(Error::Graph(node::Error::Allocation))
    ));
    assert!(matches!(current.check(), Err(Error::Refused)));
}

#[test]
fn unprotected_generated_source_is_not_promoted_into_protected_evidence() {
    let (evidence, _, _) = fixture();
    let protected = universe::MetadataProof::fixture_package(
        "other",
        "other",
        "1.0.0",
        "path",
        PathBuf::from("/other"),
    );
    let entries = [Entry::File {
        role: Role::DependencySource,
        package_id: Some(ID.into()),
        path: format!("{OUT}/generated.rs").into(),
        bytes: 1,
        digest: [1; 32],
    }];
    let projection = observed(
        &entries,
        &evidence,
        &protected,
        &mut Allocation::for_proof(PRIOR, &Limits::standard()),
    )
    .expect("unprotected output");
    let value: serde_json::Value = serde_json::from_slice(projection.bytes()).expect("projection");
    assert_eq!(value[1], serde_json::json!([]));
}

/// An unprotected package may read a `#[path]` source outside every
/// package root (the generated polyglot app crate reads the project's `src/`).
#[test]
fn unprotected_source_outside_every_package_root_is_not_refused() {
    let (evidence, _, _) = fixture();
    let protected = universe::MetadataProof::fixture_package(
        "other",
        "other",
        "1.0.0",
        "path",
        PathBuf::from("/other"),
    );
    let entries = [Entry::File {
        role: Role::DependencySource,
        package_id: Some(ID.into()),
        path: "/fixture/project/src/model.rs".into(),
        bytes: 1,
        digest: [1; 32],
    }];
    let projection = observed(
        &entries,
        &evidence,
        &protected,
        &mut Allocation::for_proof(PRIOR, &Limits::standard()),
    )
    .expect("unprotected source outside its package root");
    let value: serde_json::Value = serde_json::from_slice(projection.bytes()).expect("projection");
    assert_eq!(value[1], serde_json::json!([]));
}

#[test]
fn cold_comparisons_use_original_work_budget_without_claiming_owned_bytes() {
    use super::super::account::{Account as _, Legacy};
    let limits = Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = Allocation::for_proof(PRIOR, &limits);
    Legacy(&mut allocation, &graph)
        .comparison(1024)
        .expect("borrowed comparison work");
    assert_eq!(
        allocation.usage(),
        PRIOR,
        "borrowed comparisons retain no memory"
    );
    graph
        .decode(usize::try_from(node::MAX_DECODED_BYTES_PER_GRAPH).expect("test range") - 1024)
        .expect("same request work account is now exhausted");
    let (evidence, protected, _) = fixture();
    let entries = [Entry::File {
        role: Role::DependencySource,
        package_id: Some(ID.into()),
        path: format!("{OUT}/generated.rs").into(),
        bytes: 1,
        digest: [1; 32],
    }];
    // The full projector spends the same supplied work account: its
    // comparisons cross the exhausted decode target and are recorded on that
    // graph, while the projection still completes and nothing latches.
    super::legacy_accounted(
        &entries,
        &evidence,
        &protected,
        TARGET,
        &limits,
        &mut allocation,
        &graph,
    )
    .expect("the comparison target is recorded, not refused");
    let axis = *graph
        .exceedances()
        .axis(node::DECODED_RESOURCE)
        .expect("full projector must spend the supplied account");
    assert_eq!(axis.target, node::MAX_DECODED_BYTES_PER_GRAPH);
    assert!(axis.first_requested > node::MAX_DECODED_BYTES_PER_GRAPH);
    allocation
        .check()
        .expect("an accounting exceedance never latches the account");
    // Checked-arithmetic overflow of the work account is a genuine failure:
    // the projector refuses before claiming any owned bytes and latches the
    // supplied input account.
    let decoded = graph.census_usage().expect("graph census").0;
    graph
        .decode(usize::try_from(u64::MAX - decoded).expect("addressable remainder"))
        .expect("saturate the work account");
    let mut fresh = Allocation::for_proof(PRIOR, &limits);
    let error = super::legacy_accounted(
        &entries, &evidence, &protected, TARGET, &limits, &mut fresh, &graph,
    )
    .expect_err("an overflowed work account refuses the full projector");
    assert!(matches!(
        error,
        crate::Error::Input {
            code: "protected-build-comparison-work",
            ..
        }
    ));
    assert_eq!(fresh.usage().bytes, PRIOR.bytes);
    assert!(matches!(
        fresh.check(),
        Err(crate::Error::InputAccountRefused)
    ));
}

#[test]
fn all_entry_variants_and_deep_execution_payloads_match_the_cold_projector() {
    let (evidence, protected, entries) = fixture();
    assert_eq!(entries.len(), 17);
    let limits = Limits::standard();
    let mut cold_account = Allocation::for_proof(PRIOR, &limits);
    let cold = legacy(
        &entries,
        &evidence,
        &protected,
        TARGET,
        &limits,
        &mut cold_account,
    )
    .expect("complete legacy projection");
    let mut account = Allocation::for_proof(PRIOR, &limits);
    let actual = observed(&entries, &evidence, &protected, &mut account)
        .expect("complete current projection");
    assert_eq!(actual.bytes(), cold.bytes());
    assert_eq!(actual.digest(), cold.digest());
    let value: serde_json::Value =
        serde_json::from_slice(actual.bytes()).expect("canonical projection");
    let projected = value[1].as_array().expect("protected entries");
    assert_eq!(projected.len(), 14);
    for kind in [
        "file",
        "output_directory",
        "source_directory",
        "registry",
        "git",
        "missing",
        "environment",
        "fact",
        "build_message",
        "directive",
    ] {
        assert!(
            projected.iter().any(|value| value["kind"] == kind),
            "retained variant {kind}"
        );
    }
    for role in ["normal", "proc_macro"] {
        assert!(
            projected
                .iter()
                .any(|value| value["execution"]["role"] == role),
            "retained artifact role {role}"
        );
    }
    assert!(projected
        .iter()
        .any(|value| value["execution"]["kind"] == "build_script"));
    let tree = projected
        .iter()
        .find(|value| value["kind"] == "output_directory")
        .expect("output tree");
    assert_eq!(
        tree["directories"].as_array().expect("directories").len(),
        2
    );
    assert_eq!(tree["files"].as_array().expect("files").len(), 2);
    assert_eq!(
        tree["execution"]["features"],
        serde_json::json!(["first", "second"])
    );
    // The original cold schedule charges its roots, every index insertion,
    // each projected entry, four saved-output working lists, and final canonical bytes.
    assert_eq!(
        cold_account.usage(),
        Usage {
            items: PRIOR.items + 1 + 1 + 3 + 14 + 4,
            bytes: PRIOR.bytes + cold.bytes().len()
        }
    );
    assert!(
        account.usage().bytes > cold_account.usage().bytes,
        "current deep copies are prospectively charged"
    );
}

#[test]
fn ambiguous_or_missing_execution_joins_match_cold_rejections() {
    for (mode, expected) in [
        (0, "protected-build-script-execution-ambiguous"),
        (1, "protected-build-script-artifact-unmapped"),
        (2, "protected-environment-artifact-ambiguous"),
        (3, "protected-build-script-artifact-unmapped"),
    ] {
        let (mut evidence, protected, _) = fixture();
        let execution = if mode == 2 {
            "/fixture/target/aarch64-apple-darwin/package.d"
        } else {
            OUT
        };
        match mode {
            0 => evidence.scripts = Box::from([script(), script()]),
            1 => evidence.scripts[0].compiler = None,
            2 => {
                let mut artifacts = evidence.artifacts.into_vec();
                artifacts.push(artifact("lib", execution));
                evidence.artifacts = artifacts.into_boxed_slice();
            }
            3 => {
                evidence.artifacts = Box::from([artifact(
                    "lib",
                    "/fixture/target/aarch64-apple-darwin/package.d",
                )])
            }
            _ => unreachable!(),
        }
        let entries = [Entry::Environment {
            package_id: ID.into(),
            execution: execution.into(),
            key: "KEY".into(),
            value: None,
        }];
        let limits = Limits::standard();
        let cold = legacy(
            &entries,
            &evidence,
            &protected,
            TARGET,
            &limits,
            &mut Allocation::for_proof(PRIOR, &limits),
        );
        assert!(
            matches!(cold, Err(crate::Error::Input { code, .. }) if code == expected),
            "cold ambiguity {mode}"
        );
        let actual = observed(
            &entries,
            &evidence,
            &protected,
            &mut Allocation::for_proof(PRIOR, &limits),
        );
        assert!(
            matches!(actual, Err(Error::Encoding(code)) if code == expected),
            "current ambiguity {mode}"
        );
    }
}

#[test]
fn deep_projection_uses_original_exact_floors_and_terminal_refusal() {
    let (evidence, protected, entries) = fixture();
    let limits = Limits::standard();
    let mut account = Allocation::for_proof(PRIOR, &limits);
    observed(&entries, &evidence, &protected, &mut account).expect("actual complete ownership");
    let spent = account.usage();
    for mode in 0..3 {
        let mut bounded = limits;
        bounded.items = spent.items - usize::from(mode == 1);
        bounded.input_proof_bytes = spent.bytes - usize::from(mode == 2);
        let mut account = Allocation::for_proof(PRIOR, &bounded);
        let result = observed(&entries, &evidence, &protected, &mut account);
        // Every floor still projects the exact measured ownership.
        result.expect("accounting targets are recorded, never refused");
        assert_eq!(account.usage(), spent);
        if mode == 0 {
            assert!(!account.exceedances().is_exceeded());
        } else {
            // One under records that axis alone with a high water equal to
            // the exact deep total, and nothing latches.
            let (exceeded, untouched, total) = if mode == 1 {
                (
                    crate::input::INPUT_ITEMS_RESOURCE,
                    crate::input::INPUT_BYTES_RESOURCE,
                    spent.items,
                )
            } else {
                (
                    crate::input::INPUT_BYTES_RESOURCE,
                    crate::input::INPUT_ITEMS_RESOURCE,
                    spent.bytes,
                )
            };
            let axis = *account
                .exceedances()
                .axis(exceeded)
                .expect("one-under deep account records the exact total");
            assert_eq!(
                (axis.target, axis.high_water),
                ((total - 1) as u64, total as u64),
                "one-under deep account {mode}"
            );
            assert!(account.exceedances().axis(untouched).is_none());
            account
                .check()
                .expect("an accounting exceedance never latches the account");
            // A genuine failure latches the account; a latched account does
            // no further projection work and keeps its spent usage.
            account.refuse();
            let stopped = account.usage();
            assert!(matches!(
                observed(&entries, &evidence, &protected, &mut account),
                Err(Error::Refused)
            ));
            assert_eq!(account.usage(), stopped);
        }
    }
}

#[test]
fn current_limits_do_not_widen_borrowed_units_and_hash_overflow_latches() {
    let (evidence, protected, entries) = fixture();
    let limits = Limits::standard();
    let mut borrowed = limits;
    borrowed.units = 0;
    let mut allocation = Allocation::for_proof(PRIOR, &limits);
    let graph = node::GraphBudget::new();
    // The wider call limits do not widen the borrowed unit target: the
    // first execution join is recorded against the borrowed 0 on the original
    // account, the projection still completes and nothing latches.
    current(
        &entries,
        &evidence,
        &protected,
        TARGET,
        &limits,
        &mut Budget::borrow(&borrowed, &mut allocation, &graph),
    )
    .expect("the borrowed unit target is recorded, not refused");
    let axis = *allocation
        .exceedances()
        .axis("protected build-script execution joins")
        .expect("the borrowed unit target records its axis");
    assert_eq!((axis.target, axis.first_requested), (0, 1));
    observed(&entries, &evidence, &protected, &mut allocation)
        .expect("an accounting exceedance never latches the account");

    let mut allocation = Allocation::for_proof(PRIOR, &limits);
    let graph = node::GraphBudget::new();
    graph
        .hash(usize::try_from(node::MAX_HASHED_BYTES_PER_GRAPH).expect("test target hash range"))
        .expect("consume original hash allowance");
    // Hash work past the target is recorded on the graph and never refused.
    current(
        &entries,
        &evidence,
        &protected,
        TARGET,
        &limits,
        &mut Budget::borrow(&limits, &mut allocation, &graph),
    )
    .expect("the hash target is recorded, not refused");
    let axis = *graph
        .exceedances()
        .axis(node::HASHED_RESOURCE)
        .expect("projection hash work records the hash axis");
    assert_eq!(axis.target, node::MAX_HASHED_BYTES_PER_GRAPH);
    assert!(axis.first_requested > node::MAX_HASHED_BYTES_PER_GRAPH);
    allocation
        .check()
        .expect("a graph exceedance never latches the input account");
    // Checked-arithmetic overflow of the hash account is a genuine failure
    // that stays terminal and latches the original input account.
    let hashed = graph.census_usage().expect("graph census").2;
    graph
        .hash(usize::try_from(u64::MAX - hashed).expect("addressable remainder"))
        .expect("saturate the hash account");
    assert!(matches!(
        current(
            &entries,
            &evidence,
            &protected,
            TARGET,
            &limits,
            &mut Budget::borrow(&limits, &mut allocation, &graph)
        ),
        Err(Error::Graph(node::Error::Allocation))
    ));
    assert!(matches!(
        observed(&entries, &evidence, &protected, &mut allocation),
        Err(Error::Refused)
    ));
}
