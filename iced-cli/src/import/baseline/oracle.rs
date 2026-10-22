//! Exact retained-cell comparisons against the immutable fixture oracle.

use rawr_import::metrics::{self, recorder::Snapshot, report};

use super::{Oracle, Outcome};

const EVENTS: [metrics::NodeEvent; 4] = [
    metrics::NodeEvent::ValidatedHit,
    metrics::NodeEvent::ExecutedMiss,
    metrics::NodeEvent::RejectedHit,
    metrics::NodeEvent::Published,
];

#[derive(Debug, thiserror::Error)]
pub(super) enum Error {
    #[error("{scenario:?} has no implemented single retained-cell oracle")]
    Unsupported { scenario: report::Scenario },
    #[error("{scenario:?} has no complete immutable oracle row")]
    Row { scenario: report::Scenario },
    #[error("{scenario:?} omitted, duplicated, or reordered its node counters")]
    Nodes { scenario: report::Scenario },
    #[error("{scenario:?} {kind:?}/{event:?}: expected {expected}, observed {observed}")]
    Node {
        scenario: report::Scenario,
        kind: metrics::NodeKind,
        event: metrics::NodeEvent,
        expected: u64,
        observed: u64,
    },
    #[error("{scenario:?} omitted, duplicated, or reordered its process counters")]
    Processes { scenario: report::Scenario },
    #[error("{scenario:?} {kind:?} children: expected {expected}, observed {observed}")]
    Process {
        scenario: report::Scenario,
        kind: metrics::ProcessKind,
        expected: u64,
        observed: u64,
    },
    #[error("required DAG evidence is absent or invalid: {field}")]
    Identity { field: &'static str },
}

pub(super) fn retained(
    oracle: &Oracle,
    scenario: report::Scenario,
    counters: &Snapshot,
    package_lists: u64,
    generation: [u8; 32],
) -> Result<report::Identities, Error> {
    // The aggregate contention row requires two independently retained capsules.
    // Its frozen E+H cells cannot authorize a synthetic, joined Snapshot here.
    if scenario == report::Scenario::ConcurrentCold {
        return Err(Error::Unsupported { scenario });
    }
    if matches!(
        scenario,
        report::Scenario::CorruptRunner | report::Scenario::CorruptResolution
    ) {
        let pipeline = counters
            .pipeline
            .as_ref()
            .ok_or(Error::Identity { field: "pipeline" })?;
        if pipeline.resolution_target.is_none()
            || pipeline.runner_target.is_none()
            || pipeline.resolution_target == pipeline.runner_target
        {
            return Err(Error::Identity {
                field: "actual owned bundle targets",
            });
        }
    }
    let row = oracle
        .rows
        .iter()
        .find(|row| row.scenario == scenario)
        .filter(|row| row.outcomes.len() == oracle.nodes.len())
        .ok_or(Error::Row { scenario })?;
    if counters.nodes.len() != oracle.nodes.len() * EVENTS.len() {
        return Err(Error::Nodes { scenario });
    }
    for ((kind, outcome), actual) in oracle
        .nodes
        .iter()
        .zip(row.outcomes.iter())
        .zip(counters.nodes.chunks_exact(EVENTS.len()))
    {
        let expected = match outcome {
            Outcome::Hit => [1, 0, 0, 0],
            Outcome::Execute => [0, 1, 0, 1],
            Outcome::RejectExecute => [0, 1, 1, 1],
            Outcome::ExecuteHit => return Err(Error::Unsupported { scenario }),
        };
        for ((event, expected), actual) in EVENTS.into_iter().zip(expected).zip(actual) {
            if actual.kind != *kind || actual.event != event {
                return Err(Error::Nodes { scenario });
            }
            if actual.count != expected {
                return Err(Error::Node {
                    scenario,
                    kind: *kind,
                    event,
                    expected,
                    observed: actual.count,
                });
            }
        }
    }
    let executes = |kind| {
        oracle
            .nodes
            .iter()
            .zip(row.outcomes.iter())
            .find(|(candidate, _)| **candidate == kind)
            .map(|(_, outcome)| {
                u64::from(matches!(outcome, Outcome::Execute | Outcome::RejectExecute))
            })
            .ok_or(Error::Row { scenario })
    };
    let toolchain = executes(metrics::NodeKind::Toolchain)?;
    let resolution = executes(metrics::NodeKind::Resolution)?;
    let runner = executes(metrics::NodeKind::Runner)?;
    // The oracle defines these process roles per miss, independently of observed
    // counts.
    // The fixture manifest supplies P.
    let processes = [
        (metrics::ProcessKind::RustupWhich, Some(3 * toolchain)),
        (metrics::ProcessKind::RustcVersion, Some(toolchain)),
        (metrics::ProcessKind::RustdocVersion, Some(toolchain)),
        (metrics::ProcessKind::CargoVersion, Some(toolchain)),
        (metrics::ProcessKind::DriverVersion, Some(toolchain)),
        (metrics::ProcessKind::ComponentList, Some(toolchain)),
        (metrics::ProcessKind::TargetList, Some(toolchain)),
        (metrics::ProcessKind::CargoResolution, Some(6 * resolution)),
        (
            metrics::ProcessKind::PackageList,
            Some(package_lists * resolution),
        ),
        (metrics::ProcessKind::MetadataProbe, Some(resolution)),
        (metrics::ProcessKind::CargoRunner, Some(runner)),
        (metrics::ProcessKind::Runner, Some(1)),
    ];
    if counters.processes.len() != processes.len() {
        return Err(Error::Processes { scenario });
    }
    for ((kind, expected), actual) in processes.into_iter().zip(counters.processes.iter()) {
        if actual.kind != kind {
            return Err(Error::Processes { scenario });
        }
        if let Some(expected) = expected
            && actual.count != expected
        {
            return Err(Error::Process {
                scenario,
                kind,
                expected,
                observed: actual.count,
            });
        }
    }
    identities(counters, generation)
}

pub(super) fn identities(
    counters: &Snapshot,
    generation: [u8; 32],
) -> Result<report::Identities, Error> {
    let pipeline = counters
        .pipeline
        .as_ref()
        .ok_or(Error::Identity { field: "pipeline" })?;
    for (field, identity) in [
        ("toolchain", pipeline.toolchain),
        ("resolution", pipeline.resolution),
        ("api", pipeline.api),
        ("generic", pipeline.generic),
        ("profile", pipeline.profile),
        ("facade", pipeline.facade),
        ("checked", pipeline.checked),
        ("profile proof", pipeline.profile_proof),
        ("native inputs", pipeline.native_inputs),
        ("native product", pipeline.native_product),
        ("overlay", pipeline.overlay),
    ] {
        if identity == [0; 32] {
            return Err(Error::Identity { field });
        }
    }
    let runner = counters
        .runner
        .as_ref()
        .ok_or(Error::Identity { field: "runner" })?;
    for (field, identity) in [
        ("build request", runner.build_request),
        ("generated manifest", runner.generated_manifest),
        ("built artifact", runner.artifact),
        ("Rust owner", runner.generation_key),
    ] {
        if identity == [0; 32] {
            return Err(Error::Identity { field });
        }
    }
    if runner.generation_key != generation {
        return Err(Error::Identity {
            field: "admitted generation",
        });
    }
    if runner.runner_pid == 0
        || runner.epoch == 0
        || runner.executable_path.is_empty()
        || runner.spawned_ns.is_none()
    {
        return Err(Error::Identity {
            field: "runner ownership",
        });
    }
    let measured = |identity: &[u8; 32]| report::Observation::Measured {
        value: super::hex(identity),
    };
    Ok(report::Identities {
        resolution: measured(&pipeline.resolution),
        api: measured(&pipeline.api),
        checked: measured(&pipeline.checked),
        native_product: measured(&pipeline.native_product),
        overlay: measured(&pipeline.overlay),
        build_request: measured(&runner.build_request),
        generated_manifest: measured(&runner.generated_manifest),
        built_artifact: measured(&runner.artifact),
        rust_owner: measured(&runner.generation_key),
        process_epoch: report::Observation::Measured {
            value: runner.epoch.to_string().into_boxed_str(),
        },
    })
}

#[cfg(test)]
mod tests {
    use rawr_import::metrics::{self, recorder, report};

