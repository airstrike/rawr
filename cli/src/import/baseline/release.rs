//! Live release warm-window acceptance, separate from descriptive baseline reports.

use std::io::Write as _;

use rawr_import::metrics::{evidence, report};

use super::{cache, timing, Assets, Error, Project};
use timing::Visibility as _;

const REPORT_BYTES: usize = 8 * 1024 * 1024;

#[derive(serde::Serialize)]
struct Description {
    environment: report::Environment,
    fixture: report::Fixture,
    build: report::Build,
}

#[derive(serde::Serialize)]
struct Observation {
    sample: report::Sample,
    capsule: evidence::capsule::Payload,
    unchanged: cache::Unchanged,
    observer: super::observer::Report,
}

/// A live release timing result; serialized facts cannot reconstruct this proof.
pub(super) struct Qualified {
    description: Description,
    observations: Box<[Observation]>,
    timing: timing::ReleaseSeries,
}

#[derive(serde::Serialize)]
struct Reached<'a> {
    schema: &'static str,
    description: &'a Description,
    observations: &'a [Observation],
}

#[derive(serde::Serialize)]
struct Report<'a> {
    schema: &'static str,
    scenario: report::Scenario,
    command: report::Command,
    description: &'a Description,
    observations: &'a [Observation],
    retained_median_ns: u64,
    retained_max_ns: u64,
}

impl Qualified {
    /// Emit inert facts only while the original eight release proofs remain owned.
    pub(super) fn canonical_bytes(&self) -> Result<Box<[u8]>, Error> {
        if self.observations.len() != super::SAMPLED_TRIALS.len()
            || self
                .timing
                .observations()
                .zip(self.observations.iter())
                .any(|(proof, observed)| {
                    proof.origin_ns() != observed.capsule.t0_ns
                        || !proof
                            .report()
                            .is_ok_and(|facts| facts == observed.sample.timing)
                })
        {
            return Err(invalid(
                "live visibility proofs do not bind the retained samples",
            ));
        }
        encode(&Report {
            schema: "rawr.phase9.release.warm-series/v1",
            scenario: report::Scenario::IdenticalWarm,
            command: report::Command::Run,
            description: &self.description,
            observations: &self.observations,
            retained_median_ns: self.timing.median_ns(),
            retained_max_ns: self.timing.max_ns(),
        })
    }
}

pub(super) fn run(assets: &Assets, project: &Project) -> Result<Qualified, Error> {
    #[cfg(not(target_os = "macos"))]
    return Err(Error::Window {
        reason: "the governed AX retirement requires macOS".into(),
    });

    assets.apply(report::Scenario::IdenticalWarm, project)?;
    let environment = super::environment()?;
    let executable = super::release_artifact("evidence")?;
    super::isolated_cell(
        &executable,
        assets,
        project,
        rawr_import::metrics::cell::corpus_id()?,
        report::Scenario::IdenticalWarm,
        report::Command::Run,
        rawr_import::metrics::cell::Role::Setup,
        rawr_import::metrics::cell::Profile::Inactive,
    )?;
    let build = super::release_build(&executable, &environment)?;
    let fixture = assets.fixture()?;
    let fixture_digest =
        evidence::parse_digest(std::ffi::OsStr::new(fixture.digest_sha256.as_ref()))?;
    let description = Description {
        environment,
        fixture,
        build,
    };
    let mut observations = Vec::with_capacity(super::SAMPLED_TRIALS.len());
    let result = (|| {
        let mut visibilities = Vec::with_capacity(super::SAMPLED_TRIALS.len());
        for trial in super::SAMPLED_TRIALS {
            // Both complete raw snapshots are outside the measured window. The
            // original launch owns every intervening title/CG/AX/capsule step.
            let before = cache::Before::capture(&project.root, &description.environment.target)?;
            let launch = match super::launch_with::<timing::ReleaseOracle>(
                &executable,
                project,
                fixture_digest,
                trial,
            ) {
                Ok(launch) => launch,
                Err(source) => return Err(with_reached(source, &before)),
            };
            let capsule = launch.capsule.clone();
            let observer = launch.observer.clone();
            let unchanged = before
                .finish()
                .map_err(|source| cache_error(source, &capsule))?;
            let (sample, visibility) = super::sampled_launch(launch, trial)
                .map_err(|source| super::observed_launch_error(source, &capsule))?;
            // Preserve even the refused ordinal before applying acceptance gates.
            observations.push(Observation {
                sample,
                capsule,
                unchanged,
                observer,
            });
            visibilities.push(visibility);
            let (current, prior) = observations
                .split_last()
                .ok_or_else(|| invalid("the reached release ordinal disappeared"))?;
            let visibility = visibilities
                .last()
                .ok_or_else(|| invalid("the reached visibility proof disappeared"))?;
            super::oracle::retained(
                &assets.oracle,
                report::Scenario::IdenticalWarm,
                &current.capsule.counters,
                description.fixture.package_list_fallbacks,
                current.capsule.runner.generation_key,
            )
            .map_err(|source| super::observed_launch_error(source.into(), &current.capsule))?;
            validate_next(&description, prior, current, visibility)
                .map_err(|source| super::observed_launch_error(source, &current.capsule))?;
        }
        let retained: [timing::ReleaseVisibility; 5] = visibilities
            .split_off(3)
            .try_into()
            .map_err(|_| invalid("release series omitted a retained observation"))?;
        let warmups: [timing::ReleaseVisibility; 3] = visibilities
            .try_into()
            .map_err(|_| invalid("release series omitted a warmup observation"))?;
        timing::ReleaseSeries::transform(warmups, retained).map_err(Error::from)
    })();
    match result {
        Ok(timing) => Ok(Qualified {
            description,
            observations: observations.into_boxed_slice(),
            timing,
        }),
        Err(source) => {
            let evidence = match encode(&Reached {
                schema: "rawr.phase9.release.reached-series/v1",
                description: &description,
                observations: &observations,
            }) {
                Ok(bytes) => String::from_utf8_lossy(&bytes)
                    .into_owned()
                    .into_boxed_str(),
                Err(error) => format!("unavailable: {error}").into_boxed_str(),
            };
            Err(Error::ReleaseObserved {
                source: Box::new(source),
                evidence,
            })
        }
    }
}

