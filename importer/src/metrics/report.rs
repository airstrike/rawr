//! Canonical import baseline evidence protocol.

use super::recorder::Snapshot;

pub mod timing;

const CELL_SCHEMA: &str = "rawr.phase9.baseline.cell/v4";
const CORPUS_SCHEMA: &str = "rawr.phase9.baseline.corpus/v4";
const SERIES_SCHEMA: &str = "rawr.phase9.baseline.descriptive-series/v4";
const WARMUPS: u8 = 3;
const MEASURED: u8 = 5;
const REPORT_BYTES: usize = 8 * 1024 * 1024;
const TEXT_BYTES: usize = 16 * 1024;

/// One frozen baseline scenario.
#[derive(
    Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, serde::Deserialize, serde::Serialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum Scenario {
    CleanCold,
    IdenticalWarm,
    RawrOccurrence,
    RustBodyEqualApi,
    RustPublicApi,
    ProfileRule,
    ClassifierFacade,
    DemandOnly,
    CorruptRunner,
    CorruptResolution,
    ConcurrentCold,
}

impl Scenario {
    /// Scenarios executable against the current whole-owner pipeline.
    pub const CURRENT: [Self; 8] = [
        Self::CleanCold,
        Self::IdenticalWarm,
        Self::RawrOccurrence,
        Self::RustBodyEqualApi,
        Self::RustPublicApi,
        Self::ProfileRule,
        Self::ClassifierFacade,
        Self::DemandOnly,
    ];

    /// Rows whose node oracle is inapplicable until the DAG/store exists.
    pub const AFTER_CUTOVER: [Self; 3] = [
        Self::CorruptRunner,
        Self::CorruptResolution,
        Self::ConcurrentCold,
    ];
}

/// One production CLI owner exercised by the baseline.
#[derive(
    Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, serde::Deserialize, serde::Serialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum Command {
    Check,
    Run,
    Preview,
    Snapshot,
    Watch,
}

impl Command {
    /// The complete fixed production-owner set.
    pub const ALL: [Self; 5] = [
        Self::Check,
        Self::Run,
        Self::Preview,
        Self::Snapshot,
        Self::Watch,
    ];
}

/// Explicit availability for platform- or lifecycle-dependent observations.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum Observation<T> {
    Measured { value: T },
    NotApplicable { reason: Box<str> },
    Unavailable { reason: Box<str> },
}

/// Sampling semantics encoded independently from sequence length.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Trial {
    Exact,
    Warmup { ordinal: u8 },
    Measured { ordinal: u8 },
}

/// Stable machine and toolchain identity.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Environment {
    pub machine: Box<str>,
    pub operating_system: Box<str>,
    pub architecture: Box<str>,
    pub cpu: Box<str>,
    pub rustc: Box<str>,
    pub rustdoc: Box<str>,
    pub cargo: Box<str>,
    pub driver: Box<str>,
    pub target: Box<str>,
    pub profile: Box<str>,
}

/// Immutable fixture identity and its frozen package-list fallback count `P`.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Fixture {
    pub name: Box<str>,
    pub digest_sha256: Box<str>,
    pub package_list_fallbacks: u64,
}

/// Exact feature-built release artifact and reproducible build identity.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Build {
    pub executable_path: Box<str>,
    pub executable_digest_sha256: Box<str>,
    pub device: u64,
    pub inode: u64,
    pub size: u64,
    pub source_tree: Box<str>,
    pub lockfile_digest_sha256: Box<str>,
    pub compiler: Box<str>,
    pub cargo: Box<str>,
    pub target: Box<str>,
    pub profile: Box<str>,
    pub features: Box<[Box<str>]>,
}

/// Exact cache preparation and observed state for one execution.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Cache {
    pub preparation: Box<str>,
    pub state: Box<str>,
    pub bytes_before: u64,
    pub bytes_after: u64,
    pub durable_mutations: Box<[Box<str>]>,
}

/// Content and owner identities compared across scenarios.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Identities {
    pub resolution: Observation<Box<str>>,
    pub api: Observation<Box<str>>,
    pub checked: Observation<Box<str>>,
    pub native_product: Observation<Box<str>>,
    pub overlay: Observation<Box<str>>,
    pub build_request: Observation<Box<str>>,
    pub generated_manifest: Observation<Box<str>>,
    pub built_artifact: Observation<Box<str>>,
    pub rust_owner: Observation<Box<str>>,
    pub process_epoch: Observation<Box<str>>,
}

