//! The cold Probe boundary and the same output validation without a launcher.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::{json, Value};

use super::preparation::Fixture;
use super::{Plan, Probe};
use crate::cargo::{self, resolution::layout::Layout, toolchain, unit};
use crate::extract::protocol;
use crate::input::build::Build;
use crate::process::{self, test_launch, ToolPaths as _};
use crate::{Error, Result};

const PACKAGE: &str = "path+file:///fixture#dependency@0.1.0";
const KEY: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const ARTIFACT: &[u8] = b"selected rmeta fixture";

struct Inputs {
    graph: unit::BuildGraph,
    build: Build,
    metadata: unit::Metadata,
}

impl Inputs {
    fn materialize(layout: &Layout) -> Self {
        Self::materialize_at(layout, layout.final_form().build(), None)
    }

    fn materialize_at(layout: &Layout, artifact_root: &Path, uplift: Option<&Path>) -> Self {
        let manifest = layout.final_form().manifest();
        let source = layout.final_form().source();
        let deps = artifact_root.join(toolchain::HOST).join("debug/deps");
        fs::create_dir_all(&deps).expect("pinned Cargo dependency directory creates");
        let artifact = deps.join("libdependency.rmeta");
        fs::write(manifest, b"[package]\nname='dependency'\nversion='0.1.0'\n")
            .expect("fixture manifest publishes");
        fs::write(source, b"pub struct Dependency;\n").expect("fixture source publishes");
        fs::write(&artifact, ARTIFACT).expect("selected artifact publishes");
        fs::write(deps.join("dependency.d"), b"fixture dep-info\n")
            .expect("Build's required dep-info path exists");
        let mut filenames = vec![artifact];
        if let Some(uplift) = uplift {
            fs::create_dir_all(uplift.parent().expect("uplift has a parent"))
                .expect("pinned Cargo final-output directory creates");
            fs::write(uplift, b"uplifted library fixture").expect("target library publishes");
            filenames.push(uplift.to_path_buf());
        }
        let output = unit::Output::fixture(
            PACKAGE,
            source.to_str().expect("fixture source is Unicode"),
            "dependency",
            "lib",
            "rlib",
        )
        .with_fixture_semantic_key(KEY);
        let graph = unit::BuildGraph::fixture_selected_artifact(output, "dependency");
        let message = json!({
            "reason": "compiler-artifact",
            "package_id": PACKAGE,
            "manifest_path": manifest,
            "target": {
                "kind": ["lib"], "crate_types": ["rlib"], "name": "dependency",
                "src_path": source,
            },
            "features": [],
            "profile": {
                "debug_assertions": true, "debuginfo": 2, "opt_level": "0",
                "overflow_checks": true, "test": false,
            },
            "filenames": filenames, "fresh": false,
        });
        let bytes = format!("{message}\n{{\"reason\":\"build-finished\",\"success\":true}}\n");
        let build = Build::parse(
            bytes.as_bytes(),
            Box::default(),
            toolchain::supported_cargo_commit(),
            &cargo::Limits::standard(),
        )
        .expect("the actual bounded Build parser accepts the fixture");
        let metadata = unit::Metadata::fixture_packages(std::iter::empty());
        Self {
            graph,
            build,
            metadata,
        }
    }

    fn plan(&self, layout: &Layout, proof: &toolchain::Proof) -> Result<Plan> {
        Plan::prepare_in(
            layout,
            &self.graph,
            proof,
            toolchain::HOST,
            &cargo::Limits::standard(),
        )
    }

    fn probe(&self, layout: &Layout, proof: &toolchain::Proof) -> Result<Probe> {
        self.plan(layout, proof)?
            .bind(&self.build, &self.graph, &cargo::Limits::standard())
    }
}