fn validate_next(
    description: &Description,
    prior: &[Observation],
    current: &Observation,
    visibility: &timing::ReleaseVisibility,
) -> Result<(), Error> {
    let capsule = &current.capsule;
    let sample = &current.sample;
    if super::SAMPLED_TRIALS.get(prior.len()) != Some(&sample.trial)
        || !artifact_matches(&description.build, &capsule.executable)
        || super::hex(&capsule.fixture_digest) != description.fixture.digest_sha256
        || capsule.t0_ns != visibility.origin_ns()
        || sample.counters != capsule.counters
        || sample.timing != visibility.report()?
        || sample.cache.bytes_before != sample.cache.bytes_after
        || !sample.cache.durable_mutations.is_empty()
    {
        return Err(invalid(
            "sample, capsule, timing, trial, or raw project cache differs",
        ));
    }
    if prior
        .iter()
        .any(|previous| previous.capsule.launch_id == capsule.launch_id)
    {
        return Err(invalid("one launch cannot satisfy two release ordinals"));
    }
    if let Some(first) = prior.first() {
        if first.sample.cache.state != sample.cache.state
            || !first.unchanged.same_state(&current.unchanged)
        {
            return Err(invalid(
                "raw project cache or shared store changed between release ordinals",
            ));
        }
        stable_identities(&first.sample.counters, &sample.counters)?;
    }
    Ok(())
}

fn artifact_matches(build: &report::Build, actual: &evidence::process::file::Identity) -> bool {
    build.executable_path.as_bytes() == actual.path.as_ref()
        && build.executable_digest_sha256 == super::hex(&actual.digest)
        && build.device == actual.device
        && build.inode == actual.inode
        && build.size == actual.size
}

fn stable_identities(
    first: &rawr_import::metrics::recorder::Snapshot,
    current: &rawr_import::metrics::recorder::Snapshot,
) -> Result<(), Error> {
    let (Some(first_runner), Some(current_runner)) = (&first.runner, &current.runner) else {
        return Err(invalid(
            "release series omitted authenticated runner ownership",
        ));
    };
    if first.pipeline.is_none()
        || current.pipeline.is_none()
        || first.profile.is_none()
        || current.profile.is_none()
        || first.pipeline != current.pipeline
        || first.profile != current.profile
        || first_runner.build_request != current_runner.build_request
        || first_runner.generated_manifest != current_runner.generated_manifest
        || first_runner.generation_key != current_runner.generation_key
        || first_runner.artifact != current_runner.artifact
        || first_runner.executable_digest != current_runner.executable_digest
        || first_runner.executable_bytes != current_runner.executable_bytes
    {
        return Err(invalid(
            "identical warm launch changed a pipeline or artifact identity",
        ));
    }
    Ok(())
}

fn invalid(reason: &'static str) -> Error {
    Error::ReleaseSeries {
        reason: reason.into(),
    }
}

fn with_reached(source: Error, value: &impl serde::Serialize) -> Error {
    Error::ReleaseObserved {
        source: Box::new(source),
        evidence: match encode(value) {
            Ok(bytes) => String::from_utf8_lossy(&bytes)
                .into_owned()
                .into_boxed_str(),
            Err(error) => format!("unavailable: {error}").into_boxed_str(),
        },
    }
}

fn cache_error(source: cache::Error, capsule: &evidence::capsule::Payload) -> Error {
    let reached = source
        .observed()
        .map(encode)
        .or_else(|| source.before().map(encode));
    let source = match reached {
        Some(reached) => Error::ReleaseObserved {
            source: Box::new(source.into()),
            evidence: match reached {
                Ok(bytes) => String::from_utf8_lossy(&bytes)
                    .into_owned()
                    .into_boxed_str(),
                Err(error) => format!("unavailable: {error}").into_boxed_str(),
            },
        },
        None => source.into(),
    };
    super::observed_launch_error(source, capsule)
}