/// One exact, warmup, or measured production-owner execution.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Sample {
    pub trial: Trial,
    pub wall_ns: u64,
    pub cpu_user_ns: Observation<u64>,
    pub cpu_system_ns: Observation<u64>,
    pub peak_rss_bytes: Observation<u64>,
    pub admitted_ns: Observation<u64>,
    pub started_ns: Observation<u64>,
    pub timing: timing::Timing,
    pub retirement: timing::Retirement,
    pub updates_per_second_milli: Observation<u64>,
    pub responsiveness_ns: Observation<u64>,
    pub rollback: Observation<bool>,
    pub last_good_ns: Observation<u64>,
    pub counters: Snapshot,
    pub cache: Cache,
    pub identities: Identities,
}

/// Parent observations and complete hash-linked setup capsule.
#[cfg(feature = "import-baseline")]
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Preparation {
    pub capsule: super::cell::SetupCapsule,
    pub capsule_digest: [u8; 32],
    pub launch_wall_ns: u64,
    pub cpu_user_ns: u64,
    pub cpu_system_ns: u64,
    pub peak_rss_bytes: u64,
    pub sampler_count: u64,
    pub cache: super::cell::project::Transition,
}

/// Parent observations and complete hash-linked retained-cell capsule.
#[cfg(feature = "import-baseline")]
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Invocation {
    pub capsule: super::cell::CellCapsule,
    pub capsule_digest: [u8; 32],
    pub launch_wall_ns: u64,
    pub cpu_user_ns: u64,
    pub cpu_system_ns: u64,
    pub peak_rss_bytes: u64,
    pub sampler_count: u64,
    pub cache: super::cell::project::Transition,
}

/// One canonical complete deterministic scenario/owner cell.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Cell {
    schema: Box<str>,
    pub scenario: Scenario,
    pub command: Command,
    pub lifecycle: Box<str>,
    pub environment: Environment,
    pub fixture: Fixture,
    #[cfg(feature = "import-baseline")]
    pub preparation: Option<Preparation>,
    #[cfg(feature = "import-baseline")]
    pub invocation: Option<Invocation>,
    pub sample: Sample,
}

/// The exact forty-cell current-pipeline corpus.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Corpus {
    schema: Box<str>,
    pub cells: Box<[Cell]>,
}

/// The distinct identical-warm `run` launch series.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Series {
    schema: Box<str>,
    pub scenario: Scenario,
    pub command: Command,
    pub lifecycle: Box<str>,
    pub environment: Environment,
    pub fixture: Fixture,
    pub build: Build,
    pub samples: Box<[Sample]>,
}