fn fragment(probe: &Probe) -> Value {
    let entry = &probe.entries[0];
    let artifact = &entry.state.artifact;
    json!({
        "handshake": {
            "schema": protocol::SCHEMA,
            "package": entry.package,
            "root": entry.root,
            "target": entry.target,
            "target_kind": entry.target_kind,
            "target_kinds": entry.target_kinds,
            "crate_types": entry.crate_types,
            "crate_name": entry.crate_name,
            "edition": entry.edition,
            "profile": entry.profile,
            "compilation": "target",
            "host": probe.host,
            "selected_target": probe.selected_target,
            "features": entry.features,
            "probe_arguments_digest": "2".repeat(64),
            "compiler_commit": probe.compiler_commit,
            "invocation_nonce": entry.invocation_nonce,
            "unit_correlation": entry.unit_correlation,
            "unit_key": entry.unit_key,
            "dependencies": entry.dependencies,
            "artifact": {
                "path": artifact.path, "length": artifact.length,
                "digest": artifact.digest, "profile": artifact.profile, "fresh": artifact.fresh,
                "crate_hash": "fixture-crate-hash", "stable_crate_id": "3333333333333333",
                "probe_alias": "dependency",
            },
            "sysroot": [],
            "loaded_crates": [{
                "crate_name": entry.crate_name, "stable_crate_id": "3333333333333333",
                "artifacts": [artifact.path],
            }],
            "external_crates": [],
        },
        "reachable": [], "definitions": [], "implementations": [],
        "scalar_predicate_proofs": [],
    })
}

fn sidecar(probe: &Probe) -> PathBuf {
    probe
        .protocol_directory
        .join(format!("{}.json", probe.entries[0].unit_correlation))
}

fn file(layout: &Layout, path: &Path, bytes: Vec<u8>) -> test_launch::File {
    test_launch::File::new(
        path.strip_prefix(layout.root())
            .expect("script output belongs to its Attempt")
            .to_path_buf(),
        bytes,
    )
}

fn script(layout: &Layout, steps: Vec<test_launch::Step>) -> test_launch::Script {
    test_launch::Script::new(
        layout.root().to_path_buf(),
        test_launch::FileLimits {
            count: 4,
            bytes: 64 * 1024,
        },
        steps,
    )
    .expect("the launch script owns only this fixture Attempt")
}

fn launch(
    layout: &Layout,
    native: &process::Native,
    files: Vec<test_launch::File>,
    exit: u8,
) -> test_launch::Step {
    let paths = native.production();
    let executable = paths.driver().to_path_buf();
    let rustc = paths.rustc().as_os_str().to_owned();
    let plan = layout.extraction().plan().as_os_str().to_owned();
    let protocol = layout.extraction().protocol().as_os_str().to_owned();
    let source = layout.extraction().source().as_os_str().to_owned();
    let directory = layout.extraction().root().to_path_buf();
    let completion_plan = layout.extraction().plan().to_path_buf();
    let completion_path = layout.extraction().protocol().join("attempts");
    let completion_root = layout.root().to_path_buf();
    test_launch::Step::new(
        move |_| {
            test_launch::Expected::new(
                executable,
                process::Command::probe(
                    vec!["--probe".into(), rustc, plan, protocol, source],
                    directory,
                    Box::default(),
                )
                .expect("the independent expected Probe command seals"),
                cargo::Limits::standard().process_limits(),
            )
        },
        move |_| {
            let mut files = files;
            if exit == 0 {
                let plan: Value =
                    serde_json::from_slice(&fs::read(&completion_plan).unwrap()).unwrap();
                let rows = plan["entries"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|entry| protocol::attempt::Invocation {
                        nonce: entry["invocation_nonce"].as_str().unwrap(),
                        correlation: entry["unit_correlation"].as_str().unwrap(),
                    })
                    .collect::<Vec<_>>();
                let mut bytes = Vec::new();
                protocol::attempt::encode(
                    &mut bytes,
                    &"2".repeat(64),
                    true,
                    &rows,
                    std::iter::empty(),
                    std::iter::empty(),
                )
                .unwrap();
                files.push(test_launch::File::new(
                    completion_path
                        .strip_prefix(&completion_root)
                        .unwrap()
                        .to_path_buf(),
                    bytes,
                ));
            }
            test_launch::Reply::exit(exit, Vec::new(), Vec::new(), files)
        },
    )
}

