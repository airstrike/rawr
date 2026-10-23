//! Reproducible baseline evidence harness over immutable fixture assets.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
#[cfg(feature = "import-baseline")]
use std::fs::OpenOptions;
use std::io::Read as _;
#[cfg(feature = "import-baseline")]
use std::io::Write as _;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::MetadataExt as _;
#[cfg(feature = "import-baseline")]
use std::os::unix::fs::OpenOptionsExt as _;
#[cfg(feature = "import-baseline")]
use std::os::unix::fs::PermissionsExt as _;
#[cfg(feature = "import-baseline")]
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
#[cfg(feature = "import-baseline")]
use std::process::{Child, ExitStatus, Stdio};
#[cfg(feature = "import-baseline")]
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "import-baseline")]
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rawr_import::metrics::recorder::Snapshot;
#[cfg(feature = "import-baseline")]
use rawr_import::metrics::report::{Build, Invocation, Preparation, Series};
use rawr_import::metrics::report::{
    Cache, Cell, Command, Environment, Fixture, Observation, Sample, Scenario, Trial,
};
use sha2::{Digest as _, Sha256};

#[cfg(feature = "import-baseline")]
mod cache;
#[cfg(feature = "import-baseline")]
mod corpus;
#[cfg(feature = "import-baseline")]
mod corruption;
#[cfg(feature = "import-baseline")]
mod observer;
#[cfg(feature = "import-baseline")]
mod oracle;
#[cfg(feature = "import-baseline")]
mod paired;
#[cfg(feature = "import-baseline")]
mod provenance;
#[cfg(feature = "import-baseline")]
mod release;
#[cfg(feature = "import-baseline")]
mod stderr;
#[cfg(feature = "import-baseline")]
mod timing;

const ASSET_LIMIT: usize = 32;
const ASSET_BYTES: usize = 256 * 1024;
const OUTPUT_BYTES: usize = 64 * 1024;
const FIXTURE_SCHEMA: &str = "rawr.phase9.baseline.fixture/v1";
const SCENARIO_SCHEMA: &str = "rawr.phase9.baseline.scenarios/v1";
const ORACLE_SCHEMA: &str = "rawr.phase9.counter-oracle/v1";
#[cfg(feature = "import-baseline")]
const SAMPLED_TRIALS: [Trial; 8] = [
    Trial::Warmup { ordinal: 1 },
    Trial::Warmup { ordinal: 2 },
    Trial::Warmup { ordinal: 3 },
    Trial::Measured { ordinal: 1 },
    Trial::Measured { ordinal: 2 },
    Trial::Measured { ordinal: 3 },
    Trial::Measured { ordinal: 4 },
    Trial::Measured { ordinal: 5 },
];