/// Failure to seal or encode canonical baseline evidence.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("baseline accounting evidence is malformed or incomplete after a recording fault")]
    Accounting,
    #[error("a deterministic baseline cell must contain one exact trial")]
    CellTrial,
    #[cfg(feature = "import-baseline")]
    #[error("a cell has incomplete or inconsistent parent/capsule evidence")]
    CellEvidence,
    #[error("the current baseline corpus must contain every one of eight scenarios by five owners exactly once")]
    CorpusCells,
    #[error("the warm-run series must contain ordered warmup 1..=3 then measured 1..=5 trials")]
    SeriesTrials,
    #[error("the warm-run series changed a title-independent runner, cache, or process identity")]
    SeriesIdentity,
    #[error("the descriptive warm-run series schema or fixed owner identity is invalid")]
    SeriesSchema,
    #[error(
        "the descriptive warm-run series contains a non-descriptive or incomplete timing state"
    )]
    SeriesTiming,
    #[error("timing evidence moved before its pre-spawn clock origin")]
    TimingClock,
    #[error("timing evidence contains inconsistent timestamp or comparison facts")]
    TimingEvidence,
    #[error("release visibility exceeds its absolute fifteen-second deadline")]
    ReleaseVisible,
    #[error("descriptive visibility exceeds its absolute thirty-minute envelope")]
    DescriptiveVisible,
    #[error("retirement evidence contains inconsistent lifecycle facts")]
    RetirementEvidence,
    #[error("post-press retirement exceeds its fixed ten-second deadline")]
    RetirementDeadline,
    #[error("baseline report text exceeds {limit} bytes")]
    Text { limit: usize },
    #[error("baseline report field {field} must not be empty")]
    Empty { field: &'static str },
    #[error("baseline fixture digest must be exactly 64 lowercase hexadecimal characters")]
    FixtureDigest,
    #[error("baseline report JSON encoding failed")]
    Encode(#[source] serde_json::Error),
    #[cfg(feature = "import-baseline")]
    #[error(transparent)]
    CellProtocol(#[from] super::cell::Error),
    #[error("baseline report exceeds {limit} bytes")]
    Size { limit: usize },
}

#[cfg(feature = "import-baseline")]
fn measured(observation: &Observation<u64>) -> u64 {
    match observation {
        Observation::Measured { value } => *value,
        Observation::NotApplicable { .. } | Observation::Unavailable { .. } => u64::MAX,
    }
}

impl Cell {
    /// Transform one complete exact execution into a deterministic cell.
    pub fn transform(
        scenario: Scenario,
        command: Command,
        lifecycle: Box<str>,
        environment: Environment,
        fixture: Fixture,
        sample: Sample,
    ) -> Result<Self, Error> {
        if sample.trial != Trial::Exact {
            return Err(Error::CellTrial);
        }
        let cell = Self {
            schema: CELL_SCHEMA.into(),
            scenario,
            command,
            lifecycle,
            environment,
            fixture,
            #[cfg(feature = "import-baseline")]
            preparation: None,
            #[cfg(feature = "import-baseline")]
            invocation: None,
            sample,
        };
        validate_text(
            &cell.environment,
            &cell.fixture,
            &cell.lifecycle,
            [&cell.sample],
        )?;
        Ok(cell)
    }

    /// Bind the complete parent/capsule evidence to one exact cell.
    #[cfg(feature = "import-baseline")]
    #[allow(clippy::too_many_arguments)]
    pub fn transform_isolated(
        scenario: Scenario,
        command: Command,
        lifecycle: Box<str>,
        environment: Environment,
        fixture: Fixture,
        preparation: Option<Preparation>,
        invocation: Invocation,
        sample: Sample,
    ) -> Result<Self, Error> {
        let needs_preparation = scenario != Scenario::CleanCold;
        let preparation_valid = match preparation.as_ref() {
            Some(preparation) => {
                preparation.capsule.digest()? == preparation.capsule_digest
                    && preparation.capsule.request.core.scenario == scenario
                    && preparation.capsule.request.core.owner == command
                    && preparation.capsule.request.core.build
                        == invocation.capsule.request.core.build
                    && preparation.capsule.request.core.corpus_id
                        == invocation.capsule.request.core.corpus_id
                    && preparation.capsule.request.core.fixture_digest
                        == invocation.capsule.request.core.fixture_digest
                    && preparation.cache.before.state
                        == preparation.capsule.request.core.project.cache
                    && preparation.cache.after.state
                        == invocation.capsule.request.core.project.cache
                    && matches!(
                        preparation.capsule.outcome,
                        super::cell::Outcome::Success { .. }
                    )
                    && preparation.sampler_count > 0
            }
            None => !needs_preparation,
        };
        if !preparation_valid
            || needs_preparation != preparation.is_some()
            || invocation.capsule.digest()? != invocation.capsule_digest
            || invocation.capsule.request.core.scenario != scenario
            || invocation.capsule.request.core.owner != command
            || !matches!(
                invocation.capsule.outcome,
                super::cell::Outcome::Success { .. }
            )
            || invocation.capsule.counters != sample.counters
            || invocation.cache.before.state != invocation.capsule.request.core.project.cache
            || invocation.cache.before.state.bytes != sample.cache.bytes_before
            || invocation.cache.after.state.bytes != sample.cache.bytes_after
            || !cache_consistent(&invocation.cache)
            || invocation.cpu_user_ns != measured(&sample.cpu_user_ns)
            || invocation.cpu_system_ns != measured(&sample.cpu_system_ns)
            || invocation.peak_rss_bytes != measured(&sample.peak_rss_bytes)
            || invocation.sampler_count == 0
        {
            return Err(Error::CellEvidence);
        }
        let mut cell = Self::transform(scenario, command, lifecycle, environment, fixture, sample)?;
        cell.preparation = preparation;
        cell.invocation = Some(invocation);
        Ok(cell)
    }

    /// Encode stable struct/sequence order as pretty JSON with one final newline.
    pub fn canonical_bytes(&self) -> Result<Box<[u8]>, Error> {
        canonical_bytes(self)
    }
}

impl Corpus {
    /// Seal only the complete current-state Cartesian coverage.
    pub fn transform(cells: Box<[Cell]>) -> Result<Self, Error> {
        if cells.len() != Scenario::CURRENT.len() * Command::ALL.len()
            || Scenario::CURRENT.iter().any(|scenario| {
                Command::ALL.iter().any(|command| {
                    cells
                        .iter()
                        .filter(|cell| cell.scenario == *scenario && cell.command == *command)
                        .count()
                        != 1
                })
            })
            || cells
                .iter()
                .any(|cell| !Scenario::CURRENT.contains(&cell.scenario))
        {
            return Err(Error::CorpusCells);
        }
        let mut cells = cells.into_vec();
        cells.sort_unstable_by_key(|cell| (cell.scenario, cell.command));
        Ok(Self {
            schema: CORPUS_SCHEMA.into(),
            cells: cells.into_boxed_slice(),
        })
    }

    /// Encode stable struct/sequence order as pretty JSON with one final newline.
    pub fn canonical_bytes(&self) -> Result<Box<[u8]>, Error> {
        canonical_bytes(self)
    }
}

impl Series {
    /// Transform the exact sampling sequence into the distinct warm-run series.
    pub fn transform(
        lifecycle: Box<str>,
        environment: Environment,
        fixture: Fixture,
        build: Build,
        samples: Box<[Sample]>,
    ) -> Result<Self, Error> {
        let series = Self {
            schema: SERIES_SCHEMA.into(),
            scenario: Scenario::IdenticalWarm,
            command: Command::Run,
            lifecycle,
            environment,
            fixture,
            build,
            samples,
        };
        series.validate()?;
        Ok(series)
    }

    fn validate(&self) -> Result<(), Error> {
        if self.schema.as_ref() != SERIES_SCHEMA
            || self.scenario != Scenario::IdenticalWarm
            || self.command != Command::Run
        {
            return Err(Error::SeriesSchema);
        }
        validate_series_samples(&self.samples)?;
        validate_text(
            &self.environment,
            &self.fixture,
            &self.lifecycle,
            self.samples.iter(),
        )?;
        validate_build(&self.build)
    }

    /// Encode stable struct/sequence order as pretty JSON with one final newline.
    pub fn canonical_bytes(&self) -> Result<Box<[u8]>, Error> {
        self.validate()?;
        canonical_bytes(self)
    }
}

fn validate_series_samples(samples: &[Sample]) -> Result<(), Error> {
    let expected = [
        Trial::Warmup { ordinal: 1 },
        Trial::Warmup { ordinal: 2 },
        Trial::Warmup { ordinal: WARMUPS },
        Trial::Measured { ordinal: 1 },
        Trial::Measured { ordinal: 2 },
        Trial::Measured { ordinal: 3 },
        Trial::Measured { ordinal: 4 },
        Trial::Measured { ordinal: MEASURED },
    ];
    if samples.len() != expected.len()
        || samples
            .iter()
            .zip(expected)
            .any(|(sample, trial)| sample.trial != trial)
    {
        return Err(Error::SeriesTrials);
    }
    if !stable_series(samples) {
        return Err(Error::SeriesIdentity);
    }
    if samples.iter().any(|sample| {
        !sample.timing.is_descriptive_baseline()
            || !sample.retirement.is_completed()
            || sample.timing.validate().is_err()
            || sample.retirement.validate().is_err()
    }) {
        return Err(Error::SeriesTiming);
    }
    Ok(())
}

fn validate_build(build: &Build) -> Result<(), Error> {
    let mut bytes = 0_usize;
    for (text, field) in [
        (&build.executable_path, "build.executable-path"),
        (&build.executable_digest_sha256, "build.executable-digest"),
        (&build.source_tree, "build.source-tree"),
        (&build.lockfile_digest_sha256, "build.lockfile-digest"),
        (&build.compiler, "build.compiler"),
        (&build.cargo, "build.cargo"),
        (&build.target, "build.target"),
        (&build.profile, "build.profile"),
    ] {
        add_text(&mut bytes, text, field)?;
    }
    for feature in &build.features {
        add_text(&mut bytes, feature, "build.feature")?;
    }
    if build.features.is_empty()
        || !lower_hex(&build.executable_digest_sha256)
        || !lower_hex(&build.lockfile_digest_sha256)
        || bytes > TEXT_BYTES
    {
        return Err(Error::Text { limit: TEXT_BYTES });
    }
    Ok(())
}

fn lower_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn canonical_bytes(value: &impl serde::Serialize) -> Result<Box<[u8]>, Error> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(Error::Encode)?;
    if bytes.len().saturating_add(1) > REPORT_BYTES {
        return Err(Error::Size {
            limit: REPORT_BYTES,
        });
    }
    bytes.push(b'\n');
    Ok(bytes.into_boxed_slice())
}

fn validate_text<'a>(
    environment: &Environment,
    fixture: &Fixture,
    lifecycle: &str,
    samples: impl IntoIterator<Item = &'a Sample>,
) -> Result<(), Error> {
    let mut bytes = 0_usize;
    add_text(&mut bytes, &environment.machine, "environment.machine")?;
    add_text(
        &mut bytes,
        &environment.operating_system,
        "environment.operating-system",
    )?;
    add_text(
        &mut bytes,
        &environment.architecture,
        "environment.architecture",
    )?;
    add_text(&mut bytes, &environment.cpu, "environment.cpu")?;
    add_text(&mut bytes, &environment.rustc, "environment.rustc")?;
    add_text(&mut bytes, &environment.rustdoc, "environment.rustdoc")?;
    add_text(&mut bytes, &environment.cargo, "environment.cargo")?;
    add_text(&mut bytes, &environment.driver, "environment.driver")?;
    add_text(&mut bytes, &environment.target, "environment.target")?;
    add_text(&mut bytes, &environment.profile, "environment.profile")?;
    add_text(&mut bytes, &fixture.name, "fixture.name")?;
    if fixture.digest_sha256.len() != 64
        || !fixture
            .digest_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Error::FixtureDigest);
    }
    add_text(&mut bytes, &fixture.digest_sha256, "fixture.digest-sha256")?;
    add_text(&mut bytes, lifecycle, "lifecycle")?;
    for sample in samples {
        if !sample.counters.accounting.valid() || sample.counters.accounting.fault.is_some() {
            return Err(Error::Accounting);
        }
        sample.timing.validate()?;
        sample.retirement.validate()?;
        add_observation_reason(&sample.cpu_user_ns, &mut bytes)?;
        add_observation_reason(&sample.cpu_system_ns, &mut bytes)?;
        add_observation_reason(&sample.peak_rss_bytes, &mut bytes)?;
        add_observation_reason(&sample.admitted_ns, &mut bytes)?;
        add_observation_reason(&sample.started_ns, &mut bytes)?;
        if let Some(reason) = sample.timing.reason() {
            add_text(&mut bytes, reason, "timing.reason")?;
        }
        if let Some(reason) = sample.retirement.reason() {
            add_text(&mut bytes, reason, "retirement.reason")?;
        }
        add_observation_reason(&sample.updates_per_second_milli, &mut bytes)?;
        add_observation_reason(&sample.responsiveness_ns, &mut bytes)?;
        add_observation_reason(&sample.rollback, &mut bytes)?;
        add_observation_reason(&sample.last_good_ns, &mut bytes)?;
        add_text(&mut bytes, &sample.cache.preparation, "cache.preparation")?;
        add_text(&mut bytes, &sample.cache.state, "cache.state")?;
        for mutation in &sample.cache.durable_mutations {
            add_text(&mut bytes, mutation, "cache.durable-mutation")?;
        }
        add_identity(&sample.identities.resolution, &mut bytes)?;
        add_identity(&sample.identities.api, &mut bytes)?;
        add_identity(&sample.identities.checked, &mut bytes)?;
        add_identity(&sample.identities.native_product, &mut bytes)?;
        add_identity(&sample.identities.overlay, &mut bytes)?;
        add_identity(&sample.identities.build_request, &mut bytes)?;
        add_identity(&sample.identities.generated_manifest, &mut bytes)?;
        add_identity(&sample.identities.built_artifact, &mut bytes)?;
        add_identity(&sample.identities.rust_owner, &mut bytes)?;
        add_identity(&sample.identities.process_epoch, &mut bytes)?;
    }
    if bytes > TEXT_BYTES {
        return Err(Error::Text { limit: TEXT_BYTES });
    }
    Ok(())
}

