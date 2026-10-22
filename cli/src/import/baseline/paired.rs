//! One logical concurrent-cold cell retaining two authenticated real executions.

use std::fs;
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::{Path, PathBuf};

use rawr_import::metrics::{cell, principal, report};

use super::{corruption, Assets, Project, Resources};

mod live;
mod oracle;

#[derive(serde::Serialize)]
pub(super) struct Observed {
    schema: &'static str,
    scenario: report::Scenario,
    command: report::Command,
    fixture: report::Fixture,
    environment: report::Environment,
    verified: oracle::Verified,
    reached: Reached,
    project_cache: cell::project::Transition,
    shared_store: cell::project::Transition,
    /// Actual resolution/runner allocations and their own terminal cleanup only.
    bundle_history: report::Observation<()>,
    /// Existing shared-store temporary families, through actual terminal owners.
    publication_temporary_history: report::Observation<()>,
    allocation_and_loser_history: report::Observation<()>,
    principal_publications: principal::join::Joined,
    durable_write_closure: report::Observation<()>,
}

#[derive(Debug, Default, serde::Serialize)]
struct Reached {
    project: PathBuf,
    seed: Option<corruption::seed::Evidence>,
    executions: Vec<Execution>,
    states: Vec<corruption::State>,
    cleanup: Vec<Box<str>>,
    post_observation_failure: Option<Box<str>>,
}

#[derive(Debug, serde::Serialize)]
struct Execution {
    request: cell::Request,
    pid: u32,
    capsule: Option<Capsule>,
    rejected_capsule: Option<Box<str>>,
    wall_ns: Option<u64>,
    /// Each observed process-tree peak stands alone; peaks are never summed.
    resources: Option<Resources>,
    exit: Option<Box<str>>,
}

#[derive(Debug, serde::Serialize)]
#[serde(tag = "role", content = "payload", rename_all = "kebab-case")]
enum Capsule {
    Retained(cell::CellCapsule),
    Setup(cell::SetupCapsule),
}

