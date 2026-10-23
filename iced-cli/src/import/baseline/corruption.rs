//! Deterministic corrupt-bundle repair observations, separate from release proof.

use std::fs;
use std::io::Write as _;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use rawr_import::metrics::{cell, pipeline, principal, recorder::Snapshot, report};

use super::{Assets, Project};

mod failure;
pub(super) mod isolation;
mod mutation;
pub(super) mod seed;
pub(super) use isolation::Home;

const REPORT_BYTES: usize = 8 * 1024 * 1024;
const SCENARIOS: [report::Scenario; 2] = [
    report::Scenario::CorruptRunner,
    report::Scenario::CorruptResolution,
];

/// An inert report of exactly one setup, injection, and ordinary retained child.
#[derive(serde::Serialize)]
pub(super) struct Observed {
    schema: &'static str,
    cell: report::Cell,
    reached: Reached,
    /// Fresh-process cells cannot supply this separate live replacement proof.
    live_last_good: report::Observation<()>,
    /// Full raw evidence is retained; it is not a typed control-write allowlist.
    durable_write_closure: report::Observation<()>,
}

#[derive(Debug, Default, serde::Serialize)]
struct Reached {
    project: PathBuf,
    seed: Option<seed::Evidence>,
    setup: Option<Capsule>,
    retained: Option<Capsule>,
    setup_history: Option<principal::join::Joined>,
    repair_history: Option<principal::join::Repaired>,
    states: Vec<State>,
    mutation: Option<mutation::Injected>,
    post_observation_failure: Option<Box<str>>,
}

#[derive(Debug, serde::Serialize)]
#[serde(tag = "role", content = "payload", rename_all = "kebab-case")]
enum Capsule {
    Setup(cell::SetupCapsule),
    Retained(cell::CellCapsule),
}

