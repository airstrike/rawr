//! Check two real snapshots directly against immutable E+H expectations.

use std::collections::BTreeSet;

use rawr_import::metrics::{self, cell, recorder::Snapshot, report};

use super::super::{Oracle, Outcome};

#[derive(Debug, serde::Serialize)]
pub(super) struct Verified {
    overlap_ns: u64,
    identities: [report::Identities; 2],
}

#[derive(Debug, thiserror::Error)]
pub(super) enum Error {
    #[error("paired cold evidence is inconsistent: {0}")]
    Binding(&'static str),
    #[error("paired counter arithmetic overflowed")]
    Overflow,
    #[error(transparent)]
    Oracle(#[from] super::super::oracle::Error),
}

impl Verified {
    pub(super) fn transform(
        oracle: &Oracle,
        capsules: [&cell::CellCapsule; 2],
        package_lists: u64,
    ) -> Result<Self, Error> {
        let [first, second] = capsules;
        let [a, b] = [&first.request.core, &second.request.core];
        if a.scenario != report::Scenario::ConcurrentCold
            || b.scenario != a.scenario
            || a.role != cell::Role::RetainedCell
            || b.role != a.role
            || a.profile != cell::Profile::Inactive
            || b.profile != a.profile
            || a.ordinal != 1
            || b.ordinal != 1
            || a.corpus_id != b.corpus_id
            || a.owner != b.owner
            || a.project != b.project
            || a.build != b.build
            || a.fixture_digest != b.fixture_digest
            || a.bounds != b.bounds
            || a.launch_id == b.launch_id
            || first.request.request_digest == second.request.request_digest
            || first.child.pid == second.child.pid
            || first.child.uid != second.child.uid
            || first.executable != second.executable
        {
            return Err(Error::Binding(
                "distinct launches do not bind one exact cold cell",
            ));
        }
        let overlap_ns = overlap(
            [first.owner_entry_ns, first.owner_return_ns],
            [second.owner_entry_ns, second.owner_return_ns],
        )?;
        let mut identities = Vec::with_capacity(2);
        for capsule in capsules {
            let cell::Outcome::Success { generation } = capsule.outcome else {
                return Err(Error::Binding("both production owners must succeed"));
            };
            identities.push(super::super::oracle::identities(
                &capsule.counters,
                generation,
            )?);
            admission(&capsule.counters)?;
        }
        equal(&first.counters, &second.counters)?;
        let runners = [
            first.counters.runner.as_ref(),
            second.counters.runner.as_ref(),
        ];
        let [Some(first_runner), Some(second_runner)] = runners else {
            return Err(Error::Binding("both runner owners required"));
        };
        if BTreeSet::from([
            first.child.pid,
            second.child.pid,
            first_runner.runner_pid,
            second_runner.runner_pid,
        ])
        .len()
            != 4
        {
            return Err(Error::Binding(
                "two owner and two runner processes must be distinct",
            ));
        }
        counts(oracle, [&first.counters, &second.counters], package_lists)?;
        bundles([&first.counters, &second.counters])?;
        temporaries([&first.counters, &second.counters], a.bounds.counter_limit)?;
        let identities = identities
            .try_into()
            .map_err(|_| Error::Binding("identity cardinality"))?;
        Ok(Self {
            overlap_ns,
            identities,
        })
    }
}

fn bundles(snapshots: [&Snapshot; 2]) -> Result<(), Error> {
    for snapshot in snapshots {
        let history = snapshot
            .accounting
            .bundles
            .as_ref()
            .filter(|history| history.fault.is_none())
            .ok_or(Error::Binding(
                "actual bundle history is missing or faulted",
            ))?;
        let pipeline = snapshot
            .pipeline
            .as_ref()
            .ok_or(Error::Binding("bundle history has no retained pipeline"))?;
        for (kind, row, target) in [
            (
                metrics::NodeKind::Resolution,
                history.resolution,
                pipeline.resolution_target,
            ),
            (
                metrics::NodeKind::Runner,
                history.runner,
                pipeline.runner_target,
            ),
        ] {
            let target = target.ok_or(Error::Binding("bundle history has no actual target"))?;
            let executed = snapshot
                .nodes
                .iter()
                .find(|row| row.kind == kind && row.event == metrics::NodeEvent::ExecutedMiss)
                .ok_or(Error::Binding("bundle execution ownership is missing"))?
                .count;
            if !match executed {
                0 => row.untouched(),
                1 => row.retained_once(target),
                _ => false,
            } {
                return Err(Error::Binding("actual bundle allocation, publication, retention or cleanup does not match its executing owner"));
            }
        }
    }
    Ok(())
}

fn temporaries(snapshots: [&Snapshot; 2], counter_limit: u64) -> Result<(), Error> {
    for snapshot in snapshots {
        let history = snapshot
            .accounting
            .publications
            .as_ref()
            .filter(|history| history.closed() && history.bounded(counter_limit))
            .ok_or(Error::Binding(
                "actual temporary history is missing, faulted, failed or incomplete",
            ))?;
        for row in &history.nodes {
            let executed = snapshot
                .nodes
                .iter()
                .find(|count| {
                    count.kind == row.kind && count.event == metrics::NodeEvent::ExecutedMiss
                })
                .ok_or(Error::Binding(
                    "temporary publication execution ownership is missing",
                ))?
                .count;
            if executed > 1
                || !row.action.winners(executed)
                || !row.content.winners(executed)
                || row.index.started > executed
            {
                return Err(Error::Binding("actual action/content publication or index attempt does not belong to its executing owner"));
            }
        }
        // The exact absent prestate cannot require an older allocation's eviction,
        // repair, or adopted temporary. Enrollment and lock-backed pin lifetimes
        // remain separately observed; their durable-write permission is not inferred.
        if history.retirements.iter().any(|row| {
            row.purpose != metrics::publication::Retirement::Enrollment && !row.counts.winners(0)
        }) || !history.recovered_files.winners(0)
            || !history.recovered_directories.winners(0)
        {
            return Err(Error::Binding(
                "cold pair retired an allocation or recovered a pre-existing temporary",
            ));
        }
    }
    Ok(())
}

fn overlap(first: [u64; 2], second: [u64; 2]) -> Result<u64, Error> {
    let start = first[0].max(second[0]);
    let end = first[1].min(second[1]);
    if first[0] >= first[1] || second[0] >= second[1] || start >= end {
        return Err(Error::Binding("actual owner intervals did not overlap"));
    }
    Ok(end - start)
}

fn admission(snapshot: &Snapshot) -> Result<(), Error> {
    for milestone in [
        metrics::Milestone::RunnerSpawned,
        metrics::Milestone::Admitted,
    ] {
        let mut rows = snapshot
            .milestones
            .iter()
            .filter(|row| row.milestone == milestone);
        if rows
            .next()
            .is_none_or(|row| row.count != 1 || row.last_observed_ns == 0)
            || rows.next().is_some()
        {
            return Err(Error::Binding(
                "each real owner needs one observed runner spawn and admission",
            ));
        }
    }
    if snapshot
        .processes
        .iter()
        .find(|row| row.kind == metrics::ProcessKind::Runner)
        .is_none_or(|row| row.count != 1)
    {
        return Err(Error::Binding("each real owner must spawn its own runner"));
    }
    Ok(())
}

fn equal(first: &Snapshot, second: &Snapshot) -> Result<(), Error> {
    let (Some(a), Some(b)) = (&first.pipeline, &second.pipeline) else {
        return Err(Error::Binding("pipeline identities absent"));
    };
    if a != b
        || a.resolution_target.is_none()
        || a.runner_target.is_none()
        || a.resolution_target == a.runner_target
        || first.profile.is_none()
        || first.profile != second.profile
    {
        return Err(Error::Binding(
            "semantic, profile, or actual retained bundle identities differ",
        ));
    }
    let (Some(a), Some(b)) = (&first.runner, &second.runner) else {
        return Err(Error::Binding("runner identities absent"));
    };
    if a.build_request != b.build_request
        || a.generated_manifest != b.generated_manifest
        || a.generation_key != b.generation_key
        || a.artifact != b.artifact
        || a.executable_path != b.executable_path
    {
        return Err(Error::Binding(
            "successful owners did not converge on the same artifact",
        ));
    }
    Ok(())
}

fn counts(oracle: &Oracle, snapshots: [&Snapshot; 2], package_lists: u64) -> Result<(), Error> {
    use super::super::oracle;
    let scenario = report::Scenario::ConcurrentCold;
    let events = [
        metrics::NodeEvent::ValidatedHit,
        metrics::NodeEvent::ExecutedMiss,
        metrics::NodeEvent::RejectedHit,
        metrics::NodeEvent::Published,
    ];
    let row = oracle
        .rows
        .iter()
        .find(|row| row.scenario == scenario)
        .filter(|row| row.outcomes.len() == oracle.nodes.len())
        .ok_or(oracle::Error::Row { scenario })?;
    if snapshots
        .iter()
        .any(|snapshot| snapshot.nodes.len() != oracle.nodes.len() * events.len())
    {
        return Err(oracle::Error::Nodes { scenario }.into());
    }
    for (index, (kind, outcome)) in oracle.nodes.iter().zip(row.outcomes.iter()).enumerate() {
        if !matches!(outcome, Outcome::ExecuteHit) {
            return Err(oracle::Error::Row { scenario }.into());
        }
        for (offset, (event, expected)) in events.into_iter().zip([1, 1, 0, 1]).enumerate() {
            let actual = snapshots.map(|snapshot| &snapshot.nodes[index * events.len() + offset]);
            if actual
                .iter()
                .any(|row| row.kind != *kind || row.event != event)
            {
                return Err(oracle::Error::Nodes { scenario }.into());
            }
            let observed = actual[0]
                .count
                .checked_add(actual[1].count)
                .ok_or(Error::Overflow)?;
            if observed != expected {
                return Err(oracle::Error::Node {
                    scenario,
                    kind: *kind,
                    event,
                    expected,
                    observed,
                }
                .into());
            }
        }
        // A publication belongs to the process that actually executed this node.
        for snapshot in snapshots {
            let node = &snapshot.nodes[index * events.len()..(index + 1) * events.len()];
            if node[0].count.checked_add(node[1].count) != Some(1) || node[1].count != node[3].count
            {
                return Err(Error::Binding(
                    "one process cannot borrow another process's node publication",
                ));
            }
        }
    }
    let expected = [
        (metrics::ProcessKind::RustupWhich, Some(3)),
        (metrics::ProcessKind::RustcVersion, Some(1)),
        (metrics::ProcessKind::RustdocVersion, Some(1)),
        (metrics::ProcessKind::CargoVersion, Some(1)),
        (metrics::ProcessKind::DriverVersion, Some(1)),
        (metrics::ProcessKind::ComponentList, Some(1)),
        (metrics::ProcessKind::TargetList, Some(1)),
        (metrics::ProcessKind::CargoResolution, Some(6)),
        (metrics::ProcessKind::PackageList, Some(package_lists)),
        (metrics::ProcessKind::MetadataProbe, Some(1)),
        (metrics::ProcessKind::CargoRunner, Some(1)),
        (metrics::ProcessKind::Runner, Some(2)),
    ];
    if snapshots
        .iter()
        .any(|snapshot| snapshot.processes.len() != expected.len())
    {
        return Err(oracle::Error::Processes { scenario }.into());
    }
    for (index, (kind, expected)) in expected.into_iter().enumerate() {
        let actual = snapshots.map(|snapshot| snapshot.processes[index]);
        if actual.iter().any(|row| row.kind != kind) {
            return Err(oracle::Error::Processes { scenario }.into());
        }
        let observed = actual[0]
            .count
            .checked_add(actual[1].count)
            .ok_or(Error::Overflow)?;
        if let Some(expected) = expected
            && observed != expected
        {
            return Err(oracle::Error::Process {
                scenario,
                kind,
                expected,
                observed,
            }
            .into());
        }
        for (snapshot, actual) in snapshots.into_iter().zip(actual) {
            let miss = |kind| {
                snapshot
                    .nodes
                    .iter()
                    .find(|row| row.kind == kind && row.event == metrics::NodeEvent::ExecutedMiss)
                    .map(|row| row.count)
                    .ok_or(Error::Binding("node execution absent"))
            };
            let expected = match kind {
                metrics::ProcessKind::RustupWhich => 3 * miss(metrics::NodeKind::Toolchain)?,
                metrics::ProcessKind::RustcVersion
                | metrics::ProcessKind::RustdocVersion
                | metrics::ProcessKind::CargoVersion
                | metrics::ProcessKind::DriverVersion
                | metrics::ProcessKind::ComponentList
                | metrics::ProcessKind::TargetList => miss(metrics::NodeKind::Toolchain)?,
                metrics::ProcessKind::CargoResolution => 6 * miss(metrics::NodeKind::Resolution)?,
                metrics::ProcessKind::PackageList => package_lists
                    .checked_mul(miss(metrics::NodeKind::Resolution)?)
                    .ok_or(Error::Overflow)?,
                metrics::ProcessKind::MetadataProbe => miss(metrics::NodeKind::Resolution)?,
                metrics::ProcessKind::CargoRunner => miss(metrics::NodeKind::Runner)?,
                metrics::ProcessKind::Runner => 1,
            };
            if actual.count != expected {
                return Err(Error::Binding(
                    "primary child count belongs to the process that executed its node",
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use rawr_import::metrics::{self, recorder, report};

    fn snapshots() -> (super::super::super::Assets, [recorder::Snapshot; 2]) {
        let assets = super::super::super::Assets::transform().unwrap();
        let mut first = recorder::Recorder::transform_at(100, 1)
            .unwrap()
            .snapshot()
            .unwrap();
        let mut second = first.clone();
        for row in first.nodes.iter_mut() {
            row.count = u64::from(matches!(
                row.event,
                metrics::NodeEvent::ExecutedMiss | metrics::NodeEvent::Published
            ));
        }
        for row in second.nodes.iter_mut() {
            row.count = u64::from(row.event == metrics::NodeEvent::ValidatedHit);
        }
        for row in first.processes.iter_mut() {
            row.count = match row.kind {
                metrics::ProcessKind::RustupWhich => 3,
                metrics::ProcessKind::CargoResolution => 6,
                metrics::ProcessKind::PackageList => {
                    assets.fixture().unwrap().package_list_fallbacks
                }
                _ => 1,
            };
        }
        second
            .processes
            .iter_mut()
            .find(|row| row.kind == metrics::ProcessKind::Runner)
            .unwrap()
            .count = 1;
        (assets, [first, second])
    }

    fn capsules() -> (super::super::super::Assets, [metrics::cell::CellCapsule; 2]) {
        let (assets, mut snapshots) = snapshots();
        let project = assets.materialize().unwrap();
        let identity = metrics::cell::project::Identity::read(&project.root).unwrap();
        let build =
            metrics::cell::Build::transform(&std::env::current_exe().unwrap(), "source", [4; 32])
                .unwrap();
        let target = |value| {
            serde_json::from_value(
                serde_json::json!({ "execution": ([value; 32]), "bundle": ([value + 1; 32]) }),
            )
            .unwrap()
        };
        let mut capsules = Vec::new();
        for (index, snapshot) in snapshots.iter_mut().enumerate() {
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
                resolution_target: Some(target(21_u8)),
                runner_target: Some(target(31_u8)),
            });
            let row = |target| metrics::bundle::Row {
                target: Some(target),
                allocated: 1,
                initialized: 1,
                seal_started: 1,
                sealed: 1,
                publication_started: 1,
                published: 1,
                retained: 1,
                ..metrics::bundle::Row::default()
            };
            snapshot.accounting.bundles = Some(if index == 0 {
                metrics::bundle::Evidence {
                    resolution: row(target(21_u8)),
                    runner: row(target(31_u8)),
                    fault: None,
                }
            } else {
                metrics::bundle::Evidence::default()
            });
            let mut publications = metrics::publication::Evidence::default();
            for row in &mut publications.nodes {
                let count = u64::from(index == 0);
                row.action = metrics::publication::Counts {
                    started: count,
                    created: count,
                    renamed: count,
                    returned: count,
                    ..metrics::publication::Counts::default()
                };
                row.content = row.action;
            }
            snapshot.accounting.publications = Some(publications);
            snapshot.profile = Some(metrics::profile::Evidence::observe(
                metrics::profile::Selection::Inactive,
                [12; 32],
                1,
                [13; 32],
            ));
            snapshot.runner = Some(metrics::runner::Owned {
                runner_pid: 201 + index as u32,
                runner_uid: 501,
                build_request: [12; 32],
                generated_manifest: [13; 32],
                generation_key: [14; 32],
                epoch: 1,
                artifact: [16; 32],
                executable_path: Box::from(&b"/fixture/runner"[..]),
                executable_digest: [23; 32],
                executable_bytes: 42,
                spawned_ns: Some(22),
            });
            for row in snapshot.milestones.iter_mut() {
                if matches!(
                    row.milestone,
                    metrics::Milestone::RunnerSpawned | metrics::Milestone::Admitted
                ) {
                    row.count = 1;
                    row.last_observed_ns = 25;
                    row.last_elapsed_ns = 24;
                }
            }
            let request = metrics::cell::Request::transform(
                [1; 32],
                [2 + index as u8; 32],
                1,
                build.clone(),
                identity.clone(),
                [3; 32],
                report::Command::Check,
                report::Scenario::ConcurrentCold,
                metrics::cell::Role::RetainedCell,
                metrics::cell::Profile::Inactive,
                1,
            )
            .unwrap();
            capsules.push(serde_json::from_value(serde_json::json!({
                "schema": "rawr.phase9.cell.retained-capsule/v3", "request": request,
                "child": {"pid": 101 + index as u32, "uid": 501}, "executable": build.executable,
                "owner_entry_ns": 10 + index as u64 * 10, "owner_return_ns": 30 + index as u64 * 10,
                "counters": snapshot, "outcome": {"status": "success", "generation": ([14; 32])},
            })).unwrap());
        }
        (assets, capsules.try_into().unwrap())
    }

    #[test]
    fn paired_proof_requires_distinct_children_overlap_and_identical_successful_bundles() {
        let (assets, capsules) = capsules();
        let p = assets.fixture().unwrap().package_list_fallbacks;
        super::Verified::transform(&assets.oracle, [&capsules[0], &capsules[1]], p).unwrap();
        for mutation in 0..11 {
            let mut changed = capsules.clone();
            let second = &mut changed[1];
            match mutation {
                0 => second.child.pid = capsules[0].child.pid,
                1 => second.request.core.launch_id = capsules[0].request.core.launch_id,
                2 => second.owner_entry_ns = capsules[0].owner_return_ns,
                3 => second.counters.pipeline.as_mut().unwrap().resolution_target = None,
                4 => second.counters.pipeline.as_mut().unwrap().overlay = [99; 32],
                5 => second.counters.runner.as_mut().unwrap().artifact = [99; 32],
                6 => {
                    second.counters.runner.as_mut().unwrap().runner_pid =
                        capsules[0].counters.runner.as_ref().unwrap().runner_pid
                }
                7 => {
                    second
                        .counters
                        .milestones
                        .iter_mut()
                        .find(|row| row.milestone == metrics::Milestone::Admitted)
                        .unwrap()
                        .count = 0
                }
                8 => {
                    second.outcome = metrics::cell::Outcome::Failure {
                        reason: "original refusal".into(),
                    }
                }
                9 => second.request.core.project.cache.digest = [99; 32],
                _ => second.counters.profile = None,
            }
            assert!(
                super::Verified::transform(&assets.oracle, [&changed[0], &changed[1]], p).is_err(),
                "mutation {mutation}"
            );
        }
    }

    #[test]
    fn paired_counters_compare_two_snapshots_to_frozen_expectations() {
        let (assets, snapshots) = snapshots();
        let before = snapshots.clone();
        super::counts(
            &assets.oracle,
            [&snapshots[0], &snapshots[1]],
            assets.fixture().unwrap().package_list_fallbacks,
        )
        .unwrap();
        assert_eq!(
            before, snapshots,
            "the oracle never creates or edits a merged Snapshot"
        );
        assert_eq!(
            report::Scenario::CURRENT.len() * report::Command::ALL.len(),
            40
        );
    }

    #[test]
    fn paired_bundle_history_rejects_unknown_duplicate_failed_or_rebound_lifetimes() {
        let (assets, capsules) = capsules();
        let p = assets.fixture().unwrap().package_list_fallbacks;
        for mutation in 0..12 {
            let mut changed = capsules.clone();
            let [first, second] = &mut changed;
            match mutation {
                0 => first.counters.accounting.bundles = None,
                1 => second.counters.accounting.bundles = None,
                2 => {
                    first.counters.accounting.bundles.as_mut().unwrap().fault =
                        Some(metrics::bundle::Fault::Overflow)
                }
                3 => {
                    first.counters.accounting.bundles.as_mut().unwrap().fault =
                        Some(metrics::bundle::Fault::Identity)
                }
                4 => {
                    first
                        .counters
                        .accounting
                        .bundles
                        .as_mut()
                        .unwrap()
                        .resolution
                        .allocated = 2
                }
                5 => {
                    first
                        .counters
                        .accounting
                        .bundles
                        .as_mut()
                        .unwrap()
                        .runner
                        .retained = 0
                }
                6 => {
                    first
                        .counters
                        .accounting
                        .bundles
                        .as_mut()
                        .unwrap()
                        .runner
                        .removal_failed = 1
                }
                7 => {
                    first
                        .counters
                        .accounting
                        .bundles
                        .as_mut()
                        .unwrap()
                        .resolution
                        .target = first.counters.pipeline.as_ref().unwrap().runner_target
                }
                8 => second.counters.accounting.bundles = first.counters.accounting.bundles.take(),
                9 => {
                    let row = &mut first.counters.accounting.bundles.as_mut().unwrap().runner;
                    row.cleanup_started = 1;
                    row.removed = 1;
                }
                10 => {
                    first
                        .counters
                        .accounting
                        .bundles
                        .as_mut()
                        .unwrap()
                        .resolution
                        .initialization_failed = 1
                }
                _ => {
                    second
                        .counters
                        .accounting
                        .bundles
                        .as_mut()
                        .unwrap()
                        .runner
                        .target = second.counters.pipeline.as_ref().unwrap().runner_target
                }
            }
            assert!(
                super::Verified::transform(&assets.oracle, [&changed[0], &changed[1]], p).is_err(),
                "mutation {mutation}"
            );
        }
    }

    #[test]
    fn paired_bundle_history_follows_each_roles_actual_executing_process() {
        let (assets, mut capsules) = capsules();
        let p = assets.fixture().unwrap().package_list_fallbacks;
        let [first, second] = &mut capsules;
        for (a, b) in first
            .counters
            .nodes
            .iter_mut()
            .zip(second.counters.nodes.iter_mut())
        {
            if a.kind == metrics::NodeKind::Runner {
                std::mem::swap(&mut a.count, &mut b.count);
            }
        }
        for (a, b) in first
            .counters
            .processes
            .iter_mut()
            .zip(second.counters.processes.iter_mut())
        {
            if a.kind == metrics::ProcessKind::CargoRunner {
                std::mem::swap(&mut a.count, &mut b.count);
            }
        }
        std::mem::swap(
            &mut first.counters.accounting.bundles.as_mut().unwrap().runner,
            &mut second.counters.accounting.bundles.as_mut().unwrap().runner,
        );
        for (a, b) in first
            .counters
            .accounting
            .publications
            .as_mut()
            .unwrap()
            .nodes
            .iter_mut()
            .zip(
                second
                    .counters
                    .accounting
                    .publications
                    .as_mut()
                    .unwrap()
                    .nodes
                    .iter_mut(),
            )
        {
            if a.kind == metrics::NodeKind::Runner {
                std::mem::swap(a, b);
            }
        }
        super::Verified::transform(&assets.oracle, [&capsules[0], &capsules[1]], p).unwrap();
        let [first, second] = &mut capsules;
        std::mem::swap(
            &mut first
                .counters
                .accounting
                .bundles
                .as_mut()
                .unwrap()
                .resolution,
            &mut second
                .counters
                .accounting
                .bundles
                .as_mut()
                .unwrap()
                .resolution,
        );
        assert!(
            super::Verified::transform(&assets.oracle, [&capsules[0], &capsules[1]], p).is_err(),
            "equal total history cannot relocate the actual resolution executor"
        );
    }

    #[test]
    fn paired_temporary_history_rejects_missing_failed_unclosed_or_relocated_publications() {
        let (assets, capsules) = capsules();
        let p = assets.fixture().unwrap().package_list_fallbacks;
        for mutation in 0..13 {
            let mut changed = capsules.clone();
            let [first, second] = &mut changed;
            match mutation {
                0 => first.counters.accounting.publications = None,
                1 => second.counters.accounting.publications = None,
                2 => {
                    first
                        .counters
                        .accounting
                        .publications
                        .as_mut()
                        .unwrap()
                        .fault = Some(metrics::publication::Fault::Overflow)
                }
                3 => {
                    first
                        .counters
                        .accounting
                        .publications
                        .as_mut()
                        .unwrap()
                        .nodes[0]
                        .action
                        .cleanup_failed = 1
                }
                4 => {
                    first
                        .counters
                        .accounting
                        .publications
                        .as_mut()
                        .unwrap()
                        .nodes[0]
                        .action
                        .renamed = 0
                }
                5 => {
                    first
                        .counters
                        .accounting
                        .publications
                        .as_mut()
                        .unwrap()
                        .nodes[0]
                        .action
                        .unresolved = 1
                }
                6 => {
                    first
                        .counters
                        .accounting
                        .publications
                        .as_mut()
                        .unwrap()
                        .nodes[0]
                        .content
                        .operation_failed = 1
                }
                7 => {
                    std::mem::swap(
                        &mut first
                            .counters
                            .accounting
                            .publications
                            .as_mut()
                            .unwrap()
                            .nodes[0],
                        &mut second
                            .counters
                            .accounting
                            .publications
                            .as_mut()
                            .unwrap()
                            .nodes[0],
                    );
                }
                8 => first
                    .counters
                    .accounting
                    .publications
                    .as_mut()
                    .unwrap()
                    .nodes
                    .swap(0, 1),
                9 => {
                    first
                        .counters
                        .accounting
                        .publications
                        .as_mut()
                        .unwrap()
                        .recovered_files = metrics::publication::Counts {
                        started: 1,
                        adopted: 1,
                        cleanup_started: 1,
                        cleaned: 1,
                        returned: 1,
                        ..metrics::publication::Counts::default()
                    }
                }
                10 => {
                    first
                        .counters
                        .accounting
                        .publications
                        .as_mut()
                        .unwrap()
                        .retirements[1]
                        .counts = metrics::publication::Counts {
                        started: 1,
                        created: 1,
                        cleanup_started: 1,
                        cleaned: 1,
                        returned: 1,
                        ..metrics::publication::Counts::default()
                    }
                }
                11 => {
                    second
                        .counters
                        .accounting
                        .publications
                        .as_mut()
                        .unwrap()
                        .nodes[0]
                        .index = metrics::publication::Counts {
                        started: 1,
                        created: 1,
                        renamed: 1,
                        returned: 1,
                        ..metrics::publication::Counts::default()
                    }
                }
                _ => {
                    first
                        .counters
                        .accounting
                        .publications
                        .as_mut()
                        .unwrap()
                        .pins[0]
                        .counts = metrics::publication::Counts {
                        started: u64::MAX,
                        created: u64::MAX,
                        renamed: u64::MAX,
                        returned: u64::MAX,
                        ..metrics::publication::Counts::default()
                    }
                }
            }
            assert!(
                super::Verified::transform(&assets.oracle, [&changed[0], &changed[1]], p).is_err(),
                "mutation {mutation}"
            );
        }
    }

    #[test]
    fn paired_temporary_conservation_preserves_transient_pin_source_purposes() {
        let (assets, mut capsules) = capsules();
        let history = capsules[0]
            .counters
            .accounting
            .publications
            .as_mut()
            .unwrap();
        let removed = metrics::publication::Counts {
            started: 1,
            created: 1,
            cleanup_started: 1,
            cleaned: 1,
            returned: 1,
            ..metrics::publication::Counts::default()
        };
        history
            .replacements
            .iter_mut()
            .find(|row| row.purpose == metrics::publication::Replacement::PinBind)
            .unwrap()
            .counts = metrics::publication::Counts {
            started: 2,
            created: 2,
            renamed: 2,
            returned: 2,
            ..metrics::publication::Counts::default()
        };
        history
            .replacements
            .iter_mut()
            .find(|row| row.purpose == metrics::publication::Replacement::InFlightRelease)
            .unwrap()
            .retired = removed;
        history
            .pins
            .iter_mut()
            .find(|row| row.role == metrics::publication::Role::InFlight)
            .unwrap()
            .released = removed;
        history
            .retirements
            .iter_mut()
            .find(|row| row.purpose == metrics::publication::Retirement::Enrollment)
            .unwrap()
            .counts = removed;
        super::Verified::transform(
            &assets.oracle,
            [&capsules[0], &capsules[1]],
            assets.fixture().unwrap().package_list_fallbacks,
        )
        .unwrap();
    }

    #[test]
    fn paired_counters_reject_overflow_duplicate_publication_and_two_cold_runs() {
        let (assets, snapshots) = snapshots();
        let p = assets.fixture().unwrap().package_list_fallbacks;
        let mut changed = snapshots.clone();
        changed[0].nodes[0].count = u64::MAX;
        assert!(matches!(
            super::counts(&assets.oracle, [&changed[0], &changed[1]], p),
            Err(super::Error::Overflow)
        ));
        let mut changed = snapshots.clone();
        changed[1].nodes[3].count = 1;
        assert!(super::counts(&assets.oracle, [&changed[0], &changed[1]], p).is_err());
        assert!(super::counts(&assets.oracle, [&snapshots[0], &snapshots[0]], p).is_err());
        let mut changed = snapshots.clone();
        changed[0].nodes[3].count = 0;
        changed[1].nodes[3].count = 1;
        assert!(
            super::counts(&assets.oracle, [&changed[0], &changed[1]], p).is_err(),
            "equal sums cannot relocate actual publication ownership"
        );
    }

    #[test]
    fn paired_counters_reject_reordering_and_missing_or_excess_primary_children() {
        let (assets, snapshots) = snapshots();
        let p = assets.fixture().unwrap().package_list_fallbacks;
        for index in 0..12 {
            let mut changed = snapshots.clone();
            changed[1].processes[index].count += 1;
            assert!(super::counts(&assets.oracle, [&changed[0], &changed[1]], p).is_err());
        }
        let mut changed = snapshots.clone();
        changed[0].processes[0].count = 0;
        changed[1].processes[0].count = 3;
        assert!(
            super::counts(&assets.oracle, [&changed[0], &changed[1]], p).is_err(),
            "equal aggregate counts cannot move toolchain children into the other process"
        );
        let mut changed = snapshots.clone();
        changed[1].nodes.swap(0, 1);
        assert!(super::counts(&assets.oracle, [&changed[0], &changed[1]], p).is_err());
    }

    #[test]
    fn paired_owner_overlap_requires_strictly_intersecting_real_intervals() {
        assert_eq!(super::overlap([10, 30], [20, 40]).unwrap(), 10);
        for (a, b) in [
            ([10, 20], [20, 30]),
            ([10, 20], [21, 30]),
            ([20, 10], [15, 30]),
            ([10, 10], [1, 30]),
        ] {
            assert!(super::overlap(a, b).is_err());
        }
    }
}