impl From<cell::Capsule> for Capsule {
    fn from(value: cell::Capsule) -> Self {
        match value {
            cell::Capsule::Retained(value) => Self::Retained(value),
            cell::Capsule::Setup(value) => Self::Setup(value),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(super) enum Error {
    #[error(transparent)]
    Owner(#[from] super::Error),
    #[error(transparent)]
    Home(#[from] corruption::isolation::Error),
    #[error(transparent)]
    Seed(#[from] corruption::seed::Error),
    #[error(transparent)]
    State(#[from] corruption::Error),
    #[error(transparent)]
    Oracle(#[from] oracle::Error),
    #[error("paired observation is incomplete: {0}")]
    Evidence(&'static str),
    #[error("paired observation failed: {source}")]
    Reached {
        #[source]
        source: Box<Error>,
        reached: Box<Reached>,
    },
}

pub(super) fn observe(
    assets: &Assets,
    project: &Project,
    executable: &Path,
    inputs: &corruption::seed::Inputs,
    corpus: [u8; 32],
    command: report::Command,
    environment: report::Environment,
) -> Result<Observed, Error> {
    let fixture = assets.fixture()?;
    let home = corruption::Home::create(&project.root)?;
    let mut reached = Reached {
        project: project.root.clone(),
        ..Reached::default()
    };
    let result = (|| {
        reached.seed = Some(inputs.copy(&home)?);
        assets.apply(report::Scenario::CleanCold, project)?;
        reached.states.push(corruption::State::capture(
            "before-both",
            &home,
            project,
            &environment.target,
        )?);
        let before = &reached.states[0];
        if before.project.identity.is_some()
            || before.shared.identity.is_some()
            || !before.project.raw.members.is_empty()
            || !before.shared.raw.members.is_empty()
            || !before.project.raw.directories.is_empty()
            || !before.shared.raw.directories.is_empty()
        {
            return Err(Error::Evidence(
                "cold state must be absent in both parent-owned cache roots",
            ));
        }
        live::run(
            executable,
            assets,
            project,
            &home,
            corpus,
            command,
            &mut reached,
        )?;
        reached.states.push(corruption::State::capture(
            "after-both-retired",
            &home,
            project,
            &environment.target,
        )?);
        let [first, second] = reached.executions.as_slice() else {
            return Err(Error::Evidence("pair cardinality"));
        };
        let capsules = [first, second].map(|execution| match &execution.capsule {
            Some(Capsule::Retained(capsule)) => Ok(capsule),
            _ => Err(Error::Evidence(
                "two authenticated retained capsules required",
            )),
        });
        let [first, second] = capsules;
        let capsules = [first?, second?];
        for (execution, capsule) in reached.executions.iter().zip(capsules) {
            if execution.request != capsule.request
                || execution.pid != capsule.child.pid
                || execution.resources.is_none()
                || execution.wall_ns.is_none()
            {
                return Err(Error::Evidence(
                    "real child envelope is incomplete or rebound",
                ));
            }
            super::require_owner_accounting(&capsule.counters)?;
            super::require_runner_accounting(&capsule.counters, command, true)?;
        }
        let verified =
            oracle::Verified::transform(&assets.oracle, capsules, fixture.package_list_fallbacks)?;
        let [before, after] = reached.states.as_slice() else {
            return Err(Error::Evidence("combined endpoint cardinality"));
        };
        if before.context != after.context
            || !reached.cleanup.is_empty()
            || capsules
                .iter()
                .any(|capsule| capsule.request.core.project.cache != before.project.raw.state)
        {
            return Err(Error::Evidence(
                "pair lost common prestate, isolated context, or complete retirement",
            ));
        }
        retained_endpoints(capsules[0], &home, &after.shared.raw)?;
        let principal_publications = principal::join::cold_pair(
            [
                (
                    capsules[0].child.pid,
                    capsules[0]
                        .counters
                        .principal
                        .as_ref()
                        .ok_or(Error::Evidence("first principal history absent"))?,
                ),
                (
                    capsules[1].child.pid,
                    capsules[1]
                        .counters
                        .principal
                        .as_ref()
                        .ok_or(Error::Evidence("second principal history absent"))?,
                ),
            ],
            &home.root().join("rawr/v1"),
            &before.shared.raw,
            &after.shared.raw,
        )
        .map_err(Error::Evidence)?;
        let project_cache = cell::project::Transition::transform(
            before.project.raw.clone(),
            after.project.raw.clone(),
        );
        let shared_store = cell::project::Transition::transform(
            before.shared.raw.clone(),
            after.shared.raw.clone(),
        );
        Ok((
            verified,
            project_cache,
            shared_store,
            principal_publications,
        ))
    })();
    match result {
        Ok((verified, project_cache, shared_store, principal_publications)) => Ok(Observed {
            schema: "rawr.phase9.concurrent-cold.observation/v1",
            scenario: report::Scenario::ConcurrentCold, command,
            fixture, environment, verified, reached, project_cache, shared_store, principal_publications,
            bundle_history: report::Observation::Measured { value: () },
            publication_temporary_history: report::Observation::Measured { value: () },
            allocation_and_loser_history: report::Observation::Measured { value: () },
            durable_write_closure: report::Observation::Unavailable { reason: "principal, ledger, pin, scoped retirement, empty-control/directory creation, temporary recovery/thaw, bundle-owned control/mode/cleanup and publication-temporary histories are joined; ordinary external controls, other mode/deletion routes and remaining cases are not a complete durable-write closure".into() },
        }),
        Err(source) => {
            match corruption::State::capture("failed", &home, project, &environment.target) {
                Ok(state) => reached.states.push(state),
                Err(error) => reached.post_observation_failure = Some(error.to_string().into_boxed_str()),
            }
            Err(Error::Reached { source: Box::new(source), reached: Box::new(reached) })
        }
    }
}

impl Observed {
    pub(super) fn capsules(&self) -> Result<[&cell::CellCapsule; 2], Error> {
        let [first, second] = self.reached.executions.as_slice() else {
            return Err(Error::Evidence("pair cardinality"));
        };
        let [first, second] = [first, second].map(|execution| match &execution.capsule {
            Some(Capsule::Retained(capsule)) => Ok(capsule),
            _ => Err(Error::Evidence(
                "two authenticated retained capsules required",
            )),
        });
        Ok([first?, second?])
    }

    pub(super) fn fixture(&self) -> &report::Fixture {
        &self.fixture
    }

    pub(super) fn environment(&self) -> &report::Environment {
        &self.environment
    }
}

pub(super) fn failure_bytes(error: &Error) -> Result<Box<[u8]>, corruption::Error> {
    match error {
        Error::Reached { source, reached } => {
            corruption::encode(&Failure::capture(source, reached))
        }
        error => corruption::encode(&Failure::capture(error, &Reached::default())),
    }
}

fn retained_endpoints(
    capsule: &cell::CellCapsule,
    home: &corruption::Home,
    after: &cell::project::Manifest,
) -> Result<(), Error> {
    let pipeline = capsule
        .counters
        .pipeline
        .as_ref()
        .ok_or(Error::Evidence("pipeline absent"))?;
    let resolution = pipeline
        .resolution_target
        .ok_or(Error::Evidence("resolution target absent"))?;
    let runner = pipeline
        .runner_target
        .ok_or(Error::Evidence("runner target absent"))?;
    let owned = capsule
        .counters
        .runner
        .as_ref()
        .ok_or(Error::Evidence("runner owner absent"))?;
    let executable = PathBuf::from(std::ffi::OsString::from_vec(owned.executable_path.to_vec()));
    let store = home.root().join("rawr/v1");
    if !executable.starts_with(store.join(runner.bundle_relative())) {
        return Err(Error::Evidence(
            "actual runner artifact is outside its observed isolated bundle",
        ));
    }
    let executable = executable
        .strip_prefix(&store)
        .map_err(|_| Error::Evidence("runner store association"))?;
    for path in [
        resolution.record_relative(),
        executable.into(),
        resolution.lease_relative(),
        runner.lease_relative(),
    ] {
        if !after
            .members
            .iter()
            .any(|member| member.path.as_ref() == path.as_os_str().as_bytes())
        {
            return Err(Error::Evidence("an actually retained bundle artifact or lease is absent after both children retired"));
        }
    }
    Ok(())
}

#[derive(serde::Serialize)]
struct Failure<'a> {
    schema: &'static str,
    primary: String,
    causes: Vec<String>,
    reached: &'a Reached,
}

impl<'a> Failure<'a> {
    fn capture(error: &Error, reached: &'a Reached) -> Self {
        let mut causes = Vec::new();
        let mut source = std::error::Error::source(error);
        while let Some(error) = source {
            causes.push(error.to_string());
            source = error.source();
        }
        Self {
            schema: "rawr.phase9.concurrent-cold.failure/v1",
            primary: error.to_string(),
            causes,
            reached,
        }
    }
}

#[test]
#[ignore = "explicit isolated registry/git inputs and five paired logical cold cells; operator only, no qualification"]
fn record_concurrent_cold_owner_cells() {
    let assets = Assets::transform().expect("immutable fixture validates");
    let environment = super::environment().expect("machine identity captures");
    let executable =
        super::release_artifact("evidence").expect("actual evidence executable exists");
    let inputs = corruption::seed::Inputs {
        registry: std::env::var_os("RAWR_BASELINE_REGISTRY_SEED")
            .map(PathBuf::from)
            .expect("explicit canonical registry seed root required"),
        git: std::env::var_os("RAWR_BASELINE_GIT_SEED")
            .map(PathBuf::from)
            .expect("explicit canonical git seed root required"),
    };
    let corpus = cell::corpus_id().expect("corpus identity");
    let output =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/phase9-08/concurrent-cold");
    fs::create_dir_all(&output).expect("evidence directory creates");
    for command in report::Command::ALL {
        let mut project = assets.materialize().expect("exclusive fixture project");
        project.preserve = true;
        let result = observe(
            &assets,
            &project,
            &executable,
            &inputs,
            corpus,
            command,
            environment.clone(),
        );
        let path = output.join(format!("concurrent-cold-{command:?}.json").to_lowercase());
        match result {
            Ok(observed) => {
                fs::write(
                    path,
                    corruption::encode(&observed).expect("bounded full paired observation"),
                )
                .expect("paired observation writes");
                project.preserve = false;
            }
            Err(Error::Reached { source, reached }) => {
                let persistence = corruption::encode(&Failure::capture(&source, &reached))
                    .map(|bytes| fs::write(&path, bytes));
                panic!("concurrent-cold/{command:?}: {source}; persistence: {persistence:?}; preserved fixture {}", project.root.display());
            }
            Err(error) => panic!(
                "concurrent-cold/{command:?}: {error}; preserved fixture {}",
                project.root.display()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn paired_failure_keeps_primary_separate_from_cleanup_and_reached_evidence() {
        let reached = super::Reached {
            cleanup: vec!["later cleanup failure".into()],
            ..super::Reached::default()
        };
        let error = super::Error::Evidence("first refusal");
        let value = serde_json::to_value(super::Failure::capture(&error, &reached)).unwrap();
        assert_eq!(
            value["primary"],
            "paired observation is incomplete: first refusal"
        );
        assert_eq!(value["reached"]["cleanup"][0], "later cleanup failure");
        assert!(value["reached"]["executions"]
            .as_array()
            .unwrap()
            .is_empty());
    }
}