    use super::{retained, Error};

    fn snapshot(assets: &super::super::Assets, scenario: report::Scenario) -> recorder::Snapshot {
        let mut snapshot = recorder::Recorder::transform_at(100, 1)
            .unwrap()
            .snapshot()
            .unwrap();
        let row = assets
            .oracle
            .rows
            .iter()
            .find(|row| row.scenario == scenario)
            .unwrap();
        for (kind, outcome) in assets.oracle.nodes.iter().zip(row.outcomes.iter()) {
            let events: &[metrics::NodeEvent] = match outcome {
                super::Outcome::Hit => &[metrics::NodeEvent::ValidatedHit],
                super::Outcome::Execute => &[
                    metrics::NodeEvent::ExecutedMiss,
                    metrics::NodeEvent::Published,
                ],
                super::Outcome::RejectExecute => &[
                    metrics::NodeEvent::ExecutedMiss,
                    metrics::NodeEvent::RejectedHit,
                    metrics::NodeEvent::Published,
                ],
                super::Outcome::ExecuteHit => &[
                    metrics::NodeEvent::ValidatedHit,
                    metrics::NodeEvent::ExecutedMiss,
                    metrics::NodeEvent::Published,
                ],
            };
            for count in snapshot
                .nodes
                .iter_mut()
                .filter(|count| count.kind == *kind)
            {
                count.count = u64::from(events.contains(&count.event));
            }
        }
        let misses = |kind| {
            snapshot
                .nodes
                .iter()
                .find(|count| count.kind == kind && count.event == metrics::NodeEvent::ExecutedMiss)
                .unwrap()
                .count
        };
        let toolchain = misses(metrics::NodeKind::Toolchain);
        let resolution = misses(metrics::NodeKind::Resolution);
        let runner = misses(metrics::NodeKind::Runner);
        for count in snapshot.processes.iter_mut() {
            count.count = match count.kind {
                metrics::ProcessKind::RustupWhich => 3 * toolchain,
                metrics::ProcessKind::RustcVersion
                | metrics::ProcessKind::RustdocVersion
                | metrics::ProcessKind::CargoVersion
                | metrics::ProcessKind::DriverVersion
                | metrics::ProcessKind::ComponentList
                | metrics::ProcessKind::TargetList => toolchain,
                metrics::ProcessKind::CargoResolution => 6 * resolution,
                metrics::ProcessKind::PackageList => {
                    assets.fixture().unwrap().package_list_fallbacks * resolution
                }
                metrics::ProcessKind::MetadataProbe => resolution,
                metrics::ProcessKind::CargoRunner => runner,
                metrics::ProcessKind::Runner => 1,
            };
        }
        snapshot.pipeline = Some(metrics::pipeline::Evidence {
            toolchain: [1; 32],
            resolution: [2; 32],
            api: [3; 32],
            generic: [4; 32],
            profile: [5; 32],
            facade: [6; 32],
            checked: [7; 32],
            profile_proof: [8; 32],
            native_inputs: [9; 32],
            native_product: [10; 32],
            overlay: [11; 32],
            resolution_target: None,
            runner_target: None,
        });
        snapshot.runner = Some(metrics::runner::Owned {
            runner_pid: 42,
            runner_uid: 501,
            build_request: [12; 32],
            generated_manifest: [13; 32],
            generation_key: [14; 32],
            epoch: 15,
            artifact: [16; 32],
            executable_path: Box::from(&b"/fixture/runner"[..]),
            executable_digest: [23; 32],
            executable_bytes: 42,
            spawned_ns: Some(1),
        });
        snapshot
    }

