use std::path::Path;

use crate::cargo::{unit, Limits};
use crate::input::stored::{Budget, Error};
use crate::input::{build, Allocation, Usage};
use crate::node;

const TARGET: &str = "aarch64-apple-darwin";
const ROOT: &str = "/fixture/aarch64-apple-darwin/build-root";

fn artifact(output: &unit::Output) -> build::Artifact {
    build::Artifact {
        package_id: output.package_id().into(),
        outputs: Box::default(),
        dep_info: Path::new(ROOT).join("debug/build/script.d"),
        key: build::Key {
            package_id: output.package_id().into(),
            target: output.target().into(),
            target_kinds: output.target_kinds().into(),
            crate_types: output.crate_types().into(),
            crate_name: output.crate_name().into(),
            features: output.features().into(),
        },
        profile: output.artifact_profile().clone(),
        fresh: false,
    }
}

fn script(target: bool) -> build::Script {
    build::Script {
        compiler: None,
        package_id: "libc".into(),
        out_dir: Path::new(ROOT).join(if target {
            "aarch64-apple-darwin/debug/build/libc/out"
        } else {
            "debug/build/libc/out"
        }),
        cfg: Box::default(),
        environment: Box::default(),
    }
}

fn fixture(swapped: bool) -> (unit::BuildGraph, Vec<build::Artifact>) {
    let outputs = [
        unit::Output::fixture(
            "libc",
            "/libc/build.rs",
            "host_script",
            "custom-build",
            "bin",
        ),
        unit::Output::fixture(
            "libc",
            "/libc/build.rs",
            "target_script",
            "custom-build",
            "bin",
        ),
    ];
    let scripts = [
        unit::BuildScript::fixture_compiler(&outputs[usize::from(swapped)], None),
        unit::BuildScript::fixture_compiler(&outputs[usize::from(!swapped)], Some(TARGET)),
    ];
    let artifacts = outputs.iter().map(artifact).collect();
    (
        unit::BuildGraph::fixture_artifacts(Box::from(outputs))
            .with_fixture_scripts(Box::from(scripts)),
        artifacts,
    )
}

#[test]
fn exact_edges_survive_enclosing_target_name_and_swapped_compilers() {
    for swapped in [false, true] {
        let (graph, artifacts) = fixture(swapped);
        let mut scripts = [script(false), script(true)];
        let limits = Limits::standard();
        let work = node::GraphBudget::new();
        let mut account = Allocation::for_proof(Usage { items: 0, bytes: 0 }, &limits);
        super::bind(
            &graph,
            &mut scripts,
            &artifacts,
            Path::new(ROOT),
            TARGET,
            &mut Budget::borrow(&limits, &mut account, &work),
        )
        .expect("exact graph association");
        assert_eq!(scripts[0].compiler, Some((usize::from(swapped), false)));
        assert_eq!(scripts[1].compiler, Some((usize::from(!swapped), true)));
    }
}

#[test]
fn missing_ambiguous_reused_and_outside_associations_reject() {
    for mode in 0..4 {
        let (graph, mut artifacts) = fixture(false);
        let mut scripts = [script(false), script(true)];
        match mode {
            0 => {
                artifacts.pop();
            }
            1 => artifacts.push(artifact(&graph.artifacts()[0])),
            2 => scripts[1] = script(false),
            3 => scripts[1].out_dir = "/outside/out".into(),
            _ => unreachable!(),
        }
        let limits = Limits::standard();
        let work = node::GraphBudget::new();
        let mut account = Allocation::for_proof(Usage { items: 0, bytes: 0 }, &limits);
        assert!(
            super::bind(
                &graph,
                &mut scripts,
                &artifacts,
                Path::new(ROOT),
                TARGET,
                &mut Budget::borrow(&limits, &mut account, &work)
            )
            .is_err(),
            "mode {mode}"
        );
    }
}

#[test]
fn exhausted_decode_axis_does_not_refuse_association_but_a_refused_account_latches() {
    let (graph, artifacts) = fixture(false);
    let mut scripts = [script(false), script(true)];
    let limits = Limits::standard();
    let work = node::GraphBudget::new();
    work.decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
        .expect("consume the decode allowance");
    let mut account = Allocation::for_proof(Usage { items: 0, bytes: 0 }, &limits);
    let mut budget = Budget::borrow(&limits, &mut account, &work);
    // Script association compares admitted values only: no decode event.
    super::bind(
        &graph,
        &mut scripts,
        &artifacts,
        Path::new(ROOT),
        TARGET,
        &mut budget,
    )
    .expect("pure association needs no decode allowance");
    assert_eq!(
        work.census_usage().expect("graph census").0,
        node::MAX_DECODED_BYTES_PER_GRAPH
    );
    // A refused original account still stops the association at its first step.
    assert!(budget.storage(0, usize::MAX).is_err());
    assert!(matches!(budget.check(), Err(Error::Refused)));
    let mut scripts = [script(false), script(true)];
    assert!(super::bind(
        &graph,
        &mut scripts,
        &artifacts,
        Path::new(ROOT),
        TARGET,
        &mut budget
    )
    .is_err());
}

#[test]
fn cold_and_replay_rederive_the_same_mapping_from_current_edges() {
    let (graph, artifacts) = fixture(false);
    let limits = Limits::standard();
    let work = node::GraphBudget::new();
    let build = build::Build {
        evidence: build::Evidence {
            usage: Usage { items: 0, bytes: 0 },
            packages: std::collections::BTreeMap::from([(
                "libc".into(),
                build::Package {
                    manifest: "/libc/Cargo.toml".into(),
                    root: "/libc".into(),
                    build_script: Some("/libc/build.rs".into()),
                },
            )])
            .into(),
            scripts: Box::from([script(false), script(true)]),
            artifacts: artifacts.into_boxed_slice(),
            environment: Box::default(),
        },
        package_files: std::collections::BTreeMap::new(),
    };
    let complete = build
        .correlate(&graph, &limits, &work, Path::new(ROOT), TARGET)
        .expect("cold association");
    let mut retained = build::Retained {
        build: complete.build,
    };
    assert_eq!(
        retained.build.evidence.scripts[0].compiler,
        Some((0, false))
    );
    assert_eq!(retained.build.evidence.scripts[1].compiler, Some((1, true)));
    // Derived associations have no wire authority, even when altered in memory.
    retained.build.evidence.scripts[0].compiler = Some((usize::MAX, true));
    let mut account = Allocation::for_proof(Usage { items: 0, bytes: 0 }, &limits);
    let mut budget = Budget::borrow(&limits, &mut account, &work);
    let bytes = retained
        .encode(&mut budget)
        .expect("retained observations encode");
    for swapped in [false, true] {
        let (current, _) = fixture(swapped);
        let decoded =
            build::Retained::decode(&bytes, &current, Path::new(ROOT), TARGET, &mut budget)
                .expect("current graph association");
        assert_eq!(
            decoded.build.evidence.scripts[0].compiler,
            Some((usize::from(swapped), false))
        );
        assert_eq!(
            decoded.build.evidence.scripts[1].compiler,
            Some((usize::from(!swapped), true))
        );
        assert_eq!(
            decoded
                .encode(&mut budget)
                .expect("unchanged historical bytes"),
            bytes
        );
    }
}