fn stable_series(samples: &[Sample]) -> bool {
    let Some(first) = samples.first() else {
        return false;
    };
    samples.iter().all(|sample| {
        sample.identities.build_request == first.identities.build_request
            && sample.identities.generated_manifest == first.identities.generated_manifest
            && sample.identities.built_artifact == first.identities.built_artifact
            && sample.identities.rust_owner == first.identities.rust_owner
            && sample.identities.process_epoch == first.identities.process_epoch
            && sample.counters.nodes == first.counters.nodes
            && sample.counters.processes == first.counters.processes
            && stable_baseline_evidence(&sample.counters, &first.counters)
            && sample.cache.bytes_before == sample.cache.bytes_after
            && sample.cache.state == first.cache.state
            && sample.cache.durable_mutations.is_empty()
    })
}

#[cfg(feature = "import-baseline")]
fn stable_baseline_evidence(
    left: &super::recorder::Snapshot,
    right: &super::recorder::Snapshot,
) -> bool {
    left.profile == right.profile && left.pipeline == right.pipeline
}

#[cfg(not(feature = "import-baseline"))]
fn stable_baseline_evidence(
    _left: &super::recorder::Snapshot,
    _right: &super::recorder::Snapshot,
) -> bool {
    true
}

fn add_text(bytes: &mut usize, text: &str, field: &'static str) -> Result<(), Error> {
    if text.is_empty() {
        return Err(Error::Empty { field });
    }
    *bytes = bytes.saturating_add(text.len());
    Ok(())
}