fn snapshot(probe: &Probe) -> Vec<(PathBuf, SystemTime, Vec<u8>)> {
    let sidecar = sidecar(probe);
    let attempts = probe.protocol_directory.join("attempts");
    [
        probe.plan_file.as_path(),
        probe.probe_source.as_path(),
        probe.artifacts[0].0.as_path(),
        sidecar.as_path(),
        attempts.as_path(),
    ]
    .into_iter()
    .map(|path| {
        (
            path.to_path_buf(),
            fs::metadata(path)
                .expect("fixture metadata reads")
                .modified()
                .expect("mtime reads"),
            fs::read(path).expect("fixture bytes read"),
        )
    })
    .collect()
}

fn rejection(error: Error) -> &'static str {
    match error {
        Error::Extraction { code, .. } => code,
        Error::Limit { resource, .. } => resource,
        Error::Node { operation, .. } => operation,
        Error::Path { field, .. } => field,
        error => panic!("unexpected rejection: {error:?}"),
    }
}

#[test]
fn fragments_from_different_compiler_invocations_reject() {
    let fixture = Fixture::open("probe-invocation-binding");
    fixture
        .reserve(|layout, proof| {
            let inputs = Inputs::materialize(layout);
            let probe = inputs.probe(layout, proof)?;
            let mut entries = probe.entries.clone();
            let mut second = entries[0].clone();
            second.unit_correlation = "a".repeat(64).into_boxed_str();
            entries.push(second);
            let first: protocol::Fragment = serde_json::from_value(fragment(&probe)).unwrap();
            let mut second = first.clone();
            second.handshake.unit_correlation = "a".repeat(64);
            let mut fragments = vec![first, second];
            let limits = cargo::Limits::standard();
            let validate = |fragments: &[protocol::Fragment]| {
                super::validate_fragments(
                    fragments,
                    &entries,
                    &probe.compiler_commit,
                    &probe.host,
                    &probe.selected_target,
                    &limits,
                    &mut super::Allocation::new(&limits),
                )
            };
            validate(&fragments)?;
            fragments[1].handshake.probe_arguments_digest = "b".repeat(64);
            assert_eq!(
                rejection(validate(&fragments).unwrap_err()),
                "split-probe-invocation"
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn selected_artifacts_from_other_attempt_roots_reject_before_launch() {
    let fixture = Fixture::open("probe-artifact-root");
    for provisional in [false, true] {
        fixture
            .reserve(|layout, proof| {
                let other_root = if provisional {
                    layout.provisional().build()
                } else {
                    layout.final_form().target()
                };
                let inputs = Inputs::materialize_at(layout, other_root, None);
                let mut plan = inputs.plan(layout, proof)?;
                assert_eq!(plan.command_target()?, layout.final_form().target());
                let native = process::Native::from_toolchain(proof);
                let error = script(layout, Vec::new())
                    .run(|| {
                        plan.seal(
                            &native.production(),
                            &inputs.build,
                            &inputs.graph,
                            &inputs.metadata,
                            &cargo::Limits::standard(),
                        )
                    })
                    .expect("artifact containment rejects before any launcher callback")
                    .err()
                    .expect("another reserved root cannot supply the final Build artifact");
                assert!(matches!(error, Error::Path {
                    field: "selected rmeta artifact",
                    reason,
                    ..
                } if reason.as_ref() == "escaped its sealed parent"));
                assert!(!layout.extraction().plan().exists());
                Ok(())
            })
            .expect("other-root artifact rejection cleans the reservation");
    }
    fixture.retire();
}

#[test]
fn cold_seal_and_read_only_validation_produce_the_same_capture() {
    let fixture = Fixture::open("probe-parity");
    fixture
        .reserve(|layout, proof| {
            // Cargo reports root-library uplift paths in target while retaining
            // the same unit's rmeta and dep-info below build/<target>/debug/deps.
            let uplift = layout
                .final_form()
                .target()
                .join(toolchain::HOST)
                .join("debug/libdependency.rlib");
            let inputs = Inputs::materialize_at(layout, layout.final_form().build(), Some(&uplift));
            let expected = inputs.probe(layout, proof)?;
            let mut value = fragment(&expected);
            value["handshake"]["loaded_crates"][0]["artifacts"] =
                json!([expected.entries[0].state.artifact.path, uplift,]);
            let bytes = serde_json::to_vec(&value).expect("fixture fragment encodes");
            let native = process::Native::from_toolchain(proof);
            let step = launch(
                layout,
                &native,
                vec![file(layout, &sidecar(&expected), bytes)],
                0,
            );
            let plan = inputs.plan(layout, proof)?;
            assert_eq!(plan.target_directory(), layout.final_form().target());
            assert_eq!(expected.artifact_directory, layout.final_form().build());
            assert!(expected.artifacts.iter().all(|(path, _, _)| {
                path.starts_with(layout.final_form().build())
                    && !path.starts_with(layout.final_form().target())
            }));
            let limits = cargo::Limits::standard();
            protocol::reset_accounted_decodes();
            let cold = script(layout, vec![step])
                .run(|| {
                    plan.seal(
                        &native.production(),
                        &inputs.build,
                        &inputs.graph,
                        &inputs.metadata,
                        &limits,
                    )
                })
                .expect("exactly one independently matched cold Probe runs")?;
            assert_eq!(protocol::accounted_decodes(), 1);
            let before = snapshot(&expected);
            protocol::reset_accounted_decodes();
            let retained = script(layout, Vec::new())
                .run(|| {
                    Plan::replay_in(
                        layout.paths(),
                        &inputs.graph,
                        proof,
                        toolchain::HOST,
                        &inputs.build,
                        &inputs.metadata,
                        &limits,
                        layout.budget(),
                    )
                })
                .expect("read-only validation runs no child")?;
            assert_eq!(protocol::accounted_decodes(), 1);
            assert_eq!(cold.document, retained.document);
            assert_eq!(cold.document_bytes, retained.document_bytes);
            assert_eq!(cold.controls, retained.controls);
            assert_eq!(cold.sidecars, retained.sidecars);
            assert_eq!(cold.artifacts, retained.artifacts);
            assert_eq!(cold.artifacts.len(), 2);
            assert!(cold.artifacts.iter().any(|(path, _, _)| path == &uplift));
            for (path, modified, bytes) in before {
                assert_eq!(fs::read(&path).expect("retained bytes read"), bytes);
                assert_eq!(
                    fs::metadata(path)
                        .expect("retained metadata reads")
                        .modified()
                        .unwrap(),
                    modified
                );
            }
            Ok(())
        })
        .expect("the paired validation fixture cleans up");
    fixture.retire();
}

#[test]
fn retained_controls_are_checked_before_sidecars_without_republication() {
    let fixture = Fixture::open("probe-controls");
    for case in ["plan", "source", "source-alias"] {
        fixture
            .reserve(|layout, proof| {
                let inputs = Inputs::materialize(layout);
                let probe = inputs.probe(layout, proof)?;
                let bytes =
                    serde_json::to_vec(&fragment(&probe)).expect("fixture fragment encodes");
                let sidecar = sidecar(&probe);
                let native = process::Native::from_toolchain(proof);
                let step = launch(layout, &native, vec![file(layout, &sidecar, bytes)], 0);
                let limits = cargo::Limits::standard();
                let probe = script(layout, vec![step])
                    .run(|| probe.run(&native.production(), &limits))
                    .expect("cold publication executes exactly one Probe")?;
                let control = if case == "plan" {
                    &probe.plan_file
                } else {
                    &probe.probe_source
                };
                if case == "source-alias" {
                    let alias = layout.extraction().root().join("source-alias.rs");
                    fs::write(&alias, super::PROBE_SOURCE).expect("valid aliased source");
                    fs::remove_file(control).expect("replace fixed source");
                    std::os::unix::fs::symlink(&alias, control).expect("internal source alias");
                } else {
                    fs::write(control, b"corrupt control").expect("control corrupts");
                }
                fs::write(&sidecar, b"corrupt sidecar").expect("later input corrupts too");
                let before = snapshot(&probe);
                protocol::reset_accounted_decodes();
                let error = script(layout, Vec::new())
                    .run(|| {
                        Plan::replay_in(
                            layout.paths(),
                            &inputs.graph,
                            proof,
                            toolchain::HOST,
                            &inputs.build,
                            &inputs.metadata,
                            &limits,
                            layout.budget(),
                        )
                    })
                    .expect("retained rejection has no launch capability")
                    .err()
                    .expect("exact control bytes reject drift");
                if case == "source-alias" {
                    assert!(matches!(
                        error,
                        Error::Path {
                            field: "metadata probe source",
                            ..
                        }
                    ));
                } else {
                    assert_eq!(rejection(error), "metadata-probe-control-drift");
                }
                assert_eq!(protocol::accounted_decodes(), 0);
                for (path, modified, bytes) in before {
                    assert_eq!(fs::read(&path).expect("corrupt bytes remain"), bytes);
                    assert_eq!(fs::metadata(path).unwrap().modified().unwrap(), modified);
                }
                Ok(())
            })
            .expect("control rejection cleans the real reservation");
    }
    fixture.retire();
}

#[test]
fn cold_and_retained_reject_the_same_sidecar_and_artifact_corruption() {
    let fixture = Fixture::open("probe-corruption");
    for (case, expected) in [
        ("missing", "missing-or-extra-sidecar"),
        ("extra", "extraction protocol files"),
        ("schema", "sidecar-schema"),
        ("correlation", "probe-plan-mismatch"),
        ("artifact", "conflicting-loaded-artifact"),
        ("target-root", "missing-loaded-artifact-join"),
        ("provisional-build", "loaded compiler artifact"),
        ("sibling-build", "loaded compiler artifact"),
    ] {
        fixture
            .reserve(|layout, proof| {
                let inputs = Inputs::materialize(layout);
                let cold = inputs.probe(layout, proof)?;
                let retained = inputs.probe(layout, proof)?;
                let mut value = fragment(&cold);
                if case == "correlation" {
                    value["handshake"]["invocation_nonce"] = json!("4".repeat(64));
                }
                if matches!(case, "target-root" | "provisional-build" | "sibling-build") {
                    let sibling = layout.root().join("build/final-other");
                    let other_root = match case {
                        "target-root" => layout.final_form().target(),
                        "provisional-build" => layout.provisional().build(),
                        _ => {
                            fs::create_dir(&sibling).expect("sibling output root creates");
                            &sibling
                        }
                    };
                    let other_artifact = other_root.join("libdependency.rmeta");
                    fs::write(&other_artifact, ARTIFACT)
                        .expect("same bytes in another root publish");
                    value["handshake"]["loaded_crates"][0]["artifacts"] = json!([other_artifact]);
                }
                let bytes = if case == "schema" {
                    b"{}".to_vec()
                } else {
                    serde_json::to_vec(&value).expect("fixture fragment encodes")
                };
                let mut files = Vec::new();
                if case != "missing" {
                    files.push(file(layout, &sidecar(&cold), bytes));
                }
                if case == "extra" {
                    files.push(file(
                        layout,
                        &cold.protocol_directory.join("extra.json"),
                        b"{}".to_vec(),
                    ));
                }
                if case == "artifact" {
                    files.push(file(
                        layout,
                        &cold.artifacts[0].0,
                        b"corrupt rmeta fixture".to_vec(),
                    ));
                }
                let native = process::Native::from_toolchain(proof);
                let step = launch(layout, &native, files, 0);
                let limits = cargo::Limits::standard();
                let cold_error = script(layout, vec![step])
                    .run(|| {
                        cold.run(&native.production(), &limits)?.validate_outputs(
                            &inputs.build,
                            &inputs.graph,
                            &inputs.metadata,
                            &limits,
                            None,
                        )
                    })
                    .expect("the cold exact Probe runs")
                    .err()
                    .expect("corrupt cold outputs reject");
                let retained_error = script(layout, Vec::new())
                    .run(|| {
                        retained.validate_retained(
                            &inputs.build,
                            &inputs.graph,
                            &inputs.metadata,
                            &limits,
                        )
                    })
                    .expect("retained validation runs no child")
                    .err()
                    .expect("the same retained corruption rejects");
                // The cold snapshot now rejects artifact mutation before loaded-
                // artifact joining; legacy retained validation still joins it.
                let cold_expected = if case == "artifact" {
                    "compiler-namespace"
                } else {
                    expected
                };
                assert_eq!(rejection(cold_error), cold_expected, "cold {case}");
                assert_eq!(rejection(retained_error), expected, "retained {case}");
                Ok(())
            })
            .expect("corrupt-output fixture cleans the reservation");
    }
    fixture.retire();
}

#[test]
fn failed_cold_probe_stops_before_output_decode() {
    let fixture = Fixture::open("probe-failure");
    fixture
        .reserve(|layout, proof| {
            let inputs = Inputs::materialize(layout);
            let expected = inputs.probe(layout, proof)?;
            let native = process::Native::from_toolchain(proof);
            let step = launch(
                layout,
                &native,
                vec![file(layout, &sidecar(&expected), b"{}".to_vec())],
                7,
            );
            let plan = inputs.plan(layout, proof)?;
            protocol::reset_accounted_decodes();
            let error = script(layout, vec![step])
                .run(|| {
                    plan.seal(
                        &native.production(),
                        &inputs.build,
                        &inputs.graph,
                        &inputs.metadata,
                        &cargo::Limits::standard(),
                    )
                })
                .expect("the exact failing Probe is the only child")
                .err()
                .expect("failed Probe cannot yield a Capture");
            assert!(matches!(error, Error::Process { .. }));
            assert_eq!(protocol::accounted_decodes(), 0);
            assert_eq!(
                fs::read(layout.extraction().plan()).expect("cold plan was published"),
                expected.plan_bytes
            );
            Ok(())
        })
        .expect("failed Probe reservation cleans up");
    fixture.retire();
}

#[test]
fn completion_is_required_bound_and_retained_by_capture() {
    let fixture = Fixture::open("probe-completion-retention");
    fixture
        .reserve(|layout, proof| {
            let inputs = Inputs::materialize(layout);
            let probe = inputs.probe(layout, proof)?;
            let bytes = serde_json::to_vec(&fragment(&probe)).unwrap();
            let native = process::Native::from_toolchain(proof);
            let step = launch(
                layout,
                &native,
                vec![file(layout, &sidecar(&probe), bytes)],
                0,
            );
            let limits = cargo::Limits::standard();
            let probe = script(layout, vec![step])
                .run(|| probe.run(&native.production(), &limits))
                .unwrap()?;
            let mut capture =
                probe.validate_retained(&inputs.build, &inputs.graph, &inputs.metadata, &limits)?;
            let path = layout.extraction().protocol().join("attempts");
            let original = fs::read(&path).unwrap();
            let completion = protocol::attempt::Completion::decode(&original).unwrap();
            let rows = completion.invocations().collect::<Vec<_>>();
            let mut changed_query = Vec::new();
            protocol::attempt::encode(
                &mut changed_query,
                completion.arguments_digest,
                completion.conservative,
                &rows,
                std::iter::empty(),
                ["/changed-query/libfixture.rmeta"].into_iter(),
            )
            .unwrap();
            protocol::attempt::Completion::decode(&changed_query).unwrap();
            fs::write(&path, changed_query).unwrap();
            assert_eq!(
                rejection(capture.revalidate(&limits).err().unwrap()),
                "compiler-sidecar-drift"
            );
            fs::write(&path, &original).unwrap();
            let mut foreign = original.clone();
            foreign[8] = if foreign[8] == b'a' { b'b' } else { b'a' };
            fs::write(&path, &foreign).unwrap();
            assert!(capture.revalidate(&limits).is_err());
            let replay = || {
                Plan::replay_in(
                    layout.paths(),
                    &inputs.graph,
                    proof,
                    toolchain::HOST,
                    &inputs.build,
                    &inputs.metadata,
                    &limits,
                    layout.budget(),
                )
            };
            assert_eq!(
                rejection(replay().err().unwrap()),
                "compiler-attempt-completion"
            );
            fs::remove_file(&path).unwrap();
            assert_eq!(
                rejection(replay().err().unwrap()),
                "missing-or-extra-sidecar"
            );
            fs::write(&path, original).unwrap();
            replay()?;
            Ok(())
        })
        .unwrap();
    fixture.retire();
}

#[test]
fn retained_validation_keeps_the_original_cumulative_allocation() {
    let fixture = Fixture::open("probe-account");
    fixture
        .reserve(|layout, proof| {
            let inputs = Inputs::materialize(layout);
            let probe = inputs.probe(layout, proof)?;
            let bytes = serde_json::to_vec(&fragment(&probe)).expect("fixture fragment encodes");
            let native = process::Native::from_toolchain(proof);
            let step = launch(
                layout,
                &native,
                vec![file(layout, &sidecar(&probe), bytes)],
                0,
            );
            let limits = cargo::Limits::standard();
            let mut probe = script(layout, vec![step])
                .run(|| probe.run(&native.production(), &limits))
                .expect("cold Probe prepares complete outputs")?;
            let before = snapshot(&probe);
            // The canonical item target records and cannot exhaust the
            // account. Only a genuine cause latches the original account, and
            // retained validation keeps that latched account instead of
            // replacing it with a fresh one.
            probe.allocation.refuse();
            protocol::reset_accounted_decodes();
            let error = script(layout, Vec::new())
                .run(|| {
                    probe.validate_retained(&inputs.build, &inputs.graph, &inputs.metadata, &limits)
                })
                .expect("refusal cannot run a child")
                .err()
                .expect("the latched original account is not replaced");
            assert_eq!(rejection(error), crate::extract::REFUSED_ACCOUNT_OPERATION);
            assert_eq!(protocol::accounted_decodes(), 0);
            for (path, modified, bytes) in before {
                assert_eq!(fs::read(&path).unwrap(), bytes);
                assert_eq!(fs::metadata(path).unwrap().modified().unwrap(), modified);
            }
            Ok(())
        })
        .expect("allocation refusal cleans the reserved attempt");
    fixture.retire();
}

#[test]
fn shared_validator_preserves_the_narrow_decode_observation() {
    let fixture = Fixture::open("probe-observer");
    for valid in [true, false] {
        fixture
            .reserve(|layout, proof| {
                let inputs = Inputs::materialize(layout);
                let probe = inputs.probe(layout, proof)?;
                let bytes = if valid {
                    serde_json::to_vec(&fragment(&probe)).expect("fixture fragment encodes")
                } else {
                    b"{}".to_vec()
                };
                let native = process::Native::from_toolchain(proof);
                let step = launch(
                    layout,
                    &native,
                    vec![file(layout, &sidecar(&probe), bytes)],
                    0,
                );
                let limits = cargo::Limits::standard();
                let probe = script(layout, vec![step])
                    .run(|| probe.run(&native.production(), &limits))
                    .expect("only the cold portion has a launch capability")?;
                let evidence = process::EvidenceSession::new();
                let result = script(layout, Vec::new())
                    .run(|| {
                        probe.validate_outputs(
                            &inputs.build,
                            &inputs.graph,
                            &inputs.metadata,
                            &limits,
                            Some(&evidence),
                        )
                    })
                    .expect("the observed validator runs no child");
                if valid {
                    result?;
                } else {
                    assert_eq!(
                        rejection(result.err().expect("malformed sidecar rejects")),
                        "sidecar-schema"
                    );
                }
                assert_eq!(evidence.stages().decode.invocations(), 1);
                assert_eq!(evidence.stages().probe.invocations(), 0);
                assert_eq!(evidence.stages().build.invocations(), 0);
                Ok(())
            })
            .expect("decode observation preserves success and failure");
    }
    fixture.retire();
}