    #[test]
    fn every_current_frozen_row_requires_exact_node_and_process_counters() {
        let assets = super::super::Assets::transform().unwrap();
        let package_lists = assets.fixture().unwrap().package_list_fallbacks;
        for scenario in report::Scenario::CURRENT {
            let snapshot = snapshot(&assets, scenario);
            let identities =
                retained(&assets.oracle, scenario, &snapshot, package_lists, [14; 32]).unwrap();
            assert_eq!(
                identities.resolution,
                report::Observation::Measured {
                    value: super::super::hex(&[2; 32])
                }
            );
            assert_eq!(
                identities.api,
                report::Observation::Measured {
                    value: super::super::hex(&[3; 32])
                }
            );
            assert_eq!(
                identities.checked,
                report::Observation::Measured {
                    value: super::super::hex(&[7; 32])
                }
            );
            assert_eq!(
                identities.native_product,
                report::Observation::Measured {
                    value: super::super::hex(&[10; 32])
                }
            );
            assert_eq!(
                identities.overlay,
                report::Observation::Measured {
                    value: super::super::hex(&[11; 32])
                }
            );
            assert_eq!(
                identities.build_request,
                report::Observation::Measured {
                    value: super::super::hex(&[12; 32])
                }
            );
            assert_eq!(
                identities.generated_manifest,
                report::Observation::Measured {
                    value: super::super::hex(&[13; 32])
                }
            );
            assert_eq!(
                identities.rust_owner,
                report::Observation::Measured {
                    value: super::super::hex(&[14; 32])
                }
            );
            assert_eq!(
                identities.process_epoch,
                report::Observation::Measured { value: "15".into() }
            );
            assert_eq!(
                identities.built_artifact,
                report::Observation::Measured {
                    value: super::super::hex(&[16; 32])
                }
            );
            for other in report::Scenario::CURRENT {
                if other != scenario {
                    assert!(
                        matches!(
                            retained(&assets.oracle, other, &snapshot, package_lists, [14; 32]),
                            Err(Error::Node { .. })
                        ),
                        "{scenario:?} cannot satisfy {other:?}"
                    );
                }
            }
            for index in 0..snapshot.nodes.len() {
                let mut changed = snapshot.clone();
                changed.nodes[index].count += 1;
                assert!(
                    matches!(
                        retained(&assets.oracle, scenario, &changed, package_lists, [14; 32]),
                        Err(Error::Node { .. })
                    ),
                    "{scenario:?} node {index}"
                );
            }
            for index in 0..snapshot.processes.len() {
                let mut changed = snapshot.clone();
                changed.processes[index].count += 1;
                let result = retained(&assets.oracle, scenario, &changed, package_lists, [14; 32]);
                assert!(
                    matches!(result, Err(Error::Process { .. })),
                    "{scenario:?} process {index}"
                );
            }
        }
    }