pub(super) fn encode(value: &impl serde::Serialize) -> Result<Box<[u8]>, Error> {
    struct Buffer {
        bytes: Vec<u8>,
        exceeded: bool,
    }
    impl std::io::Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.bytes.len().saturating_add(bytes.len()) > REPORT_BYTES {
                self.exceeded = true;
                return Err(std::io::Error::other("release report byte limit"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut buffer = Buffer {
        bytes: Vec::new(),
        exceeded: false,
    };
    let result = serde_json::to_writer_pretty(&mut buffer, value)
        .and_then(|()| buffer.write_all(b"\n").map_err(serde_json::Error::io));
    if buffer.exceeded {
        return Err(Error::Bound {
            field: "release report bytes",
            limit: REPORT_BYTES as u64,
        });
    }
    result.map_err(|error| Error::Report(report::Error::Encode(error)))?;
    Ok(buffer.bytes.into_boxed_slice())
}

#[cfg(test)]
mod tests {
    use rawr_import::metrics::{pipeline, profile, recorder, runner};

    fn snapshot() -> recorder::Snapshot {
        let mut snapshot = recorder::Recorder::transform_at(100, 1)
            .unwrap()
            .snapshot()
            .unwrap();
        snapshot.pipeline = Some(pipeline::Evidence {
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
        snapshot.profile = Some(profile::Evidence::observe(
            profile::Selection::Inactive,
            [12; 32],
            1,
            [13; 32],
        ));
        snapshot.runner = Some(runner::Owned {
            runner_pid: 42,
            runner_uid: 501,
            build_request: [14; 32],
            generated_manifest: [15; 32],
            generation_key: [16; 32],
            epoch: 1,
            artifact: [17; 32],
            executable_path: Box::from(&b"/fixture/runner"[..]),
            executable_digest: [23; 32],
            executable_bytes: 42,
            spawned_ns: Some(1),
        });
        snapshot
    }

    #[test]
    fn fresh_release_processes_retain_content_identity_without_reusing_process_identity() {
        let first = snapshot();
        let mut next = first.clone();
        let runner = next.runner.as_mut().unwrap();
        runner.runner_pid += 1;
        runner.epoch += 1;
        runner.spawned_ns = Some(100);
        assert!(super::stable_identities(&first, &next).is_ok());
        next.runner.as_mut().unwrap().artifact = [99; 32];
        assert!(matches!(
            super::stable_identities(&first, &next),
            Err(super::Error::ReleaseSeries { .. })
        ));
    }

    #[test]
    fn release_series_rejects_absent_or_changed_pipeline_and_profile_evidence() {
        let first = snapshot();
        let mut next = first.clone();
        next.pipeline = None;
        assert!(super::stable_identities(&first, &next).is_err());
        next = first.clone();
        next.pipeline.as_mut().unwrap().overlay = [99; 32];
        assert!(super::stable_identities(&first, &next).is_err());
        next = first.clone();
        next.profile = None;
        assert!(super::stable_identities(&first, &next).is_err());
        next = first.clone();
        next.profile.as_mut().unwrap().version += 1;
        assert!(super::stable_identities(&first, &next).is_err());
    }

    #[test]
    fn release_report_encoding_is_bounded_without_truncating_success() {
        let bytes = super::encode(&["one", "two"]).unwrap();
        assert!(bytes.ends_with(b"\n"));
        let oversized = "x".repeat(super::REPORT_BYTES);
        assert!(matches!(
            super::encode(&oversized),
            Err(super::Error::Bound {
                field: "release report bytes",
                ..
            })
        ));
    }

    #[test]
    fn every_release_capsule_binds_the_exact_recorded_executable() {
        let actual = rawr_import::metrics::evidence::process::file::Identity {
            path: Box::from(&b"/fixture/rawr"[..]),
            digest: [1; 32],
            device: 2,
            inode: 3,
            size: 4,
        };
        let build = rawr_import::metrics::report::Build {
            executable_path: "/fixture/rawr".into(),
            executable_digest_sha256: super::super::hex(&[1; 32]),
            device: 2,
            inode: 3,
            size: 4,
            source_tree: "fixture".into(),
            lockfile_digest_sha256: "fixture".into(),
            compiler: "fixture".into(),
            cargo: "fixture".into(),
            target: "fixture".into(),
            profile: "release".into(),
            features: vec!["import-baseline".into()].into_boxed_slice(),
        };
        assert!(super::artifact_matches(&build, &actual));
        let mut changed = actual.clone();
        changed.digest = [2; 32];
        assert!(!super::artifact_matches(&build, &changed));
        changed = actual.clone();
        changed.inode += 1;
        assert!(!super::artifact_matches(&build, &changed));
        changed = actual;
        changed.path = Box::from(&b"/other/rawr"[..]);
        assert!(!super::artifact_matches(&build, &changed));
    }
}