fn add_observation_reason<T>(observation: &Observation<T>, bytes: &mut usize) -> Result<(), Error> {
    match observation {
        Observation::Measured { .. } => Ok(()),
        Observation::NotApplicable { reason } | Observation::Unavailable { reason } => {
            add_text(bytes, reason, "observation.reason")
        }
    }
}

fn add_identity(observation: &Observation<Box<str>>, bytes: &mut usize) -> Result<(), Error> {
    match observation {
        Observation::Measured { value } => add_text(bytes, value, "identity.value"),
        Observation::NotApplicable { reason } | Observation::Unavailable { reason } => {
            add_text(bytes, reason, "identity.reason")
        }
    }
}

#[cfg(feature = "import-baseline")]
fn cache_consistent(cache: &super::cell::project::Transition) -> bool {
    let changed = !cache.changes.is_empty() || !cache.directory_changes.is_empty();
    (cache.before.state.digest != cache.after.state.digest) == changed
}

#[cfg(test)]
mod tests {
    use super::{Cell, Command, Corpus, Error, Series, Trial};

    #[cfg(feature = "import-baseline")]
    #[test]
    fn cache_consistency_includes_directory_only_changes() {
        use crate::metrics::cell::project;
        let root = std::env::temp_dir().join(format!(
            "rawr-report-directory-union-{}-{}",
            std::process::id(),
            crate::metrics::monotonic_ns().unwrap(),
        ));
        let before = project::Manifest::read(&root).unwrap();
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(root.join("empty")).unwrap();
        let after = project::Manifest::read(&root).unwrap();
        let mut transition = project::Transition::transform(before, after.clone());
        assert!(transition.changes.is_empty());
        assert_eq!(transition.directory_changes.len(), 2);
        assert_eq!(transition.before.state.bytes, transition.after.state.bytes);
        assert!(super::cache_consistent(&transition));
        let mut unchanged_digest = transition.clone();
        unchanged_digest.after.state.digest = unchanged_digest.before.state.digest;
        assert!(!super::cache_consistent(&unchanged_digest));
        transition.directory_changes = Box::default();
        assert!(
            !super::cache_consistent(&transition),
            "a changed digest needs at least one file or directory change"
        );
        assert!(super::cache_consistent(&project::Transition::transform(
            after.clone(),
            after
        )));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recording_fault_is_diagnostic_data_but_cannot_seal_a_baseline_report() {
        let mut sample = sample(Trial::Exact);
        sample.counters.accounting.fault = Some(crate::metrics::accounting::Fault::GraphCapacity);
        assert!(matches!(
            Cell::transform(
                super::Scenario::CleanCold,
                Command::Check,
                "check".into(),
                environment(),
                fixture(),
                sample,
            ),
            Err(Error::Accounting)
        ));
    }

    #[test]
    fn deterministic_cell_rejects_sampled_trials() {
        let mut sample = sample(Trial::Warmup { ordinal: 1 });
        let error = Cell::transform(
            super::Scenario::CleanCold,
            Command::Check,
            "check".into(),
            environment(),
            fixture(),
            sample.clone(),
        )
        .expect_err("warmup cannot masquerade as exact evidence");
        assert!(matches!(error, Error::CellTrial));

        sample.trial = Trial::Exact;
        let cell = Cell::transform(
            super::Scenario::CleanCold,
            Command::Check,
            "check".into(),
            environment(),
            fixture(),
            sample,
        )
        .expect("exact cell seals");
        assert!(cell
            .canonical_bytes()
            .expect("cell encoding succeeds")
            .ends_with(b"\n"));
    }

    #[test]
    fn warm_run_series_requires_the_explicit_three_plus_five_sequence() {
        let trials = [
            Trial::Warmup { ordinal: 1 },
            Trial::Warmup { ordinal: 2 },
            Trial::Warmup { ordinal: 3 },
            Trial::Measured { ordinal: 1 },
            Trial::Measured { ordinal: 2 },
            Trial::Measured { ordinal: 3 },
            Trial::Measured { ordinal: 4 },
            Trial::Measured { ordinal: 5 },
        ];
        let series = Series::transform(
            "run".into(),
            environment(),
            fixture(),
            build(),
            trials.map(sample).into(),
        )
        .expect("the sampling sequence seals");
        assert_eq!(
            series.canonical_bytes().expect("first encoding succeeds"),
            series.canonical_bytes().expect("second encoding succeeds")
        );
        let relabeled = String::from_utf8(
            series
                .canonical_bytes()
                .expect("descriptive series encodes")
                .into_vec(),
        )
        .expect("canonical JSON is UTF-8")
        .replace("descriptive-baseline", "release-oracle");
        let relabeled: Series =
            serde_json::from_str(&relabeled).expect("report bytes remain inert data");
        assert!(matches!(
            relabeled.canonical_bytes(),
            Err(Error::SeriesTiming)
        ));

        let mut wrong = trials.map(sample);
        wrong[0].trial = Trial::Exact;
        assert!(matches!(
            Series::transform(
                "run".into(),
                environment(),
                fixture(),
                build(),
                wrong.into()
            ),
            Err(Error::SeriesTrials)
        ));

        #[cfg(feature = "import-baseline")]
        {
            let mut mismatched = trials.map(sample);
            for sample in &mut mismatched {
                sample.counters.pipeline = Some(crate::metrics::pipeline::Evidence {
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
            }
            mismatched[7]
                .counters
                .pipeline
                .as_mut()
                .expect("pipeline evidence exists")
                .native_inputs = [99; 32];
            assert!(matches!(
                Series::transform(
                    "run".into(),
                    environment(),
                    fixture(),
                    build(),
                    mismatched.into()
                ),
                Err(Error::SeriesIdentity)
            ));
        }
    }

    #[test]
    fn all_eleven_scenario_rows_remain_closed_and_partitioned() {
        assert_eq!(super::Scenario::CURRENT.len(), 8);
        assert_eq!(super::Scenario::AFTER_CUTOVER.len(), 3);
        assert!(super::Scenario::CURRENT
            .iter()
            .all(|scenario| !super::Scenario::AFTER_CUTOVER.contains(scenario)));
    }

    #[test]
    fn corpus_rejects_a_missing_profile_cell_instead_of_fabricating_it() {
        let cells = super::Scenario::CURRENT
            .into_iter()
            .filter(|scenario| *scenario != super::Scenario::ProfileRule)
            .flat_map(|scenario| {
                Command::ALL.map(move |command| {
                    Cell::transform(
                        scenario,
                        command,
                        format!("{command:?}").into(),
                        environment(),
                        fixture(),
                        sample(Trial::Exact),
                    )
                    .expect("exact fixture cell seals")
                })
            })
            .collect::<Vec<_>>();
        assert!(matches!(
            Corpus::transform(cells.into_boxed_slice()),
            Err(Error::CorpusCells)
        ));
    }

    #[test]
    fn typed_unavailable_observations_require_a_nonempty_reason() {
        let mut sample = sample(Trial::Exact);
        sample.timing = super::timing::Timing::unavailable(Box::<str>::default());
        assert!(matches!(
            Cell::transform(
                super::Scenario::CleanCold,
                Command::Run,
                "run".into(),
                environment(),
                fixture(),
                sample,
            ),
            Err(Error::Empty {
                field: "timing.reason"
            })
        ));
    }

    fn sample(trial: Trial) -> super::Sample {
        use super::{Cache, Identities, Observation, Sample};
        use crate::metrics::recorder::Recorder;

        Sample {
            trial,
            wall_ns: 1,
            cpu_user_ns: Observation::Unavailable {
                reason: "fixture observer".into(),
            },
            cpu_system_ns: Observation::Unavailable {
                reason: "fixture observer".into(),
            },
            peak_rss_bytes: Observation::Unavailable {
                reason: "fixture observer".into(),
            },
            admitted_ns: not_applicable(),
            started_ns: not_applicable(),
            timing: super::timing::Timing::descriptive_baseline(1, 2)
                .expect("descriptive timing seals"),
            retirement: super::timing::Retirement::completed(2, 3).expect("retirement seals"),
            updates_per_second_milli: not_applicable(),
            responsiveness_ns: not_applicable(),
            rollback: not_applicable(),
            last_good_ns: not_applicable(),
            counters: Recorder::transform(1)
                .expect("recorder seals")
                .snapshot()
                .expect("empty counters snapshot"),
            cache: Cache {
                preparation: "fixture preparation".into(),
                state: "fixture state".into(),
                bytes_before: 0,
                bytes_after: 0,
                durable_mutations: Box::default(),
            },
            identities: Identities {
                resolution: not_applicable(),
                api: not_applicable(),
                checked: not_applicable(),
                native_product: not_applicable(),
                overlay: not_applicable(),
                build_request: measured("build-request"),
                generated_manifest: measured("generated-manifest"),
                built_artifact: measured("built-artifact"),
                rust_owner: measured("rust-owner"),
                process_epoch: measured("1"),
            },
        }
    }

    fn measured(value: &str) -> super::Observation<Box<str>> {
        super::Observation::Measured {
            value: value.into(),
        }
    }

    fn not_applicable<T>() -> super::Observation<T> {
        super::Observation::NotApplicable {
            reason: "not meaningful for fixture owner".into(),
        }
    }

    fn environment() -> super::Environment {
        super::Environment {
            machine: "fixture".into(),
            operating_system: "fixture".into(),
            architecture: "fixture".into(),
            cpu: "fixture".into(),
            rustc: "fixture".into(),
            rustdoc: "fixture".into(),
            cargo: "fixture".into(),
            driver: "fixture".into(),
            target: "fixture".into(),
            profile: "fixture".into(),
        }
    }

    fn fixture() -> super::Fixture {
        super::Fixture {
            name: "fixture".into(),
            digest_sha256: "00".repeat(32).into_boxed_str(),
            package_list_fallbacks: 0,
        }
    }

    fn build() -> super::Build {
        super::Build {
            executable_path: "/fixture/rawr".into(),
            executable_digest_sha256: "11".repeat(32).into_boxed_str(),
            device: 1,
            inode: 2,
            size: 3,
            source_tree: "/fixture".into(),
            lockfile_digest_sha256: "22".repeat(32).into_boxed_str(),
            compiler: "rustc fixture".into(),
            cargo: "cargo fixture".into(),
            target: "fixture-target".into(),
            profile: "release".into(),
            features: vec![Box::<str>::from("import-baseline")].into_boxed_slice(),
        }
    }
}
