//! One parent-owned 55-cell observation manifest, retaining complete row documents.

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::{Read as _, Write as _};
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};

use rawr_import::metrics::{cell, evidence, report};
use sha2::{Digest as _, Sha256};

use super::{corruption, paired, provenance, Assets, Project};

#[derive(serde::Serialize)]
struct Context {
    corpus: [u8; 32],
    fixture: report::Fixture,
    fixture_digest: [u8; 32],
    environment: report::Environment,
    build: cell::Build,
    provenance: report::Build,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, serde::Serialize)]
struct Key {
    scenario: report::Scenario,
    command: report::Command,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
struct Location {
    path: Box<[u8]>,
    device: u64,
    inode: u64,
}

#[derive(serde::Serialize)]
struct Current {
    key: Key,
    project: Location,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
enum Kind {
    Ordinary,
    Corruption,
    Paired,
}

enum Observed {
    Ordinary(report::Cell),
    Corruption(corruption::Observed),
    Paired(paired::Observed),
}

#[derive(Clone, serde::Serialize)]
struct Execution {
    request: cell::Request,
    capsule_digest: [u8; 32],
}

struct Facts<'a> {
    fixture: &'a report::Fixture,
    environment: &'a report::Environment,
    executions: Vec<Execution>,
}

#[derive(serde::Serialize)]
struct Document {
    name: Box<str>,
    bytes: u64,
    digest: [u8; 32],
}

/// Only live typed observations can produce an entry; JSON is never admission input.
#[derive(serde::Serialize)]
struct Entry {
    key: Key,
    kind: Kind,
    context_digest: [u8; 32],
    project: Location,
    executions: Vec<Execution>,
    document: Document,
}

struct Attempt {
    context: Context,
    directory: PathBuf,
    entries: Vec<Entry>,
    current: Option<Current>,
}

#[derive(serde::Serialize)]
struct Complete<'a> {
    schema: &'static str,
    context: &'a Context,
    entries: &'a [Entry],
    qualification: report::Observation<()>,
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error(transparent)]
    Owner(#[from] super::Error),
    #[error(transparent)]
    Corruption(#[from] corruption::Error),
    #[error(transparent)]
    Paired(#[from] paired::Error),
    #[error(transparent)]
    Protocol(#[from] cell::Error),
    #[error(transparent)]
    Provenance(#[from] provenance::Error),
    #[error(transparent)]
    Evidence(#[from] evidence::Error),
    #[error(transparent)]
    Report(#[from] report::Error),
    #[error("complete-corpus observation is inconsistent: {0}")]
    Binding(&'static str),
    #[error("complete-corpus document I/O failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{source}")]
    Observed {
        #[source]
        source: Box<Error>,
        bytes: Box<[u8]>,
    },
    #[error(
        "{source}; incomplete attempt retained at {directory}; evidence persistence: {persistence}"
    )]
    Failed {
        #[source]
        source: Box<Error>,
        directory: PathBuf,
        persistence: Box<str>,
    },
}

impl Context {
    fn digest(&self) -> Result<[u8; 32], Error> {
        Ok(Sha256::digest(corruption::encode(self)?).into())
    }
}

impl Location {
    fn read(project: &Project) -> Result<Self, Error> {
        let metadata =
            fs::symlink_metadata(&project.root).map_err(|source| io(&project.root, source))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(Error::Binding("owned project root changed type"));
        }
        Ok(Self {
            path: project.root.as_os_str().as_bytes().into(),
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }

    fn matches(&self, project: &cell::project::Identity) -> bool {
        self.path == project.path && self.device == project.device && self.inode == project.inode
    }
}

impl Execution {
    fn setup(capsule: &cell::SetupCapsule) -> Result<Self, Error> {
        if !matches!(&capsule.outcome, cell::Outcome::Success { .. })
            || capsule.request.core.role != cell::Role::Setup
            || capsule.executable != capsule.request.core.build.executable
        {
            return Err(Error::Binding(
                "setup capsule outcome, role, or executable binding",
            ));
        }
        Ok(Self {
            request: capsule.request.clone(),
            capsule_digest: capsule.digest()?,
        })
    }

    fn retained(capsule: &cell::CellCapsule) -> Result<Self, Error> {
        if !matches!(&capsule.outcome, cell::Outcome::Success { .. })
            || capsule.request.core.role != cell::Role::RetainedCell
            || capsule.executable != capsule.request.core.build.executable
        {
            return Err(Error::Binding(
                "retained capsule outcome, role, or executable binding",
            ));
        }
        Ok(Self {
            request: capsule.request.clone(),
            capsule_digest: capsule.digest()?,
        })
    }
}

impl Observed {
    fn kind(&self) -> Kind {
        match self {
            Self::Ordinary(_) => Kind::Ordinary,
            Self::Corruption(_) => Kind::Corruption,
            Self::Paired(_) => Kind::Paired,
        }
    }

    fn facts(&self) -> Result<Facts<'_>, Error> {
        match self {
            Self::Ordinary(value) => single(value),
            Self::Corruption(value) => single(value.cell()),
            Self::Paired(value) => {
                let capsules = value.capsules()?;
                Ok(Facts {
                    fixture: value.fixture(),
                    environment: value.environment(),
                    executions: capsules
                        .into_iter()
                        .map(Execution::retained)
                        .collect::<Result<_, _>>()?,
                })
            }
        }
    }

    fn bytes(&self) -> Result<Box<[u8]>, Error> {
        match self {
            Self::Ordinary(value) => Ok(value.canonical_bytes()?),
            Self::Corruption(value) => Ok(corruption::encode(value)?),
            Self::Paired(value) => Ok(corruption::encode(value)?),
        }
    }
}

fn single(value: &report::Cell) -> Result<Facts<'_>, Error> {
    let retained = value
        .invocation
        .as_ref()
        .ok_or(Error::Binding("single cell omitted its actual invocation"))?;
    if retained.capsule.request.core.scenario != value.scenario
        || retained.capsule.request.core.owner != value.command
        || retained.capsule.digest()? != retained.capsule_digest
    {
        return Err(Error::Binding("single cell header or capsule link changed"));
    }
    let mut executions = Vec::new();
    if let Some(setup) = &value.preparation {
        if setup.capsule.digest()? != setup.capsule_digest {
            return Err(Error::Binding("setup capsule link changed"));
        }
        executions.push(Execution::setup(&setup.capsule)?);
    }
    executions.push(Execution::retained(&retained.capsule)?);
    Ok(Facts {
        fixture: &value.fixture,
        environment: &value.environment,
        executions,
    })
}

impl Document {
    fn bind(key: Key, bytes: &[u8]) -> Self {
        Self {
            name: format!("{:?}-{:?}.json", key.scenario, key.command)
                .to_lowercase()
                .into_boxed_str(),
            bytes: bytes.len() as u64,
            digest: Sha256::digest(bytes).into(),
        }
    }

    fn verify(&self, directory: &Path) -> Result<(), Error> {
        let path = directory.join(self.name.as_ref());
        let metadata = fs::symlink_metadata(&path).map_err(|source| io(&path, source))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() != self.bytes
        {
            return Err(Error::Binding("row document type or length changed"));
        }
        let file = fs::File::open(&path).map_err(|source| io(&path, source))?;
        let opened = file.metadata().map_err(|source| io(&path, source))?;
        if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() {
            return Err(Error::Binding("row document changed while opening"));
        }
        let mut bytes = Vec::new();
        file.take(self.bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|source| io(&path, source))?;
        if bytes.len() as u64 != self.bytes
            || <[u8; 32]>::from(Sha256::digest(&bytes)) != self.digest
        {
            return Err(Error::Binding("row document bytes changed"));
        }
        Ok(())
    }
}

impl Entry {
    fn transform(
        context: &Context,
        current: &Current,
        kind: Kind,
        facts: Facts<'_>,
        document: Document,
    ) -> Result<Self, Error> {
        let scenario = current.key.scenario;
        let expected_kind = match scenario {
            report::Scenario::CorruptRunner | report::Scenario::CorruptResolution => {
                Kind::Corruption
            }
            report::Scenario::ConcurrentCold => Kind::Paired,
            _ => Kind::Ordinary,
        };
        let roles: &[cell::Role] = match scenario {
            report::Scenario::CleanCold => &[cell::Role::RetainedCell],
            report::Scenario::ConcurrentCold => {
                &[cell::Role::RetainedCell, cell::Role::RetainedCell]
            }
            _ => &[cell::Role::Setup, cell::Role::RetainedCell],
        };
        if kind != expected_kind
            || facts.fixture != &context.fixture
            || facts.environment != &context.environment
            || facts.executions.len() != roles.len()
        {
            return Err(Error::Binding(
                "row kind, context, or execution cardinality",
            ));
        }
        let mut launches = BTreeSet::new();
        for (execution, role) in facts.executions.iter().zip(roles) {
            execution.request.validate()?;
            let request = &execution.request.core;
            let profile = match (scenario, *role) {
                (report::Scenario::ProfileRule, cell::Role::Setup) => cell::Profile::Baseline,
                (report::Scenario::ProfileRule, cell::Role::RetainedCell) => cell::Profile::Variant,
                _ => cell::Profile::Inactive,
            };
            if request.corpus_id != context.corpus
                || request.build != context.build
                || request.fixture_digest != context.fixture_digest
                || request.scenario != scenario
                || request.owner != current.key.command
                || request.role != *role
                || request.profile != profile
                || !current.project.matches(&request.project)
                || !launches.insert(request.launch_id)
            {
                return Err(Error::Binding(
                    "actual request differs from the common parent context",
                ));
            }
        }
        if kind == Kind::Paired
            && facts.executions[0].request.core.project != facts.executions[1].request.core.project
        {
            return Err(Error::Binding(
                "paired executions lost their common complete prestate",
            ));
        }
        Ok(Self {
            key: current.key,
            kind,
            context_digest: context.digest()?,
            project: current.project.clone(),
            executions: facts.executions,
            document,
        })
    }
}

impl Attempt {
    fn transform(context: Context, parent: &Path) -> Result<Self, Error> {
        fs::create_dir_all(parent).map_err(|source| io(parent, source))?;
        let directory = parent.join(evidence::hex(&context.corpus));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .map_err(|source| io(&directory, source))?;
        Ok(Self {
            context,
            directory,
            entries: Vec::new(),
            current: None,
        })
    }

    fn record(
        &mut self,
        key: Key,
        project: &mut Project,
        assets: &Assets,
        executable: &Path,
        inputs: &corruption::seed::Inputs,
    ) -> Result<(), Error> {
        project.preserve = true;
        self.current = Some(Current {
            key,
            project: Location::read(project)?,
        });
        let current = self
            .current
            .as_ref()
            .ok_or(Error::Binding("current row disappeared"))?;
        extend(&self.entries, current)?;
        let observed = match key.scenario {
            report::Scenario::CorruptRunner | report::Scenario::CorruptResolution => {
                Observed::Corruption(corruption::observe(
                    assets,
                    project,
                    executable,
                    inputs,
                    self.context.corpus,
                    key.scenario,
                    key.command,
                    self.context.environment.clone(),
                )?)
            }
            report::Scenario::ConcurrentCold => Observed::Paired(paired::observe(
                assets,
                project,
                executable,
                inputs,
                self.context.corpus,
                key.command,
                self.context.environment.clone(),
            )?),
            _ => Observed::Ordinary(super::observe(
                assets,
                project,
                executable,
                self.context.corpus,
                key.scenario,
                key.command,
                self.context.environment.clone(),
            )?),
        };
        let bytes = observed.bytes()?;
        let result = (|| {
            let entry = Entry::transform(
                &self.context,
                current,
                observed.kind(),
                observed.facts()?,
                Document::bind(key, &bytes),
            )?;
            let prior_launches: BTreeSet<_> = self
                .entries
                .iter()
                .flat_map(|entry| {
                    entry
                        .executions
                        .iter()
                        .map(|execution| execution.request.core.launch_id)
                })
                .collect();
            if entry
                .executions
                .iter()
                .any(|execution| prior_launches.contains(&execution.request.core.launch_id))
            {
                return Err(Error::Binding("a prior launch was reused by another row"));
            }
            write(&self.directory.join(entry.document.name.as_ref()), &bytes)?;
            entry.document.verify(&self.directory)?;
            Ok(entry)
        })();
        let entry = result.map_err(|source| Error::Observed {
            source: Box::new(source),
            bytes,
        })?;
        self.entries.push(entry);
        self.current = None;
        // A later row marks its project preserved again before any owner work.
        project.preserve = false;
        Ok(())
    }

    fn finish(&self) -> Result<PathBuf, Error> {
        let complete = Complete::transform(&self.context, &self.entries)?;
        for entry in &self.entries {
            entry.document.verify(&self.directory)?;
        }
        let path = self.directory.join("manifest.json");
        write(&path, &corruption::encode(&complete)?)?;
        Ok(path)
    }

    fn failed(&self, source: Error) -> Error {
        #[derive(serde::Serialize)]
        struct Reached<'a> {
            schema: &'static str,
            context: &'a Context,
            entries: &'a [Entry],
            current: &'a Option<Current>,
            primary: String,
            causes: Vec<String>,
        }
        let mut persistence = Vec::new();
        let detail = match &source {
            Error::Owner(error) => corruption::owner_failure_bytes(error),
            Error::Corruption(error) => corruption::failure_bytes(error),
            Error::Paired(error) => paired::failure_bytes(error),
            _ => corruption::encode(&diagnostic(&source)),
        };
        persistence.push(match detail {
            Ok(bytes) => {
                write(&self.directory.join("failure.json"), &bytes).map_err(|e| e.to_string())
            }
            Err(error) => Err(error.to_string()),
        });
        if let Error::Observed { bytes, .. } = &source {
            persistence.push(
                write(&self.directory.join("rejected-row.json"), bytes).map_err(|e| e.to_string()),
            );
        }
        let (primary, causes) = diagnostic(&source);
        let reached = Reached {
            schema: "rawr.phase9.complete-corpus.incomplete/v1",
            context: &self.context,
            entries: &self.entries,
            current: &self.current,
            primary,
            causes,
        };
        persistence.push(match corruption::encode(&reached) {
            Ok(bytes) => {
                write(&self.directory.join("incomplete.json"), &bytes).map_err(|e| e.to_string())
            }
            Err(error) => Err(error.to_string()),
        });
        Error::Failed {
            source: Box::new(source),
            directory: self.directory.clone(),
            persistence: format!("{persistence:?}").into_boxed_str(),
        }
    }
}

fn extend(entries: &[Entry], current: &Current) -> Result<(), Error> {
    for entry in entries {
        if entry.key == current.key {
            return Err(Error::Binding("duplicate scenario/owner row"));
        }
        let same_lane = entry.kind == Kind::Ordinary
            && report::Scenario::CURRENT.contains(&current.key.scenario)
            && entry.key.command == current.key.command;
        if same_lane && entry.project != current.project {
            return Err(Error::Binding("ordinary owner lane changed project"));
        }
        if !same_lane && entry.project.path == current.project.path {
            return Err(Error::Binding(
                "distinct owner lane or isolated row reused a project",
            ));
        }
    }
    Ok(())
}

impl<'a> Complete<'a> {
    fn transform(context: &'a Context, entries: &'a [Entry]) -> Result<Self, Error> {
        let expected: BTreeSet<_> = report::Scenario::CURRENT
            .into_iter()
            .chain(report::Scenario::AFTER_CUTOVER)
            .flat_map(|scenario| report::Command::ALL.map(|command| Key { scenario, command }))
            .collect();
        let actual: BTreeSet<_> = entries.iter().map(|entry| entry.key).collect();
        if entries.len() != expected.len() || actual != expected {
            return Err(Error::Binding(
                "exact 55-cell coverage is incomplete or duplicated",
            ));
        }
        let context_digest = context.digest()?;
        let mut launches = BTreeSet::new();
        for (index, entry) in entries.iter().enumerate() {
            if entry.context_digest != context_digest
                || entry
                    .executions
                    .iter()
                    .any(|execution| !launches.insert(execution.request.core.launch_id))
            {
                return Err(Error::Binding(
                    "complete corpus mixed parent contexts or reused a launch",
                ));
            }
            extend(
                &entries[..index],
                &Current {
                    key: entry.key,
                    project: entry.project.clone(),
                },
            )?;
        }
        Ok(Self {
            schema: "rawr.phase9.complete-corpus.observation/v1", context, entries,
            qualification: report::Observation::Unavailable {
                reason: "complete scenario coverage retains each row's explicit unknowns; it does not establish durable-write closure, ordinary controls, or release qualification".into(),
            },
        })
    }
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|source| io(path, source))?;
    file.write_all(bytes).map_err(|source| io(path, source))?;
    file.sync_all().map_err(|source| io(path, source))
}

fn diagnostic(error: &dyn std::error::Error) -> (String, Vec<String>) {
    let mut causes = Vec::new();
    let mut cause = error.source();
    while let Some(error) = cause {
        causes.push(error.to_string());
        cause = error.source();
    }
    (error.to_string(), causes)
}

fn io(path: &Path, source: std::io::Error) -> Error {
    Error::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn run() -> Result<PathBuf, Error> {
    let assets = Assets::transform()?;
    let executable = super::release_artifact("evidence")?;
    let provenance = provenance::load(&executable, provenance::Kind::Evidence)?;
    let fixture = assets.fixture()?;
    let context = Context {
        corpus: cell::corpus_id()?,
        fixture_digest: evidence::parse_digest(std::ffi::OsStr::new(
            fixture.digest_sha256.as_ref(),
        ))?,
        fixture,
        environment: super::environment()?,
        build: provenance.cell(&executable)?,
        provenance: provenance.report()?,
    };
    let inputs = corruption::seed::Inputs {
        registry: std::env::var_os("RAWR_BASELINE_REGISTRY_SEED")
            .map(PathBuf::from)
            .ok_or(Error::Binding("explicit registry seed root required"))?,
        git: std::env::var_os("RAWR_BASELINE_GIT_SEED")
            .map(PathBuf::from)
            .ok_or(Error::Binding("explicit git seed root required"))?,
    };
    let parent =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/phase9-08/complete-corpus");
    let mut attempt = Attempt::transform(context, &parent)?;
    let result = (|| {
        for command in report::Command::ALL {
            let mut project = assets.materialize()?;
            for scenario in report::Scenario::CURRENT {
                attempt.record(
                    Key { scenario, command },
                    &mut project,
                    &assets,
                    &executable,
                    &inputs,
                )?;
            }
            for scenario in report::Scenario::AFTER_CUTOVER {
                let mut isolated = assets.materialize()?;
                attempt.record(
                    Key { scenario, command },
                    &mut isolated,
                    &assets,
                    &executable,
                    &inputs,
                )?;
            }
        }
        attempt.finish()
    })();
    result.map_err(|source| attempt.failed(source))
}

#[test]
#[ignore = "complete 55-cell observation; explicit isolated registry/git inputs; operator only, no qualification"]
fn record_complete_release_corpus() {
    run().unwrap_or_else(|error| panic!("complete corpus failed without retry: {error}"));
}

#[cfg(test)]
mod tests;