static NEXT: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("baseline fixture I/O failed at {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("baseline fixture JSON failed at {path}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("baseline fixture schema is not the supported v1 schema")]
    FixtureSchema,
    #[error("baseline fixture digest differs from its immutable manifest: expected {expected}, observed {observed}")]
    FixtureDigest {
        expected: Box<str>,
        observed: Box<str>,
    },
    #[error("baseline fixture has no calibrated package-list fallback count: {reason}")]
    PackageLists { reason: Box<str> },
    #[error("baseline fixture exceeds its {field} bound of {limit}")]
    Bound { field: &'static str, limit: u64 },
    #[error("baseline fixture contains a symlink at {path}")]
    Symlink { path: PathBuf },
    #[error("baseline fixture materialization root already exists at {path}")]
    MaterializationCollision { path: PathBuf },
    #[error("the frozen scenario definitions are incomplete or reordered")]
    Scenarios,
    #[error("the frozen node oracle is incomplete or reordered")]
    Oracle,
    #[cfg(feature = "import-baseline")]
    #[error(transparent)]
    Dag(#[from] oracle::Error),
    #[cfg(feature = "import-baseline")]
    #[error(transparent)]
    Cache(#[from] cache::Error),
    #[cfg(feature = "import-baseline")]
    #[error("release series evidence is invalid: {reason}")]
    ReleaseSeries { reason: Box<str> },
    #[cfg(feature = "import-baseline")]
    #[error("{source}\nreached release series: {evidence}")]
    ReleaseObserved {
        #[source]
        source: Box<Error>,
        evidence: Box<str>,
    },
    #[error("profile-rule evidence is unavailable: {reason}")]
    Profile { reason: Box<str> },
    #[error("baseline owner {command:?} failed: {reason}")]
    Owner { command: Command, reason: Box<str> },
    #[cfg(feature = "import-baseline")]
    #[error("{source}\nreached owner evidence: {evidence}")]
    OwnerObserved {
        #[source]
        source: Box<Error>,
        evidence: Box<str>,
    },
    #[cfg(feature = "import-baseline")]
    #[error("{source}\ncell retirement also failed: {cleanup}")]
    CellRetirement {
        #[source]
        source: Box<Error>,
        cleanup: Box<Error>,
    },
    #[error("baseline environment command {program} failed: {reason}")]
    Environment {
        program: &'static str,
        reason: Box<str>,
    },
    #[cfg(feature = "import-baseline")]
    #[error("baseline release artifact is unavailable at {path}: {reason}")]
    Release { path: PathBuf, reason: Box<str> },
    #[cfg(feature = "import-baseline")]
    #[error("baseline fresh-process launch failed: {reason}")]
    Launch { reason: Box<str> },
    #[cfg(feature = "import-baseline")]
    #[error("baseline process sample is invalid: {reason}")]
    Process { reason: Box<str> },
    #[cfg(feature = "import-baseline")]
    #[error("baseline visible-window evidence is invalid: {reason}")]
    Window { reason: Box<str> },
    #[cfg(feature = "import-baseline")]
    #[error("baseline launch failed during {stage}: {source}; {context}")]
    StagedLaunch {
        stage: stderr::Stage,
        #[source]
        source: Box<Error>,
        context: stderr::Context,
    },
    #[cfg(feature = "import-baseline")]
    #[error(transparent)]
    Stderr(#[from] stderr::Error),
    #[cfg(feature = "import-baseline")]
    #[error(transparent)]
    Timing(#[from] timing::Error),
    #[cfg(feature = "import-baseline")]
    #[error(transparent)]
    Evidence(#[from] rawr_import::metrics::evidence::Error),
    #[cfg(feature = "import-baseline")]
    #[error(transparent)]
    CellProtocol(#[from] rawr_import::metrics::cell::Error),
    #[error(transparent)]
    Report(#[from] rawr_import::metrics::report::Error),
    #[cfg(feature = "import-baseline")]
    #[error(transparent)]
    Provenance(#[from] provenance::Error),
    #[cfg(feature = "import-baseline")]
    #[error(transparent)]
    Observer(#[from] observer::protocol::Error),
}

#[derive(serde::Deserialize)]
struct Manifest {
    schema: Box<str>,
    digest_sha256: Box<str>,
    package_list_fallbacks: Option<u64>,
    missing_package_list_reason: Option<Box<str>>,
}

#[derive(serde::Deserialize)]
struct Definitions {
    schema: Box<str>,
    current: Box<[Definition]>,
    after_cutover: Box<[FutureDefinition]>,
}

#[derive(serde::Deserialize)]
struct Definition {
    scenario: Scenario,
    mutation: Option<Box<str>>,
    missing_evidence: Option<Box<str>>,
}

#[derive(serde::Deserialize)]
struct FutureDefinition {
    scenario: Scenario,
    node_evidence: Observation<serde_json::Value>,
}

#[derive(serde::Deserialize)]
struct Oracle {
    schema: Box<str>,
    nodes: Box<[rawr_import::metrics::NodeKind]>,
    rows: Box<[OracleRow]>,
}

#[derive(serde::Deserialize)]
struct OracleRow {
    scenario: Scenario,
    outcomes: Box<[Outcome]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize)]
enum Outcome {
    #[serde(rename = "H")]
    Hit,
    #[serde(rename = "E")]
    Execute,
    #[serde(rename = "XE")]
    RejectExecute,
    #[serde(rename = "E+H")]
    ExecuteHit,
}

struct Assets {
    root: PathBuf,
    manifest: Manifest,
    oracle: Oracle,
}

struct Project {
    root: PathBuf,
    preserve: bool,
}

struct State {
    bytes: u64,
    digest: Box<str>,
}

enum Plan {
    Execute {
        scenario: Scenario,
        command: Command,
    },
}

impl Assets {
    fn transform() -> Result<Self, Error> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/import-baseline");
        let manifest: Manifest = json(&root.join("manifest.json"))?;
        if manifest.schema.as_ref() != FIXTURE_SCHEMA {
            return Err(Error::FixtureSchema);
        }
        let observed = digest_assets(&root)?;
        if manifest.digest_sha256.as_ref() != observed.as_ref() {
            return Err(Error::FixtureDigest {
                expected: manifest.digest_sha256.clone(),
                observed,
            });
        }
        let definitions: Definitions = json(&root.join("scenarios.json"))?;
        validate_definitions(&definitions)?;
        let oracle: Oracle = json(&root.join("oracle.json"))?;
        validate_oracle(&oracle)?;
        Ok(Self {
            root,
            manifest,
            oracle,
        })
    }

    fn fixture(&self) -> Result<Fixture, Error> {
        let package_list_fallbacks =
            self.manifest
                .package_list_fallbacks
                .ok_or_else(|| Error::PackageLists {
                    reason: self
                        .manifest
                        .missing_package_list_reason
                        .clone()
                        .unwrap_or_else(|| "fixture manifest omitted a reason".into()),
                })?;
        Ok(Fixture {
            name: "phase9-baseline".into(),
            digest_sha256: digest_assets(&self.root)?,
            package_list_fallbacks,
        })
    }

    fn materialize(&self) -> Result<Project, Error> {
        let root = std::env::temp_dir().join(format!(
            "rawr-import-baseline-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        self.materialize_at(root)
    }

    fn materialize_at(&self, root: PathBuf) -> Result<Project, Error> {
        match fs::create_dir(&root) {
            Ok(()) => {}
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(Error::MaterializationCollision { path: root });
            }
            Err(source) => {
                return Err(Error::Io { path: root, source });
            }
        }
        let mut entries = fs::read_dir(&root).map_err(|source| Error::Io {
            path: root.clone(),
            source,
        })?;
        if entries
            .next()
            .transpose()
            .map_err(|source| Error::Io {
                path: root.clone(),
                source,
            })?
            .is_some()
        {
            return Err(Error::MaterializationCollision { path: root });
        }
        for relative in [
            "Cargo.toml",
            "native/Cargo.toml",
            "native/src/lib.rs",
            "src/main.rs",
        ] {
            copy(&self.root.join(relative), &root.join(relative))?;
        }
        let root = canonical(&root)?;
        Ok(Project {
            root,
            preserve: false,
        })
    }

    fn apply(&self, scenario: Scenario, project: &Project) -> Result<(), Error> {
        copy(
            &self.root.join("Cargo.toml"),
            &project.root.join("Cargo.toml"),
        )?;
        copy(
            &self.root.join("native/src/lib.rs"),
            &project.root.join("native/src/lib.rs"),
        )?;
        copy(
            &self.root.join("src/main.rs"),
            &project.root.join("src/main.rs"),
        )?;
        match scenario {
            Scenario::CleanCold | Scenario::IdenticalWarm => Ok(()),
            Scenario::RawrOccurrence => copy(
                &self.root.join("mutations/rawr-occurrence.rs"),
                &project.root.join("src/main.rs"),
            ),
            Scenario::RustBodyEqualApi => copy(
                &self.root.join("mutations/rust-body.rs"),
                &project.root.join("native/src/lib.rs"),
            ),
            Scenario::RustPublicApi => copy(
                &self.root.join("mutations/rust-public-api.rs"),
                &project.root.join("native/src/lib.rs"),
            ),
            Scenario::ProfileRule => Ok(()),
            Scenario::DemandOnly => copy(
                &self.root.join("mutations/demand-only.rs"),
                &project.root.join("src/main.rs"),
            ),
            Scenario::CorruptRunner | Scenario::CorruptResolution | Scenario::ConcurrentCold => {
                Err(Error::Scenarios)
            }
        }
    }

    fn plans(&self) -> Box<[Plan]> {
        Scenario::CURRENT
            .into_iter()
            .flat_map(|scenario| {
                Command::ALL.map(move |command| Plan::Execute { scenario, command })
            })
            .collect()
    }
}

impl Project {
    #[cfg(feature = "import-baseline")]
    fn cache(&self) -> Result<State, Error> {
        tree_state(&self.root.join("target/rawr/cache"))
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        if !self.preserve {
            drop(fs::remove_dir_all(&self.root));
        }
    }
}

fn validate_definitions(definitions: &Definitions) -> Result<(), Error> {
    if definitions.schema.as_ref() != SCENARIO_SCHEMA
        || definitions.current.len() != Scenario::CURRENT.len()
        || definitions
            .current
            .iter()
            .map(|definition| definition.scenario)
            .ne(Scenario::CURRENT)
        || definitions.after_cutover.len() != Scenario::AFTER_CUTOVER.len()
        || definitions
            .after_cutover
            .iter()
            .map(|definition| definition.scenario)
            .ne(Scenario::AFTER_CUTOVER)
        || definitions.current.iter().any(|definition| {
            definition.mutation.is_none() || definition.missing_evidence.is_some()
        })
        || definitions.after_cutover.iter().any(|definition| {
            !matches!(definition.node_evidence, Observation::NotApplicable { .. })
        })
    {
        return Err(Error::Scenarios);
    }
    Ok(())
}

fn validate_oracle(oracle: &Oracle) -> Result<(), Error> {
    let nodes = [
        rawr_import::metrics::NodeKind::Toolchain,
        rawr_import::metrics::NodeKind::Resolution,
        rawr_import::metrics::NodeKind::Api,
        rawr_import::metrics::NodeKind::Generic,
        rawr_import::metrics::NodeKind::Profile,
        rawr_import::metrics::NodeKind::Facade,
        rawr_import::metrics::NodeKind::Checked,
        rawr_import::metrics::NodeKind::ProfileProof,
        rawr_import::metrics::NodeKind::NativeProduct,
        rawr_import::metrics::NodeKind::Overlay,
        rawr_import::metrics::NodeKind::Runner,
    ];
    let scenarios = Scenario::CURRENT.into_iter().chain(Scenario::AFTER_CUTOVER);
    if oracle.schema.as_ref() != ORACLE_SCHEMA
        || oracle.nodes.as_ref() != nodes
        || oracle.rows.len() != nodes.len()
        || oracle.rows.iter().map(|row| row.scenario).ne(scenarios)
        || oracle
            .rows
            .iter()
            .any(|row| row.outcomes.len() != nodes.len())
    {
        return Err(Error::Oracle);
    }
    Ok(())
}

fn observe(
    assets: &Assets,
    project: &Project,
    executable: &Path,
    corpus_id: [u8; 32],
    scenario: Scenario,
    command: Command,
    environment: Environment,
) -> Result<Cell, Error> {
    let setup = prepare_cell(assets, project, executable, corpus_id, scenario, command)?;
    let retained = isolated_cell(
        executable,
        assets,
        project,
        corpus_id,
        scenario,
        command,
        rawr_import::metrics::cell::Role::RetainedCell,
        if scenario == Scenario::ProfileRule {
            rawr_import::metrics::cell::Profile::Variant
        } else {
            rawr_import::metrics::cell::Profile::Inactive
        },
    )?;
    assemble(assets, scenario, command, environment, setup, retained)
}

#[cfg(feature = "import-baseline")]
fn assemble(
    assets: &Assets,
    scenario: Scenario,
    command: Command,
    environment: Environment,
    setup: Option<CellLaunch>,
    retained: CellLaunch,
) -> Result<Cell, Error> {
    let retained_capsule = match &retained.capsule {
        rawr_import::metrics::cell::Capsule::Retained(capsule) => capsule.clone(),
        rawr_import::metrics::cell::Capsule::Setup(_) => {
            return Err(Error::Process {
                reason: "retained request returned a setup capsule".into(),
            });
        }
    };
    let setup_capsule = match setup.as_ref().map(|launch| &launch.capsule) {
        Some(rawr_import::metrics::cell::Capsule::Setup(capsule)) => Some(capsule.clone()),
        Some(rawr_import::metrics::cell::Capsule::Retained(_)) => {
            return Err(Error::Process {
                reason: "setup request returned a retained capsule".into(),
            });
        }
        None => None,
    };
    let generation = match retained_capsule.outcome {
        rawr_import::metrics::cell::Outcome::Success { generation } => generation,
        rawr_import::metrics::cell::Outcome::Failure { ref reason } => {
            return Err(observed_cell_error(
                Error::Owner {
                    command,
                    reason: reason.clone(),
                },
                &retained.capsule,
            ));
        }
    };
    let counters = retained_capsule.counters.clone();
    let fixture = assets.fixture()?;
    let identities = oracle::retained(
        &assets.oracle,
        scenario,
        &counters,
        fixture.package_list_fallbacks,
        generation,
    )
    .map_err(|error| observed_cell_error(error.into(), &retained.capsule))?;
    if let Some(setup_capsule) = setup_capsule.as_ref()
        && scenario == Scenario::ProfileRule
    {
        validate_profile_comparison(&setup_capsule.counters, &counters)
            .map_err(|error| observed_cell_error(error, &retained.capsule))?;
    }
    let lifecycle = lifecycle(command);
    let sample = Sample {
        trial: Trial::Exact,
        wall_ns: retained_capsule
            .owner_return_ns
            .saturating_sub(retained_capsule.owner_entry_ns),
        cpu_user_ns: Observation::Measured {
            value: retained.resources.user_ns,
        },
        cpu_system_ns: Observation::Measured {
            value: retained.resources.system_ns,
        },
        peak_rss_bytes: Observation::Measured {
            value: retained.resources.peak_rss_bytes,
        },
        admitted_ns: milestone(&counters, rawr_import::metrics::Milestone::Admitted),
        started_ns: if matches!(command, Command::Run | Command::Preview | Command::Watch) {
            milestone(&counters, rawr_import::metrics::Milestone::Started)
        } else {
            not_applicable("this owner does not start an event-loop child")
        },
        timing: rawr_import::metrics::report::timing::Timing::unavailable(
            "deterministic owner cells do not substitute for external visibility",
        ),
        retirement: rawr_import::metrics::report::timing::Retirement::not_applicable(
            "deterministic owner cells do not enter the AX retirement lifecycle",
        ),
        updates_per_second_milli: not_applicable(
            "this deterministic owner cell is not the native-iced/Rawr update control",
        ),
        responsiveness_ns: not_applicable(
            "this deterministic owner cell is not the Game-of-Life responsiveness control",
        ),
        rollback: not_applicable("this scenario is not an invalid/repair rollback control"),
        last_good_ns: not_applicable("this scenario does not induce a last-good recovery"),
        counters,
        cache: Cache {
            preparation: preparation(scenario).into(),
            state: format!(
                "before={} after={}",
                hex(&retained.cache.before.state.digest),
                hex(&retained.cache.after.state.digest),
            )
            .into_boxed_str(),
            bytes_before: retained.cache.before.state.bytes,
            bytes_after: retained.cache.after.state.bytes,
            durable_mutations: if retained.cache.changes.is_empty()
                && retained.cache.directory_changes.is_empty()
            {
                Box::default()
            } else {
                vec![format!(
                    "project-cache:{}->{} files={} directories={}",
                    hex(&retained.cache.before.state.digest),
                    hex(&retained.cache.after.state.digest),
                    retained.cache.changes.len(),
                    retained.cache.directory_changes.len(),
                )
                .into_boxed_str()]
                .into_boxed_slice()
            },
        },
        identities,
    };
    let invocation = Invocation {
        capsule_digest: retained_capsule.digest()?,
        capsule: retained_capsule,
        launch_wall_ns: retained.wall_ns,
        cpu_user_ns: retained.resources.user_ns,
        cpu_system_ns: retained.resources.system_ns,
        peak_rss_bytes: retained.resources.peak_rss_bytes,
        sampler_count: retained.resources.sampler_count,
        cache: retained.cache,
    };
    let preparation = match (setup, setup_capsule) {
        (Some(setup), Some(capsule)) => Some(Preparation {
            capsule_digest: capsule.digest()?,
            capsule,
            launch_wall_ns: setup.wall_ns,
            cpu_user_ns: setup.resources.user_ns,
            cpu_system_ns: setup.resources.system_ns,
            peak_rss_bytes: setup.resources.peak_rss_bytes,
            sampler_count: setup.resources.sampler_count,
            cache: setup.cache,
        }),
        (None, None) => None,
        (Some(_), None) | (None, Some(_)) => {
            return Err(Error::Process {
                reason: "setup capsule and parent observations lost cardinality".into(),
            });
        }
    };
    Cell::transform_isolated(
        scenario,
        command,
        lifecycle.into(),
        environment,
        fixture,
        preparation,
        invocation,
        sample,
    )
    .map_err(Error::from)
}

fn prepare_cell(
    assets: &Assets,
    project: &Project,
    executable: &Path,
    corpus_id: [u8; 32],
    scenario: Scenario,
    command: Command,
) -> Result<Option<CellLaunch>, Error> {
    let setup_source = match scenario {
        Scenario::CleanCold => {
            assets.apply(Scenario::CleanCold, project)?;
            return Ok(None);
        }
        Scenario::IdenticalWarm
        | Scenario::RawrOccurrence
        | Scenario::RustBodyEqualApi
        | Scenario::RustPublicApi
        | Scenario::ProfileRule
        | Scenario::DemandOnly => Scenario::IdenticalWarm,
        Scenario::CorruptRunner | Scenario::CorruptResolution | Scenario::ConcurrentCold => {
            return Err(Error::Scenarios);
        }
    };
    assets.apply(setup_source, project)?;
    let setup = isolated_cell(
        executable,
        assets,
        project,
        corpus_id,
        scenario,
        command,
        rawr_import::metrics::cell::Role::Setup,
        if scenario == Scenario::ProfileRule {
            rawr_import::metrics::cell::Profile::Baseline
        } else {
            rawr_import::metrics::cell::Profile::Inactive
        },
    )?;
    if scenario != Scenario::IdenticalWarm && scenario != Scenario::ProfileRule {
        assets.apply(scenario, project)?;
    }
    Ok(Some(setup))
}

#[cfg(feature = "import-baseline")]
fn validate_profile_comparison(baseline: &Snapshot, variant: &Snapshot) -> Result<(), Error> {
    use rawr_import::metrics::profile;

    let baseline_profile = baseline.profile.as_ref().ok_or_else(|| Error::Profile {
        reason: "baseline profile evidence is absent".into(),
    })?;
    let variant_profile = variant.profile.as_ref().ok_or_else(|| Error::Profile {
        reason: "variant profile evidence is absent".into(),
    })?;
    let baseline_pipeline = baseline.pipeline.as_ref().ok_or_else(|| Error::Profile {
        reason: "baseline pipeline evidence is absent".into(),
    })?;
    let variant_pipeline = variant.pipeline.as_ref().ok_or_else(|| Error::Profile {
        reason: "variant pipeline evidence is absent".into(),
    })?;
    let baseline_runner = baseline.runner.as_ref().ok_or_else(|| Error::Profile {
        reason: "baseline runner evidence is absent".into(),
    })?;
    let variant_runner = variant.runner.as_ref().ok_or_else(|| Error::Profile {
        reason: "variant runner evidence is absent".into(),
    })?;
    let stable_hits = baseline_pipeline.toolchain == variant_pipeline.toolchain
        && baseline_pipeline.resolution == variant_pipeline.resolution
        && baseline_pipeline.api == variant_pipeline.api
        && baseline_pipeline.generic == variant_pipeline.generic
        && baseline_pipeline.facade == variant_pipeline.facade
        && baseline_pipeline.native_inputs == variant_pipeline.native_inputs;
    let suffix_misses = baseline_pipeline.profile != variant_pipeline.profile
        && baseline_pipeline.checked != variant_pipeline.checked
        && baseline_pipeline.profile_proof != variant_pipeline.profile_proof
        && baseline_pipeline.native_product != variant_pipeline.native_product
        && baseline_pipeline.overlay != variant_pipeline.overlay
        && baseline_runner.generated_manifest != variant_runner.generated_manifest
        && baseline_runner.build_request != variant_runner.build_request
        && baseline_runner.artifact != variant_runner.artifact
        && baseline_runner.generation_key != variant_runner.generation_key;
    if baseline_profile.selection != profile::Selection::Baseline
        || baseline_profile.fixture != profile::Fixture::Baseline
        || variant_profile.selection != profile::Selection::Variant
        || variant_profile.fixture != profile::Fixture::Variant
        || baseline_profile.identity != variant_profile.identity
        || baseline_profile.version == variant_profile.version
        || baseline_profile.rules == variant_profile.rules
        || baseline_profile.host_agreement == variant_profile.host_agreement
        || !stable_hits
        || !suffix_misses
    {
        return Err(Error::Profile {
            reason: "profile-version comparison is not H,H,H,H,E,H,E,E,E,E,E".into(),
        });
    }
    Ok(())
}

#[cfg(feature = "import-baseline")]
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
struct Resources {
    user_ns: u64,
    system_ns: u64,
    peak_rss_bytes: u64,
    sampler_count: u64,
}

#[cfg(feature = "import-baseline")]
#[derive(Default)]
struct GroupUsage {
    resources: Resources,
    cpu: BTreeMap<u32, (u64, u64)>,
    known: BTreeSet<u32>,
    group_by_pid: BTreeMap<u32, u32>,
    live_groups: BTreeSet<u32>,
}

#[cfg(feature = "import-baseline")]
struct ProcessObserver {
    stop: Arc<AtomicBool>,
    state: Arc<Mutex<ProcessObservation>>,
    thread: std::thread::JoinHandle<()>,
}

#[cfg(feature = "import-baseline")]
#[derive(Default)]
struct ProcessObservation {
    usage: GroupUsage,
    alive: bool,
    failure: Option<Box<str>>,
}

#[cfg(feature = "import-baseline")]
impl ProcessObserver {
    fn transform(root: u32) -> Result<Self, Error> {
        let stop = Arc::new(AtomicBool::new(false));
        let mut initial = ProcessObservation::default();
        initial.alive = sample_process_group(root, &mut initial.usage)?;
        let state = Arc::new(Mutex::new(initial));
        let thread_stop = Arc::clone(&stop);
        let thread_state = Arc::clone(&state);
        let thread = std::thread::spawn(move || {
            while !thread_stop.load(Ordering::Acquire) {
                let result = {
                    let mut state = lock_observation(&thread_state);
                    sample_process_group(root, &mut state.usage).map(|alive| state.alive = alive)
                };
                if let Err(error) = result {
                    lock_observation(&thread_state).failure =
                        Some(error.to_string().into_boxed_str());
                    break;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
        });
        Ok(Self {
            stop,
            state,
            thread,
        })
    }

    fn state(&self) -> (bool, Option<Box<str>>, BTreeSet<u32>) {
        let state = lock_observation(&self.state);
        (
            state.alive,
            state.failure.clone(),
            state.usage.live_groups.clone(),
        )
    }

    fn finish(self) -> Result<Resources, Error> {
        self.stop.store(true, Ordering::Release);
        self.thread.join().map_err(|_| Error::Process {
            reason: "process-group observer panicked".into(),
        })?;
        let state = lock_observation(&self.state);
        if let Some(reason) = state.failure.clone() {
            return Err(Error::Process { reason });
        }
        Ok(state.usage.resources)
    }

    fn wait_retired(&self) -> Result<(), Error> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let (alive, failure, _) = self.state();
            if let Some(reason) = failure {
                return Err(Error::Process { reason });
            }
            if !alive {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(Error::Process {
                    reason: "invalidated process group did not retire within ten seconds".into(),
                });
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

#[cfg(feature = "import-baseline")]
fn lock_observation(
    state: &Mutex<ProcessObservation>,
) -> std::sync::MutexGuard<'_, ProcessObservation> {
    match state.lock() {
        Ok(state) => state,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(feature = "import-baseline")]
struct CellLaunch {
    capsule: rawr_import::metrics::cell::Capsule,
    wall_ns: u64,
    resources: Resources,
    cache: rawr_import::metrics::cell::project::Transition,
}

#[cfg(feature = "import-baseline")]
fn observed_cell_error(source: Error, capsule: &rawr_import::metrics::cell::Capsule) -> Error {
    let evidence = match capsule.canonical_bytes() {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(error) => format!("unavailable: {error}"),
    };
    Error::OwnerObserved {
        source: Box::new(source),
        evidence: evidence.into_boxed_str(),
    }
}

#[cfg(feature = "import-baseline")]
fn require_owner_accounting(counters: &Snapshot) -> Result<(), Error> {
    use rawr_import::metrics::{accounting::Scalar, NodeEvent, NodeKind};
    let accounting = &counters.accounting;
    // These cells execute full owner preparation. Resident edits have a different
    // workload and must not be made to repeat these transforms to supply evidence.
    let required = [
        Scalar::CargoRequestItems,
        Scalar::CargoRequestBytes,
        Scalar::OwnerInputItems,
        Scalar::OwnerInputBytes,
        Scalar::CargoCurrentItems,
        Scalar::CargoCurrentBytes,
        Scalar::CargoCurrentStrings,
        Scalar::CargoCurrentContainers,
        Scalar::CargoPeakItems,
        Scalar::CargoPeakBytes,
        Scalar::CargoPeakStrings,
        Scalar::CargoPeakContainers,
        Scalar::LedgerReadBytes,
        Scalar::LedgerEncodedBytes,
        Scalar::ProjectLedgerReadBytes,
        Scalar::ProjectLedgerEncodedBytes,
        Scalar::PinRolesRequested,
        Scalar::PinRolesCommitted,
        Scalar::BundleMeasuredBytes,
        Scalar::BundleMeasuredMembers,
        Scalar::RecoveryMetadataBytes,
        Scalar::RecoveryDeletionBytes,
        Scalar::RecoveryDeletionMembers,
        Scalar::RecoveryBundles,
        Scalar::RecoveryTemporaries,
        Scalar::NativeInventoryItems,
        Scalar::NativeInventoryBytes,
    ];
    if !accounting.qualifies(&required) {
        return Err(Error::Report(
            rawr_import::metrics::report::Error::Accounting,
        ));
    }
    let published = counters
        .nodes
        .iter()
        .any(|row| row.event == NodeEvent::Published && row.count > 0);
    let control = [
        Scalar::ControlInventoryBytes,
        Scalar::ControlInventoryMembers,
    ];
    if (published || control.iter().any(|key| accounting.scalar(*key).is_some()))
        && !accounting.qualifies(&control)
    {
        return Err(Error::Report(
            rawr_import::metrics::report::Error::Accounting,
        ));
    }
    let ordinary_process = counters
        .processes
        .iter()
        .any(|row| !matches!(row.kind, rawr_import::metrics::ProcessKind::Runner) && row.count > 0);
    if ordinary_process
        && !accounting.qualifies(&[
            Scalar::ProcessElapsedNanoseconds,
            Scalar::ProcessStdoutBytes,
            Scalar::ProcessStderrBytes,
        ])
    {
        return Err(Error::Report(
            rawr_import::metrics::report::Error::Accounting,
        ));
    }
    for row in counters
        .nodes
        .iter()
        .filter(|row| row.event == NodeEvent::Published && row.count > 0)
    {
        let mut required = vec![Scalar::NodePayloadBytes(row.kind)];
        match row.kind {
            NodeKind::Resolution => required.extend([
                Scalar::ResolutionMetadataBytes,
                Scalar::ResolutionUnitBytes,
                Scalar::ResolutionBuildBytes,
                Scalar::ResolutionInputBytes,
                Scalar::ResolutionNamespaceBytes,
                Scalar::ResolutionRecordBytes,
            ]),
            NodeKind::Api => required.extend([
                Scalar::ApiCanonicalBytes,
                Scalar::ApiSelectedBytes,
                Scalar::ApiTransactionItems,
                Scalar::ApiTransactionBytes,
                Scalar::ApiRetainedItems,
                Scalar::ApiRetainedBytes,
            ]),
            NodeKind::Generic => required.extend([
                Scalar::ClassifiedItems,
                Scalar::ClassifiedBytes,
                Scalar::ClassifiedScratchBytes,
            ]),
            _ => {}
        }
        if !accounting.qualifies(&required) {
            return Err(Error::Report(
                rawr_import::metrics::report::Error::Accounting,
            ));
        }
    }
    Ok(())
}

#[cfg(feature = "import-baseline")]
fn require_runner_accounting(
    counters: &Snapshot,
    command: Command,
    explicit_shutdown: bool,
) -> Result<(), Error> {
    use rawr_import::protocol::Tag;
    let runner = counters.runner.as_ref().ok_or(Error::Report(
        rawr_import::metrics::report::Error::Accounting,
    ))?;
    let accounting = &counters.accounting;
    let covers =
        |commands: &[Tag]| accounting.native_covers(runner.runner_pid, runner.epoch, commands);
    let starts = matches!(command, Command::Run | Command::Preview | Command::Watch);
    if !covers(&[Tag::Prepare, Tag::Admit])
        || (starts && !covers(&[Tag::Start]))
        || (command == Command::Snapshot && !covers(&[Tag::Snapshot]))
        || (explicit_shutdown && !covers(&[Tag::Shutdown]))
    {
        return Err(Error::Report(
            rawr_import::metrics::report::Error::Accounting,
        ));
    }
    Ok(())
}

#[cfg(feature = "import-baseline")]
#[allow(clippy::too_many_arguments)]
fn isolated_cell(
    executable: &Path,
    assets: &Assets,
    project: &Project,
    corpus_id: [u8; 32],
    scenario: Scenario,
    command: Command,
    role: rawr_import::metrics::cell::Role,
    profile: rawr_import::metrics::cell::Profile,
) -> Result<CellLaunch, Error> {
    isolated_cell_in(
        executable, assets, project, corpus_id, scenario, command, role, profile, None,
    )
}

#[cfg(feature = "import-baseline")]
#[allow(clippy::too_many_arguments)]
fn isolated_cell_in(
    executable: &Path,
    assets: &Assets,
    project: &Project,
    corpus_id: [u8; 32],
    scenario: Scenario,
    command: Command,
    role: rawr_import::metrics::cell::Role,
    profile: rawr_import::metrics::cell::Profile,
    home: Option<&corruption::Home>,
) -> Result<CellLaunch, Error> {
    use rawr_import::metrics::cell;

    let cache_root = project.root.join("target/rawr/cache");
    let cache_before = cell::project::Manifest::read(&cache_root)?;
    let before = cell::project::Identity::read_with_manifest(&project.root, &cache_before)?;
    let fixture = assets.fixture()?;
    let fixture_digest = rawr_import::metrics::evidence::parse_digest(std::ffi::OsStr::new(
        fixture.digest_sha256.as_ref(),
    ))?;
    let listening = cell::Listening::transform()?;
    let launched_ns = rawr_import::metrics::monotonic_ns().ok_or_else(|| Error::Launch {
        reason: "process-shared monotonic clock is unavailable".into(),
    })?;
    let request = cell::Request::transform(
        corpus_id,
        listening.launch_id(),
        launched_ns,
        provenance::load(executable, provenance::Kind::Evidence)?.cell(executable)?,
        before.clone(),
        fixture_digest,
        command,
        scenario,
        role,
        profile,
        match role {
            cell::Role::Setup => 0,
            cell::Role::RetainedCell => 1,
        },
    )?;
    let launch_hex = hex(&listening.launch_id());
    let request_hex = hex(&request.request_digest);
    let mut process = std::process::Command::new(executable);
    process
        .current_dir(&project.root)
        .arg("--baseline-cell-v1")
        .arg(listening.socket())
        .arg(launch_hex.as_ref())
        .arg(request_hex.as_ref())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    if let Some(home) = home {
        home.configure(&mut process)
            .map_err(|source| Error::Launch {
                reason: source.to_string().into_boxed_str(),
            })?;
    }
    let started = Instant::now();
    let mut child = process.spawn().map_err(|error| Error::Launch {
        reason: error.to_string().into_boxed_str(),
    })?;
    let child_id = child.id();
    let observer = match ProcessObserver::transform(child_id) {
        Ok(observer) => observer,
        Err(error) => {
            let termination = terminate_cell(&mut child, BTreeSet::from([child_id])).map(|_| ());
            return Err(cell_cleanup_error(error, [termination]));
        }
    };
    let parent = match listening.accept(&mut child, request) {
        Ok(parent) => parent,
        Err(error) => {
            let termination = terminate_cell(&mut child, observer.state().2);
            let retirement = observer.wait_retired();
            let observed = observer.finish();
            return Err(cell_cleanup_error(
                Error::CellProtocol(error),
                [termination.map(|_| ()), retirement, observed.map(|_| ())],
            ));
        }
    };
    let (receiver, closure) = parent.split();
    let receiver = std::thread::spawn(move || receiver.receive());
    let (status, capsule, resources) = wait_cell(&mut child, receiver, closure, observer)?;
    let wall_ns = duration_ns(started.elapsed())?;
    let (request, outcome) = match &capsule {
        cell::Capsule::Setup(capsule) => (&capsule.request, &capsule.outcome),
        cell::Capsule::Retained(capsule) => (&capsule.request, &capsule.outcome),
    };
    let success = matches!(outcome, cell::Outcome::Success { .. });
    if !status.success() || !success {
        let reason = match outcome {
            cell::Outcome::Success { .. } => format!("child exited with {status}"),
            cell::Outcome::Failure { reason } => reason.to_string(),
        };
        let original = Error::Owner {
            command,
            reason: reason.into_boxed_str(),
        };
        return Err(observed_cell_error(original, &capsule));
    }
    let counters = match &capsule {
        cell::Capsule::Setup(capsule) => &capsule.counters,
        cell::Capsule::Retained(capsule) => &capsule.counters,
    };
    require_owner_accounting(counters)
        .and_then(|()| require_runner_accounting(counters, command, true))
        .map_err(|error| observed_cell_error(error, &capsule))?;
    if request.core.project != before {
        return Err(Error::Process {
            reason: "admitted capsule changed its parent-sealed project prestate".into(),
        });
    }
    let cache_after = cell::project::Manifest::read(&cache_root)?;
    let after = cell::project::Identity::read_with_manifest(&project.root, &cache_after)?;
    if before.path != after.path
        || before.device != after.device
        || before.inode != after.inode
        || before.source_digest != after.source_digest
    {
        return Err(Error::Process {
            reason: "the isolated child changed parent-owned fixture inputs".into(),
        });
    }
    Ok(CellLaunch {
        capsule,
        wall_ns,
        resources,
        cache: cell::project::Transition::transform(cache_before, cache_after),
    })
}

#[cfg(feature = "import-baseline")]
fn wait_cell(
    child: &mut Child,
    receiver: std::thread::JoinHandle<
        Result<rawr_import::metrics::cell::Capsule, rawr_import::metrics::cell::ReceptionFailure>,
    >,
    closure: rawr_import::metrics::cell::Closure,
    observer: ProcessObserver,
) -> Result<(ExitStatus, rawr_import::metrics::cell::Capsule, Resources), Error> {
    let deadline = Instant::now() + Duration::from_secs(30 * 60);
    let mut status = None;
    loop {
        let (group_alive, observer_failure, _) = observer.state();
        if let Some(reason) = observer_failure {
            return Err(retire_failed_cell(
                child,
                receiver,
                closure,
                observer,
                Error::Process { reason },
            ));
        }
        if status.is_none() {
            status = match child.try_wait() {
                Ok(status) => status,
                Err(error) => {
                    return Err(retire_failed_cell(
                        child,
                        receiver,
                        closure,
                        observer,
                        Error::Process {
                            reason: error.to_string().into_boxed_str(),
                        },
                    ));
                }
            };
        }
        if status.is_some() && !group_alive && receiver.is_finished() {
            break;
        }
        if Instant::now() >= deadline {
            return Err(retire_failed_cell(
                child,
                receiver,
                closure,
                observer,
                Error::Process {
                    reason: "isolated owner exceeded the fixed thirty-minute deadline".into(),
                },
            ));
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let status = status.ok_or_else(|| Error::Process {
        reason: "isolated child completion lost its exit status".into(),
    })?;
    let capsule = receiver
        .join()
        .map_err(|_| Error::Process {
            reason: "cell capsule receiver panicked".into(),
        })
        .and_then(|result| {
            result.map_err(|failure| {
                let evidence = failure.canonical_capsule.map(|bytes| {
                    String::from_utf8_lossy(&bytes)
                        .into_owned()
                        .into_boxed_str()
                });
                let source = Error::CellProtocol(failure.source);
                match evidence {
                    Some(evidence) => Error::OwnerObserved {
                        source: Box::new(source),
                        evidence,
                    },
                    None => source,
                }
            })
        });
    let resources = observer.finish();
    match capsule {
        Ok(capsule) => {
            let resources = resources.map_err(|error| observed_cell_error(error, &capsule))?;
            Ok((status, capsule, resources))
        }
        Err(source) => Err(cell_cleanup_error(
            source,
            [
                closure.invalidate().map_err(Error::CellProtocol),
                resources.map(|_| ()),
            ],
        )),
    }
}

#[cfg(feature = "import-baseline")]
fn retire_failed_cell(
    child: &mut Child,
    receiver: std::thread::JoinHandle<
        Result<rawr_import::metrics::cell::Capsule, rawr_import::metrics::cell::ReceptionFailure>,
    >,
    closure: rawr_import::metrics::cell::Closure,
    observer: ProcessObserver,
    original: Error,
) -> Error {
    // Invalidation and retirement remain mandatory. Their failures cannot erase
    // either the initiating failure or a capsule already received by the parent.
    let invalidation = closure.invalidate().map_err(Error::CellProtocol);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let (alive, _, _) = observer.state();
        match child.try_wait() {
            Ok(Some(_)) if !alive => break,
            Ok(_) => {}
            Err(_) => break,
        }
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let termination = terminate_cell(child, observer.state().2).map(|_| ());
    let retirement = observer.wait_retired();
    let joined = receiver.join();
    let observed = observer.finish().map(|_| ());
    let original = match joined {
        Ok(Ok(capsule)) => observed_cell_error(original, &capsule),
        Ok(Err(error)) => {
            let evidence = match error.canonical_capsule {
                Some(bytes) => String::from_utf8_lossy(&bytes)
                    .into_owned()
                    .into_boxed_str(),
                None => format!("unavailable: {}", error.source).into_boxed_str(),
            };
            cell_cleanup_error(
                Error::OwnerObserved {
                    source: Box::new(original),
                    evidence,
                },
                [Err(Error::CellProtocol(error.source))],
            )
        }
        Err(_) => Error::OwnerObserved {
            source: Box::new(original),
            evidence: "unavailable: cell capsule receiver panicked during invalidation".into(),
        },
    };
    cell_cleanup_error(original, [invalidation, termination, retirement, observed])
}

#[cfg(feature = "import-baseline")]
fn cell_cleanup_error<const N: usize>(original: Error, cleanup: [Result<(), Error>; N]) -> Error {
    cleanup
        .into_iter()
        .fold(original, |source, cleanup| match cleanup {
            Ok(()) => source,
            Err(cleanup) => Error::CellRetirement {
                source: Box::new(source),
                cleanup: Box::new(cleanup),
            },
        })
}

#[cfg(feature = "import-baseline")]
fn terminate_cell(child: &mut Child, groups: BTreeSet<u32>) -> Result<ExitStatus, Error> {
    use nix::sys::signal::{killpg, Signal};
    use nix::unistd::{getpgid, Pid};

    let status = child.try_wait().map_err(|error| Error::Process {
        reason: error.to_string().into_boxed_str(),
    })?;
    if status.is_some() && groups.is_empty() {
        return status.ok_or_else(|| Error::Process {
            reason: "completed child lost its exit status".into(),
        });
    }
    let pid = Pid::from_raw(i32::try_from(child.id()).map_err(|_| Error::Process {
        reason: "child PID exceeds the process-group API".into(),
    })?);
    if status.is_none() {
        let group = getpgid(Some(pid)).map_err(|error| Error::Process {
            reason: error.to_string().into_boxed_str(),
        })?;
        if group != pid {
            return Err(Error::Process {
                reason: "refusing to terminate a child outside its exact private process group"
                    .into(),
            });
        }
    }
    for group in groups {
        let group = Pid::from_raw(i32::try_from(group).map_err(|_| Error::Process {
            reason: "authenticated process-group ID exceeds the signal API".into(),
        })?);
        match killpg(group, Signal::SIGKILL) {
            Ok(()) | Err(nix::errno::Errno::ESRCH) => {}
            Err(error) => {
                return Err(Error::Process {
                    reason: error.to_string().into_boxed_str(),
                });
            }
        }
    }
    match status {
        Some(status) => Ok(status),
        None => child.wait().map_err(|error| Error::Process {
            reason: error.to_string().into_boxed_str(),
        }),
    }
}

#[cfg(feature = "import-baseline")]
fn sample_process_group(root: u32, usage: &mut GroupUsage) -> Result<bool, Error> {
    usage.resources.sampler_count =
        usage
            .resources
            .sampler_count
            .checked_add(1)
            .ok_or_else(|| Error::Process {
                reason: "process sampler count overflowed".into(),
            })?;
    let output = std::process::Command::new("/bin/ps")
        .args(["-axo", "pid=,ppid=,pgid=,rss=,utime=,stime="])
        .output()
        .map_err(|error| Error::Process {
            reason: error.to_string().into_boxed_str(),
        })?;
    if !output.status.success() || output.stdout.len() > 16 * 1024 * 1024 {
        return Err(Error::Process {
            reason: String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(16 * 1024)
                .collect::<String>()
                .into_boxed_str(),
        });
    }
    let document = std::str::from_utf8(&output.stdout).map_err(|error| Error::Process {
        reason: error.to_string().into_boxed_str(),
    })?;
    observe_process_document(root, usage, document)
}

#[cfg(feature = "import-baseline")]
fn observe_process_document(
    root: u32,
    usage: &mut GroupUsage,
    document: &str,
) -> Result<bool, Error> {
    let mut rows = BTreeMap::new();
    for line in document.lines() {
        let mut fields = line.split_ascii_whitespace();
        let pid = parse_process_field(fields.next(), line)?;
        let parent = parse_process_field(fields.next(), line)?;
        let group = parse_process_field(fields.next(), line)?;
        let resident = parse_process_field(fields.next(), line)?;
        let user_time = fields.next().map(parse_process_time).transpose()?;
        let system_time = fields.next().map(parse_process_time).transpose()?;
        if fields.next().is_some() || user_time.is_none() || system_time.is_none() {
            return Err(Error::Process {
                reason: format!("unexpected process-tree row `{line}`").into_boxed_str(),
            });
        }
        rows.insert(
            pid,
            ProcessRow {
                parent,
                group,
                resident: u64::from(resident).saturating_mul(1024),
                user: user_time.unwrap_or(0),
                system: system_time.unwrap_or(0),
            },
        );
    }
    let mut live = BTreeSet::new();
    loop {
        let mut changed = false;
        for (pid, row) in &rows {
            let ancestry = *pid == root || row.group == root || live.contains(&row.parent);
            let relevant = ancestry || usage.known.contains(pid);
            if relevant && live.insert(*pid) {
                if !ancestry
                    && usage
                        .group_by_pid
                        .get(pid)
                        .is_some_and(|group| *group != row.group)
                {
                    return Err(Error::Process {
                        reason: format!(
                            "authenticated descendant PID {pid} was reused by process group {}",
                            row.group,
                        )
                        .into_boxed_str(),
                    });
                }
                usage.group_by_pid.insert(*pid, row.group);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut rss = 0_u64;
    let mut live_groups = BTreeSet::new();
    for pid in &live {
        let row = rows.get(pid).ok_or_else(|| Error::Process {
            reason: "live process-tree row disappeared during one sealed sample".into(),
        })?;
        usage.known.insert(*pid);
        live_groups.insert(row.group);
        let cpu = usage.cpu.entry(*pid).or_insert((0, 0));
        cpu.0 = cpu.0.max(row.user);
        cpu.1 = cpu.1.max(row.system);
        rss = rss
            .checked_add(row.resident)
            .ok_or_else(|| Error::Process {
                reason: "process-tree RSS overflowed".into(),
            })?;
    }
    let (user, system) = usage
        .cpu
        .values()
        .try_fold(
            (0_u64, 0_u64),
            |(user, system), (process_user, process_system)| {
                Some((
                    user.checked_add(*process_user)?,
                    system.checked_add(*process_system)?,
                ))
            },
        )
        .ok_or_else(|| Error::Process {
            reason: "process-tree CPU overflowed".into(),
        })?;
    usage.resources.user_ns = user;
    usage.resources.system_ns = system;
    usage.resources.peak_rss_bytes = usage.resources.peak_rss_bytes.max(rss);
    usage.live_groups = live_groups;
    Ok(!live.is_empty())
}

#[cfg(feature = "import-baseline")]
struct ProcessRow {
    parent: u32,
    group: u32,
    resident: u64,
    user: u64,
    system: u64,
}

#[cfg(feature = "import-baseline")]
fn parse_process_field(value: Option<&str>, line: &str) -> Result<u32, Error> {
    value
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| Error::Process {
            reason: format!("unexpected process-tree row `{line}`").into_boxed_str(),
        })
}

#[cfg(feature = "import-baseline")]
struct Launch<V = timing::DescriptiveVisibility> {
    capsule: rawr_import::metrics::evidence::capsule::Payload,
    wall_ns: u64,
    resources: Resources,
    visibility: V,
    retirement: rawr_import::metrics::report::timing::Retirement,
    observer: observer::Report,
    cache_before: State,
    cache_after: State,
}

#[cfg(feature = "import-baseline")]
struct TitleFile {
    path: PathBuf,
    identity: rawr_import::metrics::evidence::title::Identity,
}

#[cfg(feature = "import-baseline")]
impl TitleFile {
    fn transform(project: &Project, title: &str, launch_id: [u8; 32]) -> Result<Self, Error> {
        let directory = project.root.join("native");
        let target = directory.join(".phase9-window-title");
        match fs::symlink_metadata(&target) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Ok(_) => {
                return Err(Error::Window {
                    reason: "the fixed runtime title path already exists".into(),
                });
            }
            Err(source) => {
                return Err(Error::Io {
                    path: target,
                    source,
                });
            }
        }
        let temporary = directory.join(format!(
            ".phase9-window-title.{}.tmp",
            rawr_import::metrics::evidence::hex(&launch_id)
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|source| Error::Io {
                path: temporary.clone(),
                source,
            })?;
        let mut linked = false;
        let result = (|| {
            file.write_all(title.as_bytes())
                .map_err(|source| Error::Io {
                    path: temporary.clone(),
                    source,
                })?;
            file.sync_all().map_err(|source| Error::Io {
                path: temporary.clone(),
                source,
            })?;
            fs::hard_link(&temporary, &target).map_err(|source| Error::Io {
                path: target.clone(),
                source,
            })?;
            linked = true;
            fs::remove_file(&temporary).map_err(|source| Error::Io {
                path: temporary.clone(),
                source,
            })?;
            let canonical = fs::canonicalize(&target).map_err(|source| Error::Io {
                path: target.clone(),
                source,
            })?;
            let identity = rawr_import::metrics::evidence::title::Identity::read(&canonical)?;
            if identity.bytes() != title.as_bytes() {
                return Err(Error::Window {
                    reason: "the atomically installed title bytes changed".into(),
                });
            }
            Ok(Self {
                path: canonical,
                identity,
            })
        })();
        if result.is_err() {
            if linked && same_open_file(&file, &target) {
                let _ = fs::remove_file(&target);
            }
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    fn identity(&self) -> &rawr_import::metrics::evidence::title::Identity {
        &self.identity
    }

    fn assert_unchanged(&self) -> Result<(), Error> {
        let current = rawr_import::metrics::evidence::title::Identity::read(&self.path)?;
        if current == self.identity {
            Ok(())
        } else {
            Err(Error::Window {
                reason: "the runtime title file changed during the authenticated launch".into(),
            })
        }
    }

    fn finish(mut self) -> Result<(), Error> {
        self.assert_unchanged()?;
        fs::remove_file(&self.path).map_err(|source| Error::Io {
            path: self.path.clone(),
            source,
        })?;
        self.path = PathBuf::new();
        Ok(())
    }
}

#[cfg(feature = "import-baseline")]
fn same_open_file(left: &fs::File, right: &Path) -> bool {
    let left = left.metadata();
    let right = fs::symlink_metadata(right);
    match (left, right) {
        (Ok(left), Ok(right)) => {
            left.is_file()
                && right.is_file()
                && !right.file_type().is_symlink()
                && left.dev() == right.dev()
                && left.ino() == right.ino()
        }
        _ => false,
    }
}

#[cfg(feature = "import-baseline")]
impl Drop for TitleFile {
    fn drop(&mut self) {
        if !self.path.as_os_str().is_empty()
            && rawr_import::metrics::evidence::title::Identity::read(&self.path)
                .is_ok_and(|current| current == self.identity)
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(feature = "import-baseline")]
#[derive(serde::Serialize)]
struct WarmSeries {
    series: Series,
    observers: Box<[observer::Report]>,
}

#[cfg(feature = "import-baseline")]
impl WarmSeries {
    fn canonical_bytes(&self) -> Result<Box<[u8]>, Error> {
        release::encode(self)
    }
}

#[cfg(feature = "import-baseline")]
fn warm_series(assets: &Assets, project: &Project) -> Result<WarmSeries, Error> {
    #[cfg(not(target_os = "macos"))]
    return Err(Error::Window {
        reason: "the governed AX retirement requires macOS".into(),
    });
    assets.apply(Scenario::IdenticalWarm, project)?;
    let environment = environment()?;
    let executable = release_artifact("evidence")?;
    isolated_cell(
        &executable,
        assets,
        project,
        rawr_import::metrics::cell::corpus_id()?,
        Scenario::IdenticalWarm,
        Command::Run,
        rawr_import::metrics::cell::Role::Setup,
        rawr_import::metrics::cell::Profile::Inactive,
    )?;
    let build = release_build(&executable, &environment)?;
    let fixture = assets.fixture()?;
    let fixture_digest = rawr_import::metrics::evidence::parse_digest(std::ffi::OsStr::new(
        fixture.digest_sha256.as_ref(),
    ))?;
    let mut samples = Vec::with_capacity(SAMPLED_TRIALS.len());
    let mut observers = Vec::with_capacity(SAMPLED_TRIALS.len());
    for trial in SAMPLED_TRIALS {
        let launch = launch(&executable, project, fixture_digest, trial)?;
        observers.push(launch.observer.clone());
        samples.push(launch_sample(launch, trial)?);
    }
    let series = Series::transform(
        lifecycle(Command::Run).into(),
        environment,
        fixture,
        build,
        samples.into_boxed_slice(),
    )
    .map_err(Error::from)?;
    Ok(WarmSeries {
        series,
        observers: observers.into_boxed_slice(),
    })
}

#[cfg(feature = "import-baseline")]
fn launch(
    executable: &Path,
    project: &Project,
    fixture_digest: [u8; 32],
    trial: Trial,
) -> Result<Launch, Error> {
    launch_with::<timing::DescriptiveBaseline>(executable, project, fixture_digest, trial)
}

#[cfg(feature = "import-baseline")]
fn launch_with<A: timing::Authority>(
    executable: &Path,
    project: &Project,
    fixture_digest: [u8; 32],
    trial: Trial,
) -> Result<Launch<A::Visibility>, Error> {
    use rawr_import::metrics::evidence;

    let _build = provenance::load(executable, provenance::Kind::Evidence)?;
    let listener = evidence::Listening::transform()?;
    let launch_id = listener.launch_id();
    let title = format!("Rawr Baseline {}", evidence::hex(&launch_id[..12]));
    let title_file = TitleFile::transform(project, &title, launch_id)?;
    let forwarded = [
        std::ffi::OsString::from("run"),
        project.root.as_os_str().to_owned(),
    ];
    let command_digest = evidence::command_digest(&forwarded);
    let executable_identity = evidence::process::file::Identity::read(executable)?;
    let cache_before = project.cache()?;
    let mut observer = observer::Observer::transform(
        launch_id,
        executable_identity.clone(),
        title_file.identity().clone(),
    )?;
    let t0_ns = observer::protocol::now()
        .map_err(|source| observer_failure(source.into(), None, Custody::Unknown, &mut observer))?;
    let envelope = evidence::Envelope::transform(t0_ns)
        .map_err(|source| observer_failure(source.into(), None, Custody::Unknown, &mut observer))?;
    let timing = A::from_envelope(envelope)
        .map_err(|source| observer_failure(source.into(), None, Custody::Unknown, &mut observer))?;
    if let Err(source) = observer.begin(t0_ns, timing.window()) {
        return Err(observer_failure(
            source.into(),
            None,
            Custody::Unknown,
            &mut observer,
        ));
    }
    let (kind, ordinal) = match trial {
        Trial::Warmup { ordinal } => (evidence::Trial::Warmup, ordinal),
        Trial::Measured { ordinal } => (evidence::Trial::Measured, ordinal),
        Trial::Exact => {
            return Err(observer_failure(
                Error::Launch {
                    reason: "exact trial cannot enter the sampled series".into(),
                },
                None,
                Custody::Unknown,
                &mut observer,
            ))
        }
    };
    let challenge = evidence::Challenge::transform(
        launch_id,
        t0_ns,
        kind,
        ordinal,
        fixture_digest,
        command_digest,
        executable_identity.digest,
        title_file.identity().clone(),
    )
    .map_err(|source| observer_failure(source.into(), None, Custody::Unknown, &mut observer))?;
    let (child_stderr, stderr_capture) = stderr::Capture::transform(envelope)
        .map_err(|source| observer_failure(source.into(), None, Custody::Unknown, &mut observer))?;
    let mut command = std::process::Command::new(executable);
    command
        .arg("--baseline-evidence-v1")
        .arg(listener.socket())
        .arg(evidence::hex(&launch_id))
        .args(&forwarded)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .process_group(0);
    let mut child = match spawn_with_captured_stderr(command, child_stderr) {
        Ok(child) => child,
        Err(error) => {
            let source = observer_failure(
                Error::Launch {
                    reason: error.to_string().into_boxed_str(),
                },
                None,
                Custody::Unknown,
                &mut observer,
            );
            stderr_capture.retire();
            return Err(source);
        }
    };
    let mut runner_custody = Custody::Unknown;
    let mut stage = stderr::Stage::EvidenceAccept;
    let result = (|| {
        let mut parent =
            listener.accept_before(&mut child, challenge, timing.window().deadline_ns())?;
        stage = stderr::Stage::RunnerReceipt;
        let runner = parent
            .runner_owned(&mut child)
            .map_err(|source| {
                let observed = source
                    .startup_failure()
                    .and_then(|failure| failure.canonical_bytes().ok());
                match observed {
                    Some(bytes) => Error::OwnerObserved {
                        source: Box::new(source.into()),
                        evidence: String::from_utf8_lossy(&bytes)
                            .into_owned()
                            .into_boxed_str(),
                    },
                    None => source.into(),
                }
            })?
            .clone();
        runner_custody = Custody::Runner(runner.runner.pid);
        stage = stderr::Stage::Observation;
        observer.bind(&runner)?;
        let observed = observe_launch(&mut child, &mut observer, &timing)?;
        stage = stderr::Stage::Retirement;
        title_file.assert_unchanged()?;
        let (capsule, status, pressed_ns, observer_report) =
            retire_launch(&mut child, parent, &mut observer)?;
        // Successful retirement relinquishes these PIDs before later evidence checks.
        runner_custody = Custody::Relinquished(runner.runner.pid);
        if !status.success()
            || capsule.product != evidence::capsule::Product::Success
            || capsule.runner != runner
        {
            return Err(observed_launch_error(
                Error::Launch {
                    reason: format!("ordinary run result disagrees: {status}").into_boxed_str(),
                },
                &capsule,
            ));
        }
        require_owner_accounting(&capsule.counters)
            .and_then(|()| require_runner_accounting(&capsule.counters, Command::Run, false))
            .map_err(|source| observed_launch_error(source, &capsule))?;
        if capsule.title != *title_file.identity() || capsule.title.bytes() != title.as_bytes() {
            return Err(Error::Window {
                reason: "the final capsule title differs from the installed title file".into(),
            });
        }
        let retirement = rawr_import::metrics::report::timing::Retirement::completed(
            pressed_ns,
            observer::protocol::now()?,
        )?;
        Ok((
            capsule,
            status,
            observed,
            retirement,
            observer_report,
            pressed_ns,
        ))
    })();
    let (capsule, status, observed, _retirement, observer_report, pressed_ns) = match result {
        Ok(retired) => retired,
        Err(source) => {
            let source = observer_failure(source, Some(&mut child), runner_custody, &mut observer);
            return Err(staged_launch_failure(
                stage,
                source,
                &mut child,
                stderr_capture,
            ));
        }
    };
    let retirement_deadline = pressed_ns
        .checked_add(observer::protocol::GRACE_NS)
        .ok_or_else(|| Error::Launch {
            reason: "post-press retirement deadline overflowed".into(),
        })?;
    if let Err(failure) = stderr_capture.finish_before(&status, retirement_deadline) {
        return Err(staged_stderr_failure(stderr::Stage::Retirement, failure));
    }
    // CLI stderr is another actual reader in the original post-press grace.
    let retirement = rawr_import::metrics::report::timing::Retirement::completed(
        pressed_ns,
        observer::protocol::now()?,
    )?;
    title_file.finish()?;
    let wall_ns = observer::protocol::now()?
        .checked_sub(t0_ns)
        .ok_or_else(|| Error::Launch {
            reason: "launch wall clock moved before t0".into(),
        })?;
    let cache_after = project.cache()?;
    Ok(Launch {
        capsule,
        wall_ns,
        resources: observed.resources,
        visibility: observed.visibility,
        retirement,
        observer: observer_report,
        cache_before,
        cache_after,
    })
}

#[cfg(feature = "import-baseline")]
#[derive(Clone, Copy, Debug)]
enum Custody {
    Unknown,
    Runner(u32),
    Relinquished(u32),
}

#[cfg(feature = "import-baseline")]
impl Custody {
    fn description(self) -> String {
        match self {
            Self::Unknown => "runner custody unavailable; no signal authority".into(),
            Self::Runner(pid) => format!("runner {pid} recorded in receipt; no signal authority after possible runner reap, even while CLI remains live"),
            Self::Relinquished(pid) => {
                format!("runner {pid} relinquished; no signal authority for stored identity")
            }
        }
    }
}

/// Cancel still-owned direct children, then retain incomplete descendant cleanup.
#[cfg(feature = "import-baseline")]
fn observer_failure(
    source: Error,
    mut child: Option<&mut Child>,
    runner: Custody,
    observer: &mut observer::Observer,
) -> Error {
    use nix::sys::signal::{killpg, Signal};
    use nix::unistd::Pid;
    // A cached or newly observed exit relinquishes PID-based signal authority.
    // An inspection failure is uncertainty, not permission to signal stale IDs.
    let mut status = None;
    let mut wait_error = None;
    let live_cli = match child.as_mut().map(|child| child.try_wait()) {
        Some(Ok(Some(observed))) => {
            status = Some(observed);
            false
        }
        Some(Ok(None)) => true,
        Some(Err(error)) => {
            wait_error = Some(error);
            false
        }
        None => false,
    };
    let observer_signal = observer.cancel();
    let runner_custody = runner.description();
    // The CLI can reap its runner before joining logs or returning. Neither a
    // receipt PID nor live CLI custody protects that grandchild's lifetime.
    let runner_signal =
        "not sent: parent lacks current runner lifetime authority; workload cleanup incomplete";
    let cli_signal = child.as_mut().filter(|_| live_cli).map(|child| {
        let group = i32::try_from(child.id())
            .map(Pid::from_raw)
            .map(|pid| killpg(pid, Signal::SIGKILL));
        (group, child.kill())
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let observer_done = observer.cleanup_poll();
        if status.is_none()
            && let Some(child) = child.as_mut()
        {
            match child.try_wait() {
                Ok(observed) => status = observed,
                Err(error) => {
                    wait_error = Some(error);
                    break;
                }
            }
        }
        if observer_done && (child.is_none() || status.is_some()) {
            break;
        }
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(observer::protocol::POLL);
    }
    Error::OwnerObserved {
        source: Box::new(source),
        evidence: format!("failure cleanup: {runner_custody}; observer_signal={observer_signal}; runner_signal={runner_signal:?}; cli_signal={cli_signal:?}; cli_status={status:?}; cli_wait_error={wait_error:?}; {}; unknown descendant/ps retirement is incomplete, never normal retirement", observer.cleanup_facts()).into_boxed_str(),
    }
}

#[cfg(feature = "import-baseline")]
fn spawn_with_captured_stderr(
    mut command: std::process::Command,
    child_stderr: Stdio,
) -> std::io::Result<Child> {
    command.stderr(child_stderr).spawn()
}

#[cfg(feature = "import-baseline")]
fn observed_launch_error(
    source: Error,
    capsule: &rawr_import::metrics::evidence::capsule::Payload,
) -> Error {
    let evidence = match capsule.canonical_bytes() {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(error) => format!("unavailable: {error}"),
    };
    Error::OwnerObserved {
        source: Box::new(source),
        evidence: evidence.into_boxed_str(),
    }
}

#[cfg(feature = "import-baseline")]
fn staged_launch_failure(
    stage: stderr::Stage,
    source: Error,
    child: &mut Child,
    capture: stderr::Capture,
) -> Error {
    Error::StagedLaunch {
        stage,
        source: Box::new(source),
        context: capture.failure_context(child),
    }
}

#[cfg(feature = "import-baseline")]
fn staged_stderr_failure(stage: stderr::Stage, failure: stderr::Failure) -> Error {
    let (source, context) = failure.into_parts();
    Error::StagedLaunch {
        stage,
        source: Box::new(Error::Stderr(source)),
        context,
    }
}

#[cfg(feature = "import-baseline")]
struct Observed<V> {
    resources: Resources,
    visibility: V,
}

#[cfg(feature = "import-baseline")]
fn observe_launch<A: timing::Authority>(
    child: &mut Child,
    observer: &mut observer::Observer,
    timing: &A,
) -> Result<Observed<A::Visibility>, Error> {
    loop {
        if let Some(visible) = observer.poll_visible()? {
            return Ok(Observed {
                resources: Resources {
                    user_ns: visible.resources.user_ns,
                    system_ns: visible.resources.system_ns,
                    peak_rss_bytes: visible.resources.peak_rss_bytes,
                    sampler_count: visible.resources.samples,
                },
                visibility: timing.observe(visible.observed_ns)?,
            });
        }
        if child
            .try_wait()
            .map_err(|error| Error::Launch {
                reason: error.to_string().into_boxed_str(),
            })?
            .is_some()
        {
            return Err(Error::Launch {
                reason: "the CLI exited before the exact window became visible".into(),
            });
        }
        std::thread::sleep(observer::protocol::POLL);
    }
}

#[cfg(feature = "import-baseline")]
fn retire_launch(
    child: &mut Child,
    parent: rawr_import::metrics::evidence::Parent,
    observer: &mut observer::Observer,
) -> Result<
    (
        rawr_import::metrics::evidence::capsule::Payload,
        ExitStatus,
        u64,
        observer::Report,
    ),
    Error,
> {
    let mut receiver = parent.into_finishing()?;
    observer.close()?;
    let mut status = None;
    let mut capsule = None;
    let mut finishing = false;
    loop {
        let (pressed_ns, disappeared) = observer.retirement()?;
        if status.is_none() {
            status = child.try_wait().map_err(|error| Error::Launch {
                reason: error.to_string().into_boxed_str(),
            })?;
        }
        if capsule.is_none() {
            capsule = receiver.poll()?;
        }
        if disappeared && status.is_some() && capsule.is_some() && !finishing {
            observer.finish()?;
            finishing = true;
        }
        if finishing && let Some(report) = observer.completed()? {
            return Ok((
                capsule.ok_or_else(|| Error::Launch {
                    reason: "final capsule absent".into(),
                })?,
                status.ok_or_else(|| Error::Launch {
                    reason: "CLI reap absent".into(),
                })?,
                pressed_ns.ok_or_else(|| Error::Launch {
                    reason: "AX press timestamp absent".into(),
                })?,
                report,
            ));
        }
        std::thread::sleep(observer::protocol::POLL);
    }
}

#[cfg(feature = "import-baseline")]
fn parse_process_time(value: &str) -> Result<u64, Error> {
    let mut fields = value.rsplit(':');
    let seconds = fields.next().ok_or_else(|| Error::Process {
        reason: "process time omitted seconds".into(),
    })?;
    let minutes = fields.next().unwrap_or("0");
    let hours = fields.next().unwrap_or("0");
    if fields.next().is_some() {
        return Err(Error::Process {
            reason: "process time has too many fields".into(),
        });
    }
    let (whole, fraction) = seconds.split_once('.').unwrap_or((seconds, ""));
    if fraction.len() > 9 || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Error::Process {
            reason: "process time fraction is invalid".into(),
        });
    }
    let whole = whole.parse::<u64>().map_err(|error| Error::Process {
        reason: error.to_string().into_boxed_str(),
    })?;
    let minutes = minutes.parse::<u64>().map_err(|error| Error::Process {
        reason: error.to_string().into_boxed_str(),
    })?;
    let hours = hours.parse::<u64>().map_err(|error| Error::Process {
        reason: error.to_string().into_boxed_str(),
    })?;
    let base = hours
        .checked_mul(3_600)
        .and_then(|value| value.checked_add(minutes.saturating_mul(60)))
        .and_then(|value| value.checked_add(whole))
        .and_then(|value| value.checked_mul(1_000_000_000))
        .ok_or_else(|| Error::Process {
            reason: "process time overflowed".into(),
        })?;
    let fraction = if fraction.is_empty() {
        0
    } else {
        fraction
            .parse::<u64>()
            .ok()
            .and_then(|value| value.checked_mul(10_u64.pow((9 - fraction.len()) as u32)))
            .ok_or_else(|| Error::Process {
                reason: "process time fraction overflowed".into(),
            })?
    };
    base.checked_add(fraction).ok_or_else(|| Error::Process {
        reason: "process time overflowed".into(),
    })
}

#[cfg(feature = "import-baseline")]
fn launch_sample(launch: Launch, trial: Trial) -> Result<Sample, Error> {
    sampled_launch(launch, trial).map(|(sample, _visibility)| sample)
}

#[cfg(feature = "import-baseline")]
fn sampled_launch<V: timing::Visibility>(
    launch: Launch<V>,
    trial: Trial,
) -> Result<(Sample, V), Error> {
    // Reporting consumes the original mode's proof without converting timing authority.
    let identities = oracle::identities(
        &launch.capsule.counters,
        launch.capsule.runner.generation_key,
    )
    .map_err(|error| observed_launch_error(error.into(), &launch.capsule))?;
    let receipt = &launch.capsule.runner;
    if !launch
        .capsule
        .counters
        .runner
        .as_ref()
        .is_some_and(|runner| {
            runner.runner_pid == receipt.runner.pid
                && runner.runner_uid == receipt.runner.uid
                && runner.build_request == receipt.build_request
                && runner.generated_manifest == receipt.generated_manifest
                && runner.generation_key == receipt.generation_key
                && runner.epoch == receipt.epoch
                && runner.artifact == receipt.artifact
                && runner.executable_digest == receipt.executable_digest
                && runner.executable_bytes == receipt.executable_bytes
                && runner.spawned_ns == Some(receipt.spawned_ns)
        })
    {
        return Err(observed_launch_error(
            oracle::Error::Identity {
                field: "authenticated runner receipt",
            }
            .into(),
            &launch.capsule,
        ));
    }
    let counters = launch.capsule.counters;
    let admitted_ns = absolute_milestone(&counters, rawr_import::metrics::Milestone::Admitted)?;
    let started_ns = absolute_milestone(&counters, rawr_import::metrics::Milestone::Started)?;
    if started_ns > launch.visibility.latency_ns() {
        return Err(Error::Window {
            reason: "external visibility preceded the authenticated Started milestone".into(),
        });
    }
    let sample = Sample {
        trial,
        wall_ns: launch.wall_ns,
        cpu_user_ns: Observation::Measured {
            value: launch.resources.user_ns,
        },
        cpu_system_ns: Observation::Measured {
            value: launch.resources.system_ns,
        },
        peak_rss_bytes: Observation::Measured {
            value: launch.resources.peak_rss_bytes,
        },
        admitted_ns: Observation::Measured { value: admitted_ns },
        started_ns: Observation::Measured { value: started_ns },
        timing: launch.visibility.report()?,
        retirement: launch.retirement,
        updates_per_second_milli: not_applicable(
            "the distinct warm-run series is not the native-iced/Rawr update control",
        ),
        responsiveness_ns: not_applicable(
            "the distinct warm-run series is not the Game-of-Life responsiveness control",
        ),
        rollback: not_applicable("the identical-warm series does not induce rollback"),
        last_good_ns: not_applicable("the identical-warm series does not induce recovery"),
        counters,
        cache: Cache {
            preparation: "unchanged warmed fixture and project-local owner cache".into(),
            state: format!(
                "before={} after={}",
                launch.cache_before.digest, launch.cache_after.digest
            )
            .into_boxed_str(),
            bytes_before: launch.cache_before.bytes,
            bytes_after: launch.cache_after.bytes,
            durable_mutations: if launch.cache_before.digest == launch.cache_after.digest {
                Box::default()
            } else {
                vec![format!(
                    "project-cache:{}->{}",
                    launch.cache_before.digest, launch.cache_after.digest
                )
                .into_boxed_str()]
                .into_boxed_slice()
            },
        },
        identities,
    };
    Ok((sample, launch.visibility))
}

#[cfg(feature = "import-baseline")]
fn absolute_milestone(
    counters: &Snapshot,
    target: rawr_import::metrics::Milestone,
) -> Result<u64, Error> {
    let row = counters
        .milestones
        .iter()
        .find(|row| row.milestone == target)
        .ok_or_else(|| Error::Launch {
            reason: format!("capsule omitted {target:?}").into_boxed_str(),
        })?;
    if row.count != 1 || row.last_observed_ns == 0 {
        return Err(Error::Launch {
            reason: format!("capsule has {} {target:?} milestones", row.count).into_boxed_str(),
        });
    }
    Ok(row.last_elapsed_ns)
}

#[cfg(feature = "import-baseline")]
fn release_artifact(kind: &str) -> Result<PathBuf, Error> {
    let path = provenance::artifact(kind)?;
    validate_release_artifact(&path)
}

#[cfg(feature = "import-baseline")]
fn validate_release_artifact(path: &Path) -> Result<PathBuf, Error> {
    let metadata = fs::symlink_metadata(path).map_err(|error| Error::Release {
        path: path.to_path_buf(),
        reason: error.to_string().into_boxed_str(),
    })?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(Error::Release {
            path: path.to_path_buf(),
            reason: "release CLI is not an executable regular file".into(),
        });
    }
    let executable = fs::canonicalize(path).map_err(|error| Error::Release {
        path: path.to_path_buf(),
        reason: error.to_string().into_boxed_str(),
    })?;
    let directory = executable.parent().ok_or_else(|| Error::Release {
        path: executable.clone(),
        reason: "release executable has no containing directory".into(),
    })?;
    let driver = directory.join(format!(
        "rawr-import-driver{}",
        std::env::consts::EXE_SUFFIX
    ));
    let driver = fs::canonicalize(&driver).map_err(|error| Error::Release {
        path: driver,
        reason: error.to_string().into_boxed_str(),
    })?;
    let metadata = fs::symlink_metadata(&driver).map_err(|error| Error::Release {
        path: driver.clone(),
        reason: error.to_string().into_boxed_str(),
    })?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(Error::Release {
            path: driver,
            reason: "release import driver is not an executable regular file".into(),
        });
    }
    Ok(executable)
}

#[cfg(feature = "import-baseline")]
fn release_build(executable: &Path, _environment: &Environment) -> Result<Build, Error> {
    provenance::load(executable, provenance::Kind::Evidence)?
        .report()
        .map_err(Error::from)
}

fn environment() -> Result<Environment, Error> {
    let rustc = output("rustup", &["run", "nightly-2026-07-11", "rustc", "-Vv"])?;
    let target = rustc
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .filter(|host| !host.is_empty())
        .ok_or_else(|| Error::Environment {
            program: "rustup",
            reason: "pinned rustc -Vv omitted its host triple".into(),
        })?
        .into();
    Ok(Environment {
        machine: output("hostname", &[])?,
        operating_system: format!("{} {}", std::env::consts::OS, output("uname", &["-r"])?)
            .into_boxed_str(),
        architecture: std::env::consts::ARCH.into(),
        cpu: output("sysctl", &["-n", "machdep.cpu.brand_string"])
            .unwrap_or_else(|_| "unavailable on this host".into()),
        rustc,
        rustdoc: output("rustup", &["run", "nightly-2026-07-11", "rustdoc", "-Vv"])?,
        cargo: output("rustup", &["run", "nightly-2026-07-11", "cargo", "-Vv"])?,
        driver: concat!("rawr-iced-cli ", env!("CARGO_PKG_VERSION")).into(),
        target,
        profile: "test owner / generated runner release profile".into(),
    })
}

fn output(program: &'static str, arguments: &[&str]) -> Result<Box<str>, Error> {
    let output = std::process::Command::new(program)
        .args(arguments)
        .output()
        .map_err(|error| Error::Environment {
            program,
            reason: error.to_string().into_boxed_str(),
        })?;
    if !output.status.success() {
        return Err(Error::Environment {
            program,
            reason: String::from_utf8_lossy(&output.stderr)
                .into_owned()
                .into_boxed_str(),
        });
    }
    if output.stdout.len() > OUTPUT_BYTES {
        return Err(Error::Bound {
            field: "environment output bytes",
            limit: OUTPUT_BYTES as u64,
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_owned()
        .into_boxed_str())
}

fn milestone(counters: &Snapshot, target: rawr_import::metrics::Milestone) -> Observation<u64> {
    match counters
        .milestones
        .iter()
        .find(|row| row.milestone == target)
    {
        Some(row) if row.count == 1 => Observation::Measured {
            value: row.last_elapsed_ns,
        },
        Some(row) => Observation::Unavailable {
            reason: format!("expected one {target:?} milestone, observed {}", row.count)
                .into_boxed_str(),
        },
        None => Observation::Unavailable {
            reason: format!("the fixed recorder omitted {target:?}").into_boxed_str(),
        },
    }
}

fn preparation(scenario: Scenario) -> &'static str {
    match scenario {
        Scenario::CleanCold => "empty project-local cache and base fixture",
        Scenario::IdenticalWarm => "unchanged base fixture after matching cold owner",
        Scenario::RawrOccurrence => "warm base cache then occurrence-only Rawr mutation",
        Scenario::RustBodyEqualApi => "warm base cache then equal-API Rust body mutation",
        Scenario::RustPublicApi => "warm base cache then Rust public-API mutation",
        Scenario::ProfileRule => {
            "warm matching private baseline profile then select the semantic profile variant"
        }
        Scenario::DemandOnly => "warm base cache then demanded imported-item mutation",
        Scenario::CorruptRunner => "one exact runner artifact byte changed under its owned bundle lease after isolated cold setup",
        Scenario::CorruptResolution => "one exact resolution record byte changed under its owned bundle lease after isolated cold setup",
        Scenario::ConcurrentCold => {
            "not applicable before DAG cutover"
        }
    }
}

fn lifecycle(command: Command) -> &'static str {
    match command {
        Command::Check => "production imported check owner: prepare, admit, shutdown",
        Command::Run => "production imported run owner: prepare, start, shutdown",
        Command::Preview => "production imported preview owner: prepare, start, shutdown",
        Command::Snapshot => "production imported snapshot-update owner: prepare, render, shutdown",
        Command::Watch => {
            "production imported run/watch owner: prepare, start, construct exact-input watcher, shutdown"
        }
    }
}

fn duration_ns(duration: std::time::Duration) -> Result<u64, Error> {
    u64::try_from(duration.as_nanos()).map_err(|_| Error::Bound {
        field: "wall-clock nanoseconds",
        limit: u64::MAX,
    })
}

fn not_applicable<T>(reason: &'static str) -> Observation<T> {
    Observation::NotApplicable {
        reason: reason.into(),
    }
}

fn json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, Error> {
    let bytes = read(path)?;
    serde_json::from_slice(&bytes).map_err(|source| Error::Json {
        path: path.to_path_buf(),
        source,
    })
}

fn read(path: &Path) -> Result<Box<[u8]>, Error> {
    fs::read(path)
        .map(Vec::into_boxed_slice)
        .map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })
}

fn copy(source: &Path, destination: &Path) -> Result<(), Error> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|source| Error::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    fs::copy(source, destination)
        .map(|_| ())
        .map_err(|error| Error::Io {
            path: destination.to_path_buf(),
            source: error,
        })
}

fn canonical(path: &Path) -> Result<PathBuf, Error> {
    fs::canonicalize(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn digest_assets(root: &Path) -> Result<Box<str>, Error> {
    let mut files = BTreeMap::new();
    collect(root, root, &mut files)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    for (relative, member) in files {
        digest.update((relative.len() as u64).to_le_bytes());
        digest.update(&relative);
        digest.update(member.bytes.to_le_bytes());
        member.hash(&mut digest, &mut buffer)?;
    }
    Ok(hex(&digest.finalize()))
}

#[cfg(feature = "import-baseline")]
fn tree_state(root: &Path) -> Result<State, Error> {
    let state = rawr_import::metrics::cell::project::State::read(root)?;
    Ok(State {
        bytes: state.bytes,
        digest: hex(&state.digest),
    })
}

fn collect(
    root: &Path,
    directory: &Path,
    files: &mut BTreeMap<Box<[u8]>, TreeMember>,
) -> Result<(), Error> {
    let mut entries = fs::read_dir(directory)
        .map_err(|source| Error::Io {
            path: directory.to_path_buf(),
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| Error::Io {
            path: directory.to_path_buf(),
            source,
        })?;
    entries.sort_unstable_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|source| Error::Io {
            path: path.clone(),
            source,
        })?;
        if metadata.file_type().is_symlink() {
            return Err(Error::Symlink { path });
        }
        if metadata.is_dir() {
            collect(root, &path, files)?;
            continue;
        }
        if !metadata.is_file() || path.file_name().is_some_and(|name| name == "manifest.json") {
            continue;
        }
        if files.len() >= ASSET_LIMIT {
            return Err(Error::Bound {
                field: "fixture members",
                limit: ASSET_LIMIT as u64,
            });
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| Error::Scenarios)?
            .as_os_str()
            .as_bytes()
            .to_vec()
            .into_boxed_slice();
        if relative.is_empty() {
            return Err(Error::Scenarios);
        }
        if metadata.len() > ASSET_BYTES as u64 {
            return Err(Error::Bound {
                field: "fixture file bytes",
                limit: ASSET_BYTES as u64,
            });
        }
        let member = TreeMember {
            path,
            device: metadata.dev(),
            inode: metadata.ino(),
            bytes: metadata.len(),
        };
        if files.insert(relative, member).is_some() {
            return Err(Error::Scenarios);
        }
    }
    Ok(())
}

struct TreeMember {
    path: PathBuf,
    device: u64,
    inode: u64,
    bytes: u64,
}

impl TreeMember {
    fn hash(&self, digest: &mut Sha256, buffer: &mut [u8]) -> Result<(), Error> {
        let mut file = fs::File::open(&self.path).map_err(|source| Error::Io {
            path: self.path.clone(),
            source,
        })?;
        self.validate(&file.metadata().map_err(|source| Error::Io {
            path: self.path.clone(),
            source,
        })?)?;
        let mut observed = 0_u64;
        loop {
            let read = file.read(buffer).map_err(|source| Error::Io {
                path: self.path.clone(),
                source,
            })?;
            if read == 0 {
                break;
            }
            observed = observed.checked_add(read as u64).ok_or(Error::Bound {
                field: "tree member bytes",
                limit: self.bytes,
            })?;
            if observed > self.bytes {
                return Err(Error::Scenarios);
            }
            digest.update(&buffer[..read]);
        }
        if observed != self.bytes {
            return Err(Error::Scenarios);
        }
        self.validate(&file.metadata().map_err(|source| Error::Io {
            path: self.path.clone(),
            source,
        })?)?;
        self.validate(
            &fs::symlink_metadata(&self.path).map_err(|source| Error::Io {
                path: self.path.clone(),
                source,
            })?,
        )
    }

    fn validate(&self, metadata: &fs::Metadata) -> Result<(), Error> {
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.dev() != self.device
            || metadata.ino() != self.inode
            || metadata.len() != self.bytes
        {
            return Err(Error::Scenarios);
        }
        Ok(())
    }
}

fn hex(bytes: &[u8]) -> Box<str> {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(text, "{byte:02x}");
    }
    text.into_boxed_str()
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "import-baseline")]
    #[test]
    fn every_receipt_custody_state_reports_absent_runner_signal_authority() {
        for custody in [
            super::Custody::Unknown,
            super::Custody::Runner(42),
            super::Custody::Relinquished(42),
        ] {
            assert!(custody.description().contains("no signal authority"));
        }
        assert!(super::Custody::Runner(42)
            .description()
            .contains("even while CLI remains live"));
    }

    #[cfg(feature = "import-baseline")]
    #[test]
    fn cell_cleanup_preserves_the_initiating_failure_and_secondary_diagnostics() {
        let error = super::cell_cleanup_error(
            super::Error::Owner {
                command: super::Command::Run,
                reason: "first owner refusal".into(),
            },
            [
                Ok(()),
                Err(super::Error::Process {
                    reason: "retirement refusal".into(),
                }),
            ],
        );
        let display = error.to_string();
        assert!(display.starts_with("baseline owner Run failed: first owner refusal"));
        assert!(display.contains("retirement refusal"));
        let super::Error::CellRetirement { source, .. } = error else {
            panic!("the secondary failure remains separately attributed");
        };
        assert!(matches!(*source, super::Error::Owner { .. }));
    }

    #[cfg(feature = "import-baseline")]
    #[test]
    fn runner_accounting_requires_the_actual_owner_and_command_boundaries() {
        use rawr_import::{
            metrics::{self, Sink},
            protocol::{account, Tag},
        };
        let recorder = metrics::recorder::Recorder::transform_at(100, 1).unwrap();
        recorder.runner_owned(&metrics::runner::Owned {
            runner_pid: 42,
            runner_uid: 501,
            build_request: [1; 32],
            generated_manifest: [2; 32],
            generation_key: [3; 32],
            epoch: 9,
            artifact: [4; 32],
            executable_path: Box::from(&b"/fixture/runner"[..]),
            executable_digest: [23; 32],
            executable_bytes: 42,
            spawned_ns: Some(1),
        });
        let check = || {
            super::require_runner_accounting(
                &recorder.snapshot().unwrap(),
                super::Command::Run,
                true,
            )
        };
        assert!(check().is_err());
        for tag in [Tag::Prepare, Tag::Admit, Tag::Start, Tag::Shutdown] {
            recorder.native(
                43,
                9,
                account::Observation {
                    request: account::Request::from_parts(tag, &[5; 56]).unwrap(),
                    accounts: [None; 2],
                },
            );
        }
        assert!(
            check().is_err(),
            "another runner's reports cannot satisfy this owner"
        );
        for tag in [Tag::Prepare, Tag::Admit, Tag::Start] {
            recorder.native(
                42,
                9,
                account::Observation {
                    request: account::Request::from_parts(tag, &[5; 56]).unwrap(),
                    accounts: [None; 2],
                },
            );
        }
        assert!(check().is_err(), "explicit retirement remains unreported");
        assert!(
            super::require_runner_accounting(
                &recorder.snapshot().unwrap(),
                super::Command::Run,
                false,
            )
            .is_ok(),
            "window-close runs need not send an explicit shutdown command"
        );
        recorder.native(
            42,
            9,
            account::Observation {
                request: account::Request::from_parts(Tag::Shutdown, &[5; 56]).unwrap(),
                accounts: [None; 2],
            },
        );
        assert!(check().is_ok());
        recorder.native_fault(42, 9, metrics::accounting::NativeFault::Duplicate);
        assert!(check().is_err());
    }

    #[cfg(feature = "import-baseline")]
    #[test]
    fn preparation_accounting_requires_only_outputs_actually_published() {
        use rawr_import::metrics::{
            self,
            accounting::{Graph, Scalar, Stage},
            Sink,
        };
        let recorder = metrics::recorder::Recorder::transform_at(100, 1).unwrap();
        assert!(super::require_owner_accounting(&recorder.snapshot().unwrap()).is_err());
        recorder.graph(Graph {
            account: 1,
            stage: Stage::Completion,
            admitted: [0; 6],
            exceedance: None,
        });
        for key in [
            Scalar::CargoRequestItems,
            Scalar::CargoRequestBytes,
            Scalar::OwnerInputItems,
            Scalar::OwnerInputBytes,
            Scalar::CargoCurrentItems,
            Scalar::CargoCurrentBytes,
            Scalar::CargoCurrentStrings,
            Scalar::CargoCurrentContainers,
            Scalar::CargoPeakItems,
            Scalar::CargoPeakBytes,
            Scalar::CargoPeakStrings,
            Scalar::CargoPeakContainers,
            Scalar::LedgerReadBytes,
            Scalar::LedgerEncodedBytes,
            Scalar::ProjectLedgerReadBytes,
            Scalar::ProjectLedgerEncodedBytes,
            Scalar::PinRolesRequested,
            Scalar::PinRolesCommitted,
            Scalar::BundleMeasuredBytes,
            Scalar::BundleMeasuredMembers,
            Scalar::RecoveryMetadataBytes,
            Scalar::RecoveryDeletionBytes,
            Scalar::RecoveryDeletionMembers,
            Scalar::RecoveryBundles,
            Scalar::RecoveryTemporaries,
            Scalar::NativeInventoryItems,
            Scalar::NativeInventoryBytes,
        ] {
            recorder.scalar(key, 1);
        }
        assert!(super::require_owner_accounting(&recorder.snapshot().unwrap()).is_ok());
        recorder.node(metrics::NodeKind::Api, metrics::NodeEvent::Published);
        assert!(super::require_owner_accounting(&recorder.snapshot().unwrap()).is_err());
        for key in [
            Scalar::NodePayloadBytes(metrics::NodeKind::Api),
            Scalar::ApiCanonicalBytes,
            Scalar::ApiSelectedBytes,
            Scalar::ApiTransactionItems,
            Scalar::ApiTransactionBytes,
            Scalar::ApiRetainedItems,
            Scalar::ApiRetainedBytes,
            Scalar::ControlInventoryBytes,
            Scalar::ControlInventoryMembers,
        ] {
            recorder.scalar(key, 1);
        }
        assert!(super::require_owner_accounting(&recorder.snapshot().unwrap()).is_ok());
        recorder.scalar_overflow(Scalar::ApiCanonicalBytes);
        assert!(super::require_owner_accounting(&recorder.snapshot().unwrap()).is_err());
    }

    #[cfg(feature = "import-baseline")]
    use std::collections::BTreeSet;
    #[cfg(feature = "import-baseline")]
    use std::fs;
    #[cfg(feature = "import-baseline")]
    use std::path::PathBuf;

    #[cfg(feature = "import-baseline")]
    use super::{
        spawn_with_captured_stderr, staged_launch_failure, staged_stderr_failure, stderr, Error,
    };
    use super::{Assets, Outcome, Plan};

    #[cfg(feature = "import-baseline")]
    fn diagnostic_child(script: &str) -> (std::process::Child, stderr::Capture) {
        let origin_ns = rawr_import::metrics::monotonic_ns().expect("monotonic clock is available");
        let envelope = rawr_import::metrics::evidence::Envelope::transform(origin_ns)
            .expect("absolute evidence envelope seals");
        let (child_stderr, capture) =
            stderr::Capture::transform(envelope).expect("bounded stderr capture transforms");
        let mut command = std::process::Command::new("/bin/sh");
        command
            .args(["-c", script])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null());
        let child = spawn_with_captured_stderr(command, child_stderr)
            .expect("diagnostic child spawns through the consuming transformation");
        (child, capture)
    }

    #[cfg(feature = "import-baseline")]
    fn exited_diagnostic_child(
        script: &str,
    ) -> (
        std::process::Child,
        stderr::Capture,
        std::process::ExitStatus,
    ) {
        let (mut child, capture) = diagnostic_child(script);
        let status = child.wait().expect("diagnostic child retires");
        (child, capture, status)
    }

    #[cfg(feature = "import-baseline")]
    fn receipt_eof() -> Error {
        Error::Evidence(rawr_import::metrics::evidence::Error::Io(
            std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "runner receipt closed"),
        ))
    }

    #[cfg(feature = "import-baseline")]
    #[test]
    fn pre_receipt_exit_preserves_typed_eof_and_bounded_stderr() {
        let (mut child, capture, status) =
            exited_diagnostic_child("printf '%s' 'pre-receipt diagnostic' >&2; exit 23");
        assert_eq!(status.code(), Some(23));
        let failure = staged_launch_failure(
            stderr::Stage::RunnerReceipt,
            receipt_eof(),
            &mut child,
            capture,
        );
        match &failure {
            Error::StagedLaunch { stage, source, .. } => {
                assert_eq!(*stage, stderr::Stage::RunnerReceipt);
                match source.as_ref() {
                    Error::Evidence(rawr_import::metrics::evidence::Error::Io(error)) => {
                        assert_eq!(error.kind(), std::io::ErrorKind::UnexpectedEof);
                    }
                    other => panic!("primary typed error changed: {other:?}"),
                }
            }
            other => panic!("failure lost its stage proof: {other:?}"),
        }
        let diagnostic = failure.to_string();
        assert!(diagnostic.contains("during runner receipt"));
        assert!(diagnostic.contains("CLI status exit status: 23"));
        assert!(diagnostic.contains("stderr observed=22 retained=22 truncated=false"));
        assert!(diagnostic.contains("bounded stderr tail: pre-receipt diagnostic"));
    }

    #[cfg(feature = "import-baseline")]
    #[test]
    fn delayed_pre_receipt_exit_preserves_source_stage_status_and_stderr() {
        let (mut child, capture) =
            diagnostic_child("sleep 0.05; printf '%s' 'late receipt diagnostic' >&2; exit 31");
        let failure = staged_launch_failure(
            stderr::Stage::RunnerReceipt,
            receipt_eof(),
            &mut child,
            capture,
        );
        match &failure {
            Error::StagedLaunch { stage, source, .. } => {
                assert_eq!(*stage, stderr::Stage::RunnerReceipt);
                assert!(matches!(
                    source.as_ref(),
                    Error::Evidence(rawr_import::metrics::evidence::Error::Io(error))
                        if error.kind() == std::io::ErrorKind::UnexpectedEof
                ));
            }
            other => panic!("delayed failure lost its source proof: {other:?}"),
        }
        let diagnostic = failure.to_string();
        assert!(diagnostic.contains("during runner receipt"));
        assert!(diagnostic.contains("CLI status exit status: 31"));
        assert!(diagnostic.contains("reader=complete"));
        assert!(diagnostic.contains("bounded stderr tail: late receipt diagnostic"));
    }

    #[cfg(feature = "import-baseline")]
    #[test]
    fn pre_receipt_exit_with_empty_stderr_has_explicit_fallback() {
        let (mut child, capture, status) = exited_diagnostic_child("exit 17");
        assert_eq!(status.code(), Some(17));
        let failure = staged_launch_failure(
            stderr::Stage::RunnerReceipt,
            receipt_eof(),
            &mut child,
            capture,
        );
        let diagnostic = failure.to_string();
        assert!(diagnostic.contains("CLI status exit status: 17"));
        assert!(diagnostic.contains("stderr observed=0 retained=0 truncated=false"));
        assert!(diagnostic.contains("bounded stderr tail: <empty>"));
    }

    #[cfg(feature = "import-baseline")]
    #[test]
    fn successful_spawn_drops_parent_writer_and_retires_concurrent_drain() {
        let (_child, capture, status) =
            exited_diagnostic_child("printf '%s' 'ordinary diagnostic' >&2; exit 0");
        assert!(status.success());
        capture
            .finish(&status)
            .expect("successful retirement observes EOF without a retained parent writer");
    }

    #[cfg(feature = "import-baseline")]
    #[test]
    fn retirement_overflow_preserves_typed_source_stage_status_and_tail() {
        let script = "payload=x; while [ \"${#payload}\" -lt 65536 ]; do payload=$payload$payload; done; printf '%sZ' \"$payload\" >&2; exit 0";
        let (_child, capture, status) = exited_diagnostic_child(script);
        assert!(status.success());
        let diagnostic_failure = capture
            .finish(&status)
            .expect_err("one byte beyond the fixed stderr tail fails closed");
        let failure = staged_stderr_failure(stderr::Stage::Retirement, diagnostic_failure);
        match &failure {
            Error::StagedLaunch { stage, source, .. } => {
                assert_eq!(*stage, stderr::Stage::Retirement);
                assert!(matches!(
                    source.as_ref(),
                    Error::Stderr(stderr::Error::Limit { limit })
                        if *limit == super::OUTPUT_BYTES
                ));
            }
            other => panic!("diagnostic failure lost its stage proof: {other:?}"),
        }
        let diagnostic = failure.to_string();
        assert!(diagnostic.contains("during window retirement"));
        assert!(diagnostic.contains("CLI status exit status: 0"));
        assert!(diagnostic.contains("stderr observed=65537 retained=65536 truncated=true"));
        assert!(diagnostic.contains("reader=complete"));
        assert!(diagnostic.ends_with('Z'));
    }

    #[test]
    fn immutable_assets_freeze_all_scenarios_and_the_exact_oracle() {
        let assets = Assets::transform().expect("bounded immutable assets validate");
        assert_eq!(assets.oracle.rows[0].outcomes[0], Outcome::Execute);
        assert_eq!(assets.oracle.rows[1].outcomes[0], Outcome::Hit);
        assert_eq!(assets.oracle.rows[8].outcomes[10], Outcome::RejectExecute);
        assert_eq!(assets.oracle.rows[10].outcomes[0], Outcome::ExecuteHit);
    }

    #[cfg(feature = "import-baseline")]
    #[test]
    fn feature_dispatch_has_all_thirty_five_executable_cells() {
        let assets = Assets::transform().expect("bounded immutable assets validate");
        let plans = assets.plans();
        assert_eq!(plans.len(), 35);
        let executed = plans
            .iter()
            .map(|plan| match plan {
                Plan::Execute { scenario, command } => (*scenario, *command),
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(executed.len(), 35);
        assert!(plans
            .iter()
            .all(|plan| matches!(plan, Plan::Execute { .. })));
    }

    #[cfg(feature = "import-baseline")]
    #[test]
    fn profile_comparison_requires_the_exact_prefix_and_suffix() {
        use rawr_import::metrics::{pipeline, profile, recorder::Snapshot, runner};

        fn snapshot(selection: profile::Selection, suffix: u8) -> Snapshot {
            Snapshot {
                principal: None,
                accounting: Box::default(),
                nodes: Box::default(),
                processes: Box::default(),
                milestones: Box::default(),
                runner: Some(runner::Owned {
                    runner_pid: u32::from(suffix),
                    runner_uid: 501,
                    build_request: [suffix; 32],
                    generated_manifest: [suffix.wrapping_add(1); 32],
                    generation_key: [suffix.wrapping_add(2); 32],
                    epoch: u64::from(suffix),
                    artifact: [suffix.wrapping_add(3); 32],
                    executable_path: Box::from(b"/baseline/runner".as_slice()),
                    executable_digest: [23; 32],
                    executable_bytes: 42,
                    spawned_ns: Some(u64::from(suffix)),
                }),
                profile: Some(profile::Evidence::observe(
                    selection,
                    [12; 32],
                    u32::from(suffix),
                    [suffix.wrapping_add(4); 32],
                )),
                pipeline: Some(pipeline::Evidence {
                    toolchain: [1; 32],
                    resolution: [2; 32],
                    api: [3; 32],
                    generic: [4; 32],
                    profile: [suffix.wrapping_add(5); 32],
                    facade: [6; 32],
                    checked: [suffix.wrapping_add(7); 32],
                    profile_proof: [suffix.wrapping_add(8); 32],
                    native_inputs: [9; 32],
                    native_product: [suffix.wrapping_add(10); 32],
                    overlay: [suffix.wrapping_add(11); 32],
                    resolution_target: None,
                    runner_target: None,
                }),
            }
        }

        let baseline = snapshot(profile::Selection::Baseline, 17);
        let mut variant = snapshot(profile::Selection::Variant, 18);
        super::validate_profile_comparison(&baseline, &variant)
            .expect("the exact H,H,H,H,E,H,E,E,E,E,E row validates");

        variant.pipeline.as_mut().expect("pipeline exists").facade = [99; 32];
        super::validate_profile_comparison(&baseline, &variant)
            .expect_err("a changed facade invalidates the required stable hit");
    }

    #[cfg(feature = "import-baseline")]
    #[test]
    fn cache_summary_preserves_directory_presence_and_permissions() {
        use rawr_import::metrics::cell::project;
        use std::os::unix::fs::PermissionsExt as _;
        let assets = Assets::transform().unwrap();
        let fixture = assets.materialize().unwrap();
        let root = fixture.root.join("target/rawr/cache");
        let absent = super::tree_state(&root).unwrap();
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("empty")).unwrap();
        let present = super::tree_state(&root).unwrap();
        assert_eq!(absent.bytes, present.bytes);
        assert_ne!(absent.digest, present.digest);
        let current = fs::metadata(&root).unwrap().permissions().mode();
        fs::set_permissions(&root, fs::Permissions::from_mode(current ^ 0o020)).unwrap();
        let changed = super::tree_state(&root).unwrap();
        assert_eq!(present.bytes, changed.bytes);
        assert_ne!(present.digest, changed.digest);
        let manifest = project::Manifest::read(&root).unwrap();
        assert_eq!(changed.bytes, manifest.state.bytes);
        assert_eq!(changed.digest, super::hex(&manifest.state.digest));
    }

    #[test]
    fn materialized_mutations_never_change_the_immutable_assets() {
        let assets = Assets::transform().expect("bounded immutable assets validate");
        let before = super::digest_assets(&assets.root).expect("asset digest");
        for scenario in rawr_import::metrics::report::Scenario::CURRENT {
            let project = assets.materialize().expect("fixture materializes");
            assets.apply(scenario, &project).expect("mutation applies");
        }
        assert_eq!(
            super::digest_assets(&assets.root).expect("asset digest remains"),
            before
        );
    }

    #[test]
    fn materialization_rejects_a_colliding_root_without_reuse_or_cleanup() {
        let assets = Assets::transform().expect("bounded immutable assets validate");
        let root = std::env::temp_dir().join(format!(
            "rawr-import-baseline-collision-{}-{}",
            std::process::id(),
            super::NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ));
        fs::create_dir(&root).expect("collision root creates once");
        let sentinel = root.join("sentinel");
        fs::write(&sentinel, b"last-good").expect("collision sentinel writes");

        let error = match assets.materialize_at(root.clone()) {
            Ok(_) => panic!("existing materialization root was reused"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::MaterializationCollision { path } if path == root));
        assert_eq!(
            fs::read(&sentinel).expect("collision sentinel remains"),
            b"last-good"
        );

        fs::remove_dir_all(&root).expect("test collision root cleans after verification");
    }

    #[test]
    fn fixture_manifest_retains_the_governed_package_list_calibration() {
        let assets = Assets::transform().expect("bounded immutable assets validate");
        assert_eq!(
            assets
                .fixture()
                .expect("calibrated fixture seals")
                .package_list_fallbacks,
            0
        );
    }

    #[cfg(feature = "import-baseline")]
    #[test]
    fn process_time_parser_accepts_governed_macos_ps_shapes() {
        assert_eq!(super::parse_process_time("0:00.25").unwrap(), 250_000_000);
        assert_eq!(
            super::parse_process_time("1:02:03.5").unwrap(),
            3_723_500_000_000
        );
        assert!(super::parse_process_time("1:2:3:4").is_err());
    }

    #[cfg(feature = "import-baseline")]
    #[test]
    fn process_tree_accounting_tracks_private_groups_and_rejects_pid_reuse() {
        let mut usage = super::GroupUsage::default();
        assert!(super::observe_process_document(
            100,
            &mut usage,
            "100 1 100 10 0:00.10 0:00.01\n101 100 100 20 0:00.20 0:00.02\n102 101 102 5 0:00.05 0:00.005\n",
        )
        .expect("initial group sample seals"));
        assert_eq!(usage.resources.user_ns, 350_000_000);
        assert_eq!(usage.resources.system_ns, 35_000_000);
        assert_eq!(usage.resources.peak_rss_bytes, 35 * 1024);
        assert_eq!(
            usage.live_groups,
            std::collections::BTreeSet::from([100, 102])
        );

        assert!(super::observe_process_document(
            100,
            &mut usage,
            "101 1 100 5 0:00.25 0:00.03\n102 1 102 4 0:00.08 0:00.01\n",
        )
        .expect("reparented private groups remain attributed"));
        assert_eq!(usage.resources.user_ns, 430_000_000);
        assert_eq!(usage.resources.system_ns, 50_000_000);
        assert_eq!(usage.resources.peak_rss_bytes, 35 * 1024);
        assert!(!super::observe_process_document(100, &mut usage, "")
            .expect("empty final process table proves retirement"));
        assert!(
            super::observe_process_document(100, &mut usage, "101 1 999 5 0:00.25 0:00.03\n",)
                .is_err()
        );
    }

    #[cfg(feature = "import-baseline")]
    #[test]
    fn cargo_package_list_excludes_every_runtime_title_shape() {
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt as _;

        let assets = Assets::transform().expect("bounded immutable assets validate");
        let project = assets.materialize().expect("fixture materializes");
        let temporary = project
            .root
            .join("native/.phase9-window-title.package-list.tmp");
        let mut temporary_file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .expect("temporary title shape creates");
        temporary_file
            .write_all(b"excluded temporary title")
            .expect("temporary title shape writes");
        temporary_file.sync_all().expect("temporary title syncs");

        let first = super::TitleFile::transform(&project, "First Runtime Title", [31; 32])
            .expect("first title identity seals");
        let first_list = package_list(&project);
        first.finish().expect("first title retires");

        let second = super::TitleFile::transform(&project, "Second Runtime Title", [32; 32])
            .expect("second title identity seals");
        let second_list = package_list(&project);
        second.finish().expect("second title retires");
        fs::remove_file(&temporary).expect("temporary title retires");

        assert_eq!(first_list, second_list);
        assert!(!first_list
            .split(|byte| *byte == b'\n')
            .any(|line| line.starts_with(b".phase9-window-title")));
    }

    #[cfg(feature = "import-baseline")]
    #[test]
    #[ignore = "governed title-independence owner calibration"]
    fn runtime_title_bytes_are_outside_every_owner_identity() {
        let assets = Assets::transform().expect("bounded immutable assets validate");
        let project = assets.materialize().expect("fixture materializes");
        assets
            .apply(
                rawr_import::metrics::report::Scenario::IdenticalWarm,
                &project,
            )
            .expect("identical fixture applies");
        let executable = super::release_artifact("evidence").expect("release evidence exists");
        let environment = super::environment().expect("baseline environment seals");
        let build =
            super::release_build(&executable, &environment).expect("release build identity seals");
        let authority = rawr_import::metrics::attribution::Authority::transform(
            build.source_tree,
            build.executable_digest_sha256,
        )
        .expect("reviewed title-attribution authority seals");
        let cache_root = project.root.join("target/rawr/cache");
        let corpus = rawr_import::metrics::cell::corpus_id().expect("corpus id seals");
        let setup = super::isolated_cell(
            &executable,
            &assets,
            &project,
            corpus,
            rawr_import::metrics::report::Scenario::IdenticalWarm,
            rawr_import::metrics::report::Command::Check,
            rawr_import::metrics::cell::Role::Setup,
            rawr_import::metrics::cell::Profile::Inactive,
        )
        .expect("fixture preparation is one isolated child");

        let first = super::TitleFile::transform(&project, "First Runtime Title", [41; 32])
            .expect("first title identity seals");
        let first_title_digest = first.identity().digest();
        let first_list = package_list(&project);
        let first_before = rawr_import::metrics::attribution::Snapshot::transform(
            &cache_root,
            authority.clone(),
            setup.cache.after,
        )
        .expect("first raw/projected prestate seals");
        let first_launch = super::isolated_cell(
            &executable,
            &assets,
            &project,
            corpus,
            rawr_import::metrics::report::Scenario::IdenticalWarm,
            rawr_import::metrics::report::Command::Check,
            rawr_import::metrics::cell::Role::RetainedCell,
            rawr_import::metrics::cell::Profile::Inactive,
        )
        .expect("first warm owner is one isolated child");
        let first_after = rawr_import::metrics::attribution::Snapshot::transform(
            &cache_root,
            authority.clone(),
            first_launch.cache.after.clone(),
        )
        .expect("first raw/projected poststate seals");
        first.finish().expect("first title retires");
        assert_eq!(first_before.raw, first_launch.cache.before);
        assert_eq!(first_after.raw, first_launch.cache.after);
        let first_transition = rawr_import::metrics::attribution::Transition::transform(
            first_before,
            first_after.clone(),
        )
        .expect("first title transition seals");
        let first_snapshot = match first_launch.capsule {
            rawr_import::metrics::cell::Capsule::Retained(capsule) => capsule.counters,
            rawr_import::metrics::cell::Capsule::Setup(_) => panic!("retained role changed"),
        };

        let second = super::TitleFile::transform(&project, "Second Runtime Title", [42; 32])
            .expect("second title identity seals");
        let second_title_digest = second.identity().digest();
        let second_list = package_list(&project);
        let second_launch = super::isolated_cell(
            &executable,
            &assets,
            &project,
            corpus,
            rawr_import::metrics::report::Scenario::IdenticalWarm,
            rawr_import::metrics::report::Command::Check,
            rawr_import::metrics::cell::Role::RetainedCell,
            rawr_import::metrics::cell::Profile::Inactive,
        )
        .expect("second warm owner is one isolated child");
        let second_after = rawr_import::metrics::attribution::Snapshot::transform(
            &cache_root,
            authority,
            second_launch.cache.after.clone(),
        )
        .expect("second raw/projected poststate seals");
        second.finish().expect("second title retires");
        assert_eq!(first_after.raw, second_launch.cache.before);
        assert_eq!(second_after.raw, second_launch.cache.after);
        let second_transition =
            rawr_import::metrics::attribution::Transition::transform(first_after, second_after)
                .expect("second title transition seals");
        let second_snapshot = match second_launch.capsule {
            rawr_import::metrics::cell::Capsule::Retained(capsule) => capsule.counters,
            rawr_import::metrics::cell::Capsule::Setup(_) => panic!("retained role changed"),
        };

        assert_eq!(first_list, second_list);
        assert_eq!(first_snapshot.nodes, second_snapshot.nodes);
        assert_eq!(first_snapshot.processes, second_snapshot.processes);
        assert_eq!(first_snapshot.profile, second_snapshot.profile);
        assert_eq!(first_snapshot.pipeline, second_snapshot.pipeline);
        let first_runner = first_snapshot.runner.expect("first runner identity seals");
        let second_runner = second_snapshot
            .runner
            .expect("second runner identity seals");
        assert_eq!(first_runner.build_request, second_runner.build_request);
        assert_eq!(
            first_runner.generated_manifest,
            second_runner.generated_manifest
        );
        assert_eq!(first_runner.generation_key, second_runner.generation_key);
        assert_eq!(first_runner.epoch, second_runner.epoch);
        assert_eq!(first_runner.artifact, second_runner.artifact);
        assert_eq!(first_runner.executable_path, second_runner.executable_path);
        rawr_import::metrics::attribution::Comparison::transform(
            first_title_digest,
            second_title_digest,
            first_transition,
            second_transition,
        )
        .expect("title causality seals without changing raw cache evidence");
    }

    #[cfg(feature = "import-baseline")]
    fn package_list(project: &super::Project) -> Box<[u8]> {
        let output = std::process::Command::new("rustup")
            .args([
                "run",
                "nightly-2026-07-11",
                "cargo",
                "package",
                "--list",
                "--allow-dirty",
                "--manifest-path",
            ])
            .arg(project.root.join("native/Cargo.toml"))
            .output()
            .expect("pinned Cargo package list executes");
        assert!(
            output.status.success(),
            "pinned Cargo package list failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.len() <= super::OUTPUT_BYTES);
        output.stdout.into_boxed_slice()
    }

    #[test]
    #[ignore = "one governed cold-check calibration for fixture P"]
    fn calibrate_package_list_fallbacks() {
        let assets = Assets::transform().expect("bounded immutable assets validate");
        let project = assets.materialize().expect("fixture materializes");
        assets
            .apply(rawr_import::metrics::report::Scenario::CleanCold, &project)
            .expect("cold fixture applies");
        let executable = super::release_artifact("evidence").expect("release evidence exists");
        let launch = super::isolated_cell(
            &executable,
            &assets,
            &project,
            rawr_import::metrics::cell::corpus_id().expect("corpus id seals"),
            rawr_import::metrics::report::Scenario::CleanCold,
            rawr_import::metrics::report::Command::Check,
            rawr_import::metrics::cell::Role::RetainedCell,
            rawr_import::metrics::cell::Profile::Inactive,
        )
        .expect("cold check is one isolated child");
        let snapshot = match launch.capsule {
            rawr_import::metrics::cell::Capsule::Retained(capsule) => capsule.counters,
            rawr_import::metrics::cell::Capsule::Setup(_) => panic!("retained role changed"),
        };
        let packages = snapshot
            .processes
            .iter()
            .find(|row| row.kind == rawr_import::metrics::ProcessKind::PackageList)
            .expect("fixed process row");
        eprintln!(
            "BASELINE_FIXTURE digest={} package_list_fallbacks={}",
            super::digest_assets(&assets.root).expect("asset digest"),
            packages.count,
        );
    }

    #[cfg(feature = "import-baseline")]
    #[test]
    #[ignore = "complete thirty-five-cell current-state corpus"]
    fn record_complete_current_state_corpus() {
        let assets = Assets::transform().expect("bounded immutable assets validate");
        let environment = super::environment().expect("machine identity captures");
        let output =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/phase9-01-baseline/current");
        fs::create_dir_all(&output).expect("bounded evidence directory creates");
        let executable = super::release_artifact("evidence").expect("release evidence exists");
        let corpus_id = rawr_import::metrics::cell::corpus_id().expect("corpus id seals");
        let mut cells = Vec::new();
        for command in rawr_import::metrics::report::Command::ALL {
            let project = assets.materialize().expect("fixture materializes");
            for scenario in rawr_import::metrics::report::Scenario::CURRENT {
                let cell = super::observe(
                    &assets,
                    &project,
                    &executable,
                    corpus_id,
                    scenario,
                    command,
                    environment.clone(),
                )
                .unwrap_or_else(|error| panic!("{scenario:?}/{command:?}: {error}"));
                let path = output.join(format!("{scenario:?}-{command:?}.json").to_lowercase());
                fs::write(
                    &path,
                    cell.canonical_bytes().expect("canonical cell encodes"),
                )
                .expect("canonical cell writes");
                cells.push(cell);
            }
        }
        let corpus = rawr_import::metrics::report::Corpus::transform(cells.into_boxed_slice())
            .expect("the feature-built corpus contains every canonical cell exactly once");
        fs::write(
            output.join("corpus.json"),
            corpus
                .canonical_bytes()
                .expect("corpus canonically encodes"),
        )
        .expect("corpus report writes");
    }

    #[cfg(feature = "import-baseline")]
    #[test]
    #[ignore = "one controlled AX close and normal-retirement qualification"]
    fn qualify_single_authenticated_window_retirement() {
        #[cfg(not(target_os = "macos"))]
        panic!("the governed AX retirement requires macOS");

        let assets = Assets::transform().expect("bounded immutable assets validate");
        let project = assets.materialize().expect("fixture materializes");
        assets
            .apply(
                rawr_import::metrics::report::Scenario::IdenticalWarm,
                &project,
            )
            .expect("identical fixture applies");
        let executable = super::release_artifact("evidence").expect("release evidence exists");
        super::isolated_cell(
            &executable,
            &assets,
            &project,
            rawr_import::metrics::cell::corpus_id().expect("corpus id seals"),
            rawr_import::metrics::report::Scenario::IdenticalWarm,
            rawr_import::metrics::report::Command::Run,
            rawr_import::metrics::cell::Role::Setup,
            rawr_import::metrics::cell::Profile::Inactive,
        )
        .expect("matching run owner warms in one isolated child");
        let fixture = assets.fixture().expect("fixture seals");
        let fixture_digest = rawr_import::metrics::evidence::parse_digest(std::ffi::OsStr::new(
            fixture.digest_sha256.as_ref(),
        ))
        .expect("fixture digest parses");
        let launch = super::launch(
            &executable,
            &project,
            fixture_digest,
            rawr_import::metrics::report::Trial::Measured { ordinal: 1 },
        )
        .expect("one exact launch retires normally");
        super::launch_sample(
            launch,
            rawr_import::metrics::report::Trial::Measured { ordinal: 1 },
        )
        .expect("one authenticated launch seals");
    }

    #[cfg(feature = "import-baseline")]
    #[test]
    #[ignore = "authenticated fresh-process warm-run series: 3 warmups plus 5 measured"]
    fn record_authenticated_warm_run_series() {
        let assets = Assets::transform().expect("bounded immutable assets validate");
        let project = assets.materialize().expect("fixture materializes");
        let series = super::warm_series(&assets, &project)
            .unwrap_or_else(|error| panic!("warm series failed: {error}"));
        let output = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/phase9-01-baseline/warm-run-series.json");
        fs::create_dir_all(output.parent().expect("report has parent"))
            .expect("bounded report directory creates");
        fs::write(
            output,
            series
                .canonical_bytes()
                .expect("series canonically encodes"),
        )
        .expect("series report writes");
    }

    #[cfg(feature = "import-baseline")]
    #[test]
    #[ignore = "real release window gate: three warmups and five retained launches; explicit operator only"]
    fn record_authoritative_release_warm_series() {
        let assets = Assets::transform().expect("bounded immutable assets validate");
        let project = assets.materialize().expect("fixture materializes");
        let qualified = super::release::run(&assets, &project)
            .unwrap_or_else(|error| panic!("release warm series failed without retry: {error}"));
        let output = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../target/phase9-08-release/warm-run-series.json");
        let bytes = qualified
            .canonical_bytes()
            .expect("live release proof retains all observations");
        fs::create_dir_all(output.parent().expect("report has parent"))
            .expect("bounded report directory creates");
        fs::write(output, bytes).expect("release series report writes");
    }
}