    #[test]
    fn first_counter_mismatch_is_exact_and_keeps_the_original_snapshot() {
        let assets = super::super::Assets::transform().unwrap();
        let scenario = report::Scenario::IdenticalWarm;
        let mut snapshot = snapshot(&assets, scenario);
        snapshot.nodes[0].count = 0;
        snapshot.nodes[1].count = 2;
        snapshot.processes[0].count = 99;
        let original = snapshot.clone();
        assert!(matches!(
            retained(
                &assets.oracle,
                scenario,
                &snapshot,
                assets.fixture().unwrap().package_list_fallbacks,
                [14; 32]
            ),
            Err(Error::Node {
                scenario: report::Scenario::IdenticalWarm,
                kind: metrics::NodeKind::Toolchain,
                event: metrics::NodeEvent::ValidatedHit,
                expected: 1,
                observed: 0,
            })
        ));
        assert_eq!(snapshot, original);
    }

    #[test]
    fn missing_or_ambiguous_counter_rows_are_not_zero_evidence() {
        let assets = super::super::Assets::transform().unwrap();
        let scenario = report::Scenario::IdenticalWarm;
        let snapshot = snapshot(&assets, scenario);
        let package_lists = assets.fixture().unwrap().package_list_fallbacks;
        let mut changed = snapshot.clone();
        changed.nodes = changed.nodes[1..].into();
        assert!(matches!(
            retained(&assets.oracle, scenario, &changed, package_lists, [14; 32]),
            Err(Error::Nodes { .. })
        ));
        changed = snapshot.clone();
        changed.nodes[1] = changed.nodes[0];
        assert!(matches!(
            retained(&assets.oracle, scenario, &changed, package_lists, [14; 32]),
            Err(Error::Nodes { .. })
        ));
        changed = snapshot.clone();
        changed.processes = changed.processes[1..].into();
        assert!(matches!(
            retained(&assets.oracle, scenario, &changed, package_lists, [14; 32]),
            Err(Error::Processes { .. })
        ));
        changed = snapshot;
        changed.processes[1] = changed.processes[0];
        assert!(matches!(
            retained(&assets.oracle, scenario, &changed, package_lists, [14; 32]),
            Err(Error::Processes { .. })
        ));
    }