impl From<&cell::Capsule> for Capsule {
    fn from(value: &cell::Capsule) -> Self {
        match value {
            cell::Capsule::Setup(value) => Self::Setup(value.clone()),
            cell::Capsule::Retained(value) => Self::Retained(value.clone()),
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub(super) struct State {
    stage: &'static str,
    pub(super) context: [u8; 32],
    pub(super) project: Captured,
    pub(super) shared: Captured,
}

#[derive(Debug, serde::Serialize)]
pub(super) struct Captured {
    root: PathBuf,
    pub(super) identity: Option<[u64; 2]>,
    pub(super) raw: cell::project::Manifest,
}

#[derive(Debug, thiserror::Error)]
pub(super) enum Error {
    #[error(transparent)]
    Owner(#[from] super::Error),
    #[error(transparent)]
    Home(#[from] isolation::Error),
    #[error(transparent)]
    Seed(#[from] seed::Error),
    #[error(transparent)]
    Mutation(#[from] mutation::Error),
    #[error(transparent)]
    Cell(#[from] cell::Error),
    #[error("corruption observation at {path} failed: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("corruption observation is incomplete or inconsistent: {0}")]
    Evidence(&'static str),
    #[error("corruption observation failed: {source}")]
    Reached {
        #[source]
        source: Box<Error>,
        reached: Box<Reached>,
    },
    #[error("corruption report exceeds its original bounded output envelope")]
    Output,
    #[error(transparent)]
    Encode(#[from] serde_json::Error),
}

pub(super) fn observe(
    assets: &Assets,
    project: &Project,
    executable: &Path,
    inputs: &seed::Inputs,
    corpus_id: [u8; 32],
    scenario: report::Scenario,
    command: report::Command,
    environment: report::Environment,
) -> Result<Observed, Error> {
    if !SCENARIOS.contains(&scenario) {
        return Err(Error::Evidence("scenario is not implemented"));
    }
    let home = Home::create(&project.root)?;
    let mut reached = Reached {
        project: project.root.clone(),
        ..Reached::default()
    };
    let result = (|| {
        reached.seed = Some(inputs.copy(&home)?);
        assets.apply(report::Scenario::IdenticalWarm, project)?;
        reached.states.push(State::capture(
            "before-setup",
            &home,
            project,
            &environment.target,
        )?);
        let setup = super::isolated_cell_in(
            executable,
            assets,
            project,
            corpus_id,
            scenario,
            command,
            cell::Role::Setup,
            cell::Profile::Inactive,
            Some(&home),
        )?;
        reached.setup = Some(Capsule::from(&setup.capsule));
        let (setup_counters, setup_generation) = successful(&setup.capsule)?;
        super::oracle::retained(
            &assets.oracle,
            report::Scenario::CleanCold,
            setup_counters,
            assets.fixture()?.package_list_fallbacks,
            setup_generation,
        )
        .map_err(super::Error::from)?;
        reached.states.push(State::capture(
            "prepared",
            &home,
            project,
            &environment.target,
        )?);
        let [before, prepared] = reached.states.as_slice() else {
            return Err(Error::Evidence("setup endpoint cardinality"));
        };
        if before.shared.identity.is_some()
            || prepared.shared.identity.is_none()
            || before.context != prepared.context
            || before.shared.root != prepared.shared.root
        {
            return Err(Error::Evidence(
                "setup changed the isolated store association",
            ));
        }
        reached.setup_history = Some(
            principal::join::setup(
                principal_history(&setup.capsule)?,
                &prepared.shared.root,
                &before.shared.raw,
                &prepared.shared.raw,
            )
            .map_err(Error::Evidence)?,
        );
        let context = home.context(&project.root, &environment.target)?;
        let selected = mutation::Selected::from_setup(&home, &context, scenario, setup_counters)?;
        reached.mutation = Some(selected.inject(&home)?);
        reached.states.push(State::capture(
            "corrupted",
            &home,
            project,
            &environment.target,
        )?);
        validate_injection(&reached)?;
        let retained = super::isolated_cell_in(
            executable,
            assets,
            project,
            corpus_id,
            scenario,
            command,
            cell::Role::RetainedCell,
            cell::Profile::Inactive,
            Some(&home),
        )?;
        reached.retained = Some(Capsule::from(&retained.capsule));
        reached.states.push(State::capture(
            "repaired",
            &home,
            project,
            &environment.target,
        )?);
        let (retained_counters, _) = successful(&retained.capsule)?;
        let targets = validate_repair(scenario, setup_counters, retained_counters, &reached)?;
        let [_, _, corrupted, repaired] = reached.states.as_slice() else {
            return Err(Error::Evidence("repair endpoint cardinality"));
        };
        reached.repair_history = Some(
            principal::join::repair(
                principal_history(&retained.capsule)?,
                &repaired.shared.root,
                &corrupted.shared.raw,
                &repaired.shared.raw,
                targets,
                reached
                    .setup_history
                    .as_ref()
                    .ok_or(Error::Evidence("setup history absent"))?,
            )
            .map_err(Error::Evidence)?,
        );
        super::assemble(
            assets,
            scenario,
            command,
            environment.clone(),
            Some(setup),
            retained,
        )
        .map_err(Error::from)
    })();
    match result {
        Ok(cell) => Ok(Observed {
            schema: "rawr.phase9.corruption.observation/v2", cell, reached,
            live_last_good: super::not_applicable("fresh children do not exercise the separate live last-good replacement control"),
            durable_write_closure: report::Observation::Unavailable { reason: "single-child setup and repair histories join covered principal/control effects and the exact consumed replacement; full durable-write closure remains a separate release gate".into() },
        }),
        Err(source) => {
            // Observation failure never invokes the owner a second time. Preserve
            // the original capsule/error and every endpoint that was reached.
            match State::capture("failed", &home, project, &environment.target) {
                Ok(state) => reached.states.push(state),
                Err(error) => reached.post_observation_failure = Some(error.to_string().into_boxed_str()),
            }
            Err(Error::Reached { source: Box::new(source), reached: Box::new(reached) })
        }
    }
}

impl Observed {
    /// Inspect the original setup and retained capsules without discarding raw evidence.
    pub(super) fn cell(&self) -> &report::Cell {
        &self.cell
    }
}

/// Preserve the initiating failure and every reached endpoint under the existing bound.
pub(super) fn failure_bytes(error: &Error) -> Result<Box<[u8]>, Error> {
    match error {
        Error::Reached { source, reached } => encode(&failure::Record::capture(source, reached)),
        error => encode(&failure::Record::capture(error, &Reached::default())),
    }
}

/// Keep an ordinary owner's typed refusal and any received canonical capsule.
pub(super) fn owner_failure_bytes(error: &super::Error) -> Result<Box<[u8]>, Error> {
    failure::owner(error)
}

fn successful(capsule: &cell::Capsule) -> Result<(&Snapshot, [u8; 32]), Error> {
    let (counters, outcome) = match capsule {
        cell::Capsule::Setup(capsule) => (&capsule.counters, &capsule.outcome),
        cell::Capsule::Retained(capsule) => (&capsule.counters, &capsule.outcome),
    };
    match outcome {
        cell::Outcome::Success { generation } => Ok((counters, *generation)),
        cell::Outcome::Failure { reason } => {
            let command = match capsule {
                cell::Capsule::Setup(value) => value.request.core.owner,
                cell::Capsule::Retained(value) => value.request.core.owner,
            };
            Err(Error::Owner(super::observed_cell_error(
                super::Error::Owner {
                    command,
                    reason: reason.clone(),
                },
                capsule,
            )))
        }
    }
}

fn principal_history(capsule: &cell::Capsule) -> Result<(u32, &principal::Evidence), Error> {
    let (child, counters) = match capsule {
        cell::Capsule::Setup(value) => (&value.child, &value.counters),
        cell::Capsule::Retained(value) => (&value.child, &value.counters),
    };
    Ok((
        child.pid,
        counters
            .principal
            .as_ref()
            .ok_or(Error::Evidence("principal history absent"))?,
    ))
}

impl State {
    pub(super) fn capture(
        stage: &'static str,
        home: &Home,
        project: &Project,
        target: &str,
    ) -> Result<Self, Error> {
        let context = home.context(&project.root, target)?;
        let cargo_rawr = home.root().join("rawr");
        let parent = root(&cargo_rawr)?;
        let project = Captured::read(&project.root.join("target/rawr/cache"))?;
        let shared = Captured::read(&cargo_rawr.join("v1"))?;
        if root(&cargo_rawr)? != parent {
            return Err(Error::Evidence("shared-store parent changed"));
        }
        home.verify()?;
        Ok(Self {
            stage,
            context: context.identity(),
            project,
            shared,
        })
    }
}

impl Captured {
    fn read(path: &Path) -> Result<Self, Error> {
        let identity = root(path)?;
        let raw = cell::project::Manifest::read(path)?;
        if root(path)? != identity {
            return Err(Error::Evidence("cache root changed while observed"));
        }
        Ok(Self {
            root: path.into(),
            identity,
            raw,
        })
    }
}

fn root(path: &Path) -> Result<Option<[u64; 2]>, Error> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(Error::Io {
                path: path.into(),
                source,
            })
        }
    };
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || fs::canonicalize(path).map_err(|source| Error::Io {
            path: path.into(),
            source,
        })? != path
    {
        return Err(Error::Evidence(
            "cache root is not one canonical owned directory",
        ));
    }
    Ok(Some([metadata.dev(), metadata.ino()]))
}

fn validate_injection(reached: &Reached) -> Result<(), Error> {
    let [_, before, after] = reached.states.as_slice() else {
        return Err(Error::Evidence("injection endpoint cardinality"));
    };
    let mutation = reached
        .mutation
        .as_ref()
        .ok_or(Error::Evidence("mutation is absent"))?;
    if before.context != after.context
        || before.project.raw != after.project.raw
        || before.project.identity != after.project.identity
        || before.shared.identity != after.shared.identity
    {
        return Err(Error::Evidence(
            "mutation changed context or another cache root",
        ));
    }
    let raw =
        cell::project::Transition::transform(before.shared.raw.clone(), after.shared.raw.clone());
    let [change] = raw.changes.as_ref() else {
        return Err(Error::Evidence(
            "injection did not change exactly one member",
        ));
    };
    if !raw.directory_changes.is_empty()
        || change.before.as_ref().map(|state| state.mode)
            != change.after.as_ref().map(|state| state.mode)
        || change.path != mutation.relative
        || change.before.as_ref().is_none_or(|before| {
            before.bytes != mutation.before.bytes || before.digest != mutation.before.digest
        })
        || change.after.as_ref().is_none_or(|after| {
            after.bytes != mutation.after.bytes || after.digest != mutation.after.digest
        })
    {
        return Err(Error::Evidence(
            "raw injection differs from the exact observed target",
        ));
    }
    Ok(())
}

fn validate_repair(
    scenario: report::Scenario,
    before: &Snapshot,
    after: &Snapshot,
    reached: &Reached,
) -> Result<(pipeline::target::Target, pipeline::target::Target), Error> {
    let [_, prepared, corrupted, repaired] = reached.states.as_slice() else {
        return Err(Error::Evidence("repair endpoint cardinality"));
    };
    if prepared.context != repaired.context
        || corrupted.context != repaired.context
        || prepared.shared.identity != repaired.shared.identity
        || corrupted.shared.identity != repaired.shared.identity
        || prepared.shared.root != repaired.shared.root
        || corrupted.shared.root != repaired.shared.root
    {
        return Err(Error::Evidence(
            "repair changed the isolated context or store identity",
        ));
    }
    let before_runner = before
        .runner
        .as_ref()
        .ok_or(Error::Evidence("setup runner evidence absent"))?;
    let after_runner = after
        .runner
        .as_ref()
        .ok_or(Error::Evidence("retained runner evidence absent"))?;
    if before_runner.build_request != after_runner.build_request
        || before_runner.generated_manifest != after_runner.generated_manifest
        || (scenario == report::Scenario::CorruptResolution
            && (before_runner.artifact != after_runner.artifact
                || before_runner.executable_path != after_runner.executable_path))
    {
        return Err(Error::Evidence(
            "repair changed an unaffected runner identity",
        ));
    }
    let before = before
        .pipeline
        .as_ref()
        .ok_or(Error::Evidence("setup pipeline absent"))?;
    let after = after
        .pipeline
        .as_ref()
        .ok_or(Error::Evidence("retained pipeline absent"))?;
    let targets = |pipeline: &pipeline::Evidence| -> Result<_, Error> {
        Ok((
            pipeline
                .resolution_target
                .ok_or(Error::Evidence("resolution target absent"))?,
            pipeline
                .runner_target
                .ok_or(Error::Evidence("runner target absent"))?,
        ))
    };
    let (resolution_before, runner_before) = targets(before)?;
    let (resolution_after, runner_after) = targets(after)?;
    let (old, new, unaffected) = match scenario {
        report::Scenario::CorruptRunner if resolution_before == resolution_after => {
            (runner_before, runner_after, resolution_before)
        }
        report::Scenario::CorruptResolution if runner_before == runner_after => {
            (resolution_before, resolution_after, runner_before)
        }
        _ => {
            return Err(Error::Evidence(
                "repair changed an unaffected bundle association",
            ))
        }
    };
    if old == new
        || reached
            .mutation
            .as_ref()
            .is_none_or(|mutation| mutation.target != old)
    {
        return Err(Error::Evidence(
            "ordinary repair did not replace the corrupt bundle",
        ));
    }
    let members = |manifest: &cell::project::Manifest, target: pipeline::target::Target| {
        let prefix = target.bundle_relative();
        manifest
            .members
            .iter()
            .filter(|member| {
                let path = Path::new(std::ffi::OsStr::from_bytes(&member.path));
                path.starts_with(&prefix)
            })
            .cloned()
            .collect::<Vec<_>>()
    };
    let directories = |manifest: &cell::project::Manifest, target: pipeline::target::Target| {
        let prefix = target.bundle_relative();
        manifest
            .directories
            .iter()
            .filter(|directory| {
                Path::new(std::ffi::OsStr::from_bytes(&directory.path)).starts_with(&prefix)
            })
            .cloned()
            .collect::<Vec<_>>()
    };
    if members(&corrupted.shared.raw, old).is_empty()
        || directories(&corrupted.shared.raw, old).is_empty()
        || !members(&repaired.shared.raw, old).is_empty()
        || !directories(&repaired.shared.raw, old).is_empty()
        || !members(&prepared.shared.raw, new).is_empty()
        || !directories(&prepared.shared.raw, new).is_empty()
        || members(&repaired.shared.raw, new).is_empty()
        || directories(&repaired.shared.raw, new).is_empty()
        || members(&prepared.shared.raw, unaffected) != members(&repaired.shared.raw, unaffected)
        || directories(&prepared.shared.raw, unaffected)
            != directories(&repaired.shared.raw, unaffected)
    {
        return Err(Error::Evidence(
            "raw repair did not retire the exact whole unit and preserve the unaffected bundle",
        ));
    }
    if before.toolchain != after.toolchain
        || before.api != after.api
        || before.generic != after.generic
        || before.profile != after.profile
        || before.facade != after.facade
        || before.checked != after.checked
        || before.profile_proof != after.profile_proof
        || before.native_inputs != after.native_inputs
        || before.native_product != after.native_product
        || before.overlay != after.overlay
        || (scenario == report::Scenario::CorruptRunner && before.resolution != after.resolution)
    {
        return Err(Error::Evidence(
            "equal-source repair changed an unaffected semantic identity",
        ));
    }
    Ok((old, new))
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
                return Err(std::io::Error::other("corruption report byte limit"));
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
        return Err(Error::Output);
    }
    result?;
    Ok(buffer.bytes.into_boxed_slice())
}

#[test]
#[ignore = "explicit isolated registry/git inputs and two deterministic corruption rows through five real owners"]
fn record_corruption_owner_cells() {
    let assets = Assets::transform().expect("immutable fixture validates");
    let environment = super::environment().expect("machine identity captures");
    let executable =
        super::release_artifact("evidence").expect("actual evidence executable exists");
    let inputs = seed::Inputs {
        registry: std::env::var_os("RAWR_BASELINE_REGISTRY_SEED")
            .map(PathBuf::from)
            .expect("explicit canonical registry seed root required"),
        git: std::env::var_os("RAWR_BASELINE_GIT_SEED")
            .map(PathBuf::from)
            .expect("explicit canonical git seed root required"),
    };
    let corpus = cell::corpus_id().expect("corpus identity");
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/phase9-08/corruption");
    fs::create_dir_all(&output).expect("evidence directory creates");
    for scenario in SCENARIOS {
        for command in report::Command::ALL {
            let mut project = assets.materialize().expect("exclusive fixture project");
            // Evidence output and retirement failures retain the owned tree too.
            // Only a fully saved successful observation enables ordinary cleanup.
            project.preserve = true;
            let observed = observe(
                &assets,
                &project,
                &executable,
                &inputs,
                corpus,
                scenario,
                command,
                environment.clone(),
            );
            let path = output.join(format!("{scenario:?}-{command:?}.json").to_lowercase());
            match observed {
                Ok(observed) => {
                    fs::write(path, encode(&observed).expect("bounded full observation"))
                        .expect("observation writes");
                    project.preserve = false;
                }
                Err(Error::Reached { source, reached }) => {
                    let failure = failure::Record::capture(&source, &reached);
                    let persisted = encode(&failure).and_then(|bytes| {
                        fs::write(&path, bytes).map_err(|source| Error::Io {
                            path: path.clone(),
                            source,
                        })
                    });
                    panic!("{scenario:?}/{command:?}: {source}; evidence persistence: {persisted:?}; preserved fixture {}", project.root.display());
                }
                Err(error) => {
                    panic!(
                        "{scenario:?}/{command:?}: {error}; preserved fixture {}",
                        project.root.display()
                    );
                }
            }
        }
    }
}