    #[test]
    fn absent_pipeline_and_mismatched_admitted_owner_refuse_required_evidence() {
        let assets = super::super::Assets::transform().unwrap();
        let scenario = report::Scenario::IdenticalWarm;
        let snapshot = snapshot(&assets, scenario);
        let package_lists = assets.fixture().unwrap().package_list_fallbacks;
        let mut changed = snapshot.clone();
        changed.pipeline = None;
        assert!(matches!(
            retained(&assets.oracle, scenario, &changed, package_lists, [14; 32]),
            Err(Error::Identity { field: "pipeline" })
        ));
        changed = snapshot.clone();
        changed.pipeline.as_mut().unwrap().overlay = [0; 32];
        assert!(matches!(
            retained(&assets.oracle, scenario, &changed, package_lists, [14; 32]),
            Err(Error::Identity { field: "overlay" })
        ));
        assert!(matches!(
            retained(&assets.oracle, scenario, &snapshot, package_lists, [99; 32]),
            Err(Error::Identity {
                field: "admitted generation"
            })
        ));
        changed = snapshot;
        changed.runner = None;
        assert!(matches!(
            retained(&assets.oracle, scenario, &changed, package_lists, [14; 32]),
            Err(Error::Identity { field: "runner" })
        ));
    }

    #[test]
    fn corruption_rows_require_actual_targets_and_exact_frozen_counters() {
        let assets = super::super::Assets::transform().unwrap();
        let package_lists = assets.fixture().unwrap().package_list_fallbacks;
        for scenario in [
            report::Scenario::CorruptRunner,
            report::Scenario::CorruptResolution,
        ] {
            let mut snapshot = snapshot(&assets, scenario);
            assert!(matches!(
                retained(&assets.oracle, scenario, &snapshot, package_lists, [14; 32]),
                Err(Error::Identity {
                    field: "actual owned bundle targets"
                })
            ));
            let target = serde_json::from_value(serde_json::json!({
                "execution": vec![1; 32], "bundle": vec![2; 32],
            }))
            .unwrap();
            let runner = serde_json::from_value(serde_json::json!({
                "execution": vec![3; 32], "bundle": vec![4; 32],
            }))
            .unwrap();
            let pipeline = snapshot.pipeline.as_mut().unwrap();
            pipeline.resolution_target = Some(target);
            pipeline.runner_target = Some(runner);
            retained(&assets.oracle, scenario, &snapshot, package_lists, [14; 32]).unwrap();
            for index in 0..snapshot.nodes.len() {
                let mut changed = snapshot.clone();
                changed.nodes[index].count += 1;
                assert!(matches!(
                    retained(&assets.oracle, scenario, &changed, package_lists, [14; 32]),
                    Err(Error::Node { .. })
                ));
            }
            let mut absent = snapshot.clone();
            absent.pipeline.as_mut().unwrap().resolution_target = None;
            assert!(matches!(
                retained(&assets.oracle, scenario, &absent, package_lists, [14; 32]),
                Err(Error::Identity { .. })
            ));
            snapshot.pipeline.as_mut().unwrap().runner_target = None;
            assert!(matches!(
                retained(&assets.oracle, scenario, &snapshot, package_lists, [14; 32]),
                Err(Error::Identity { .. })
            ));
        }
    }

    #[test]
    fn joined_contention_counters_do_not_claim_an_implemented_single_cell() {
        let assets = super::super::Assets::transform().unwrap();
        let scenario = report::Scenario::ConcurrentCold;
        assert!(matches!(
            retained(
                &assets.oracle,
                scenario,
                &snapshot(&assets, scenario),
                assets.fixture().unwrap().package_list_fallbacks,
                [14; 32]
            ),
            Err(Error::Unsupported { .. })
        ));
    }
}
