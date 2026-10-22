//! Authenticated one-shot deterministic-owner cell evidence.

use std::fs;
use std::io::Read as _;
use std::os::unix::fs::{FileTypeExt as _, PermissionsExt as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::evidence::process::file;
use super::recorder::Snapshot;
use super::report::{Command, Scenario};

mod encode;
mod frame;
pub mod project;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const OWNER_TIMEOUT: Duration = Duration::from_secs(30 * 60);
pub(crate) const REPORT_BYTES: usize = 8 * 1024 * 1024;
const TEXT_BYTES: usize = 16 * 1024;
const REQUEST_SCHEMA: &str = "rawr.phase9.cell.request/v1";
const SETUP_SCHEMA: &str = "rawr.phase9.cell.setup-capsule/v3";
const CELL_SCHEMA: &str = "rawr.phase9.cell.retained-capsule/v3";
static SOCKET_ID: AtomicU64 = AtomicU64::new(1);

/// Closed preparation or retained-cell role.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    Setup,
    RetainedCell,
}

/// Closed profile-fixture selection transported independently from CLI grammar.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Profile {
    Inactive,
    Baseline,
    Variant,
}

/// Fixed per-child evidence bounds. Production limits remain independently exact.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Bounds {
    pub counter_limit: u64,
    pub owner_deadline_ns: u64,
    pub capsule_bytes: u64,
}

/// Feature/source/lock and exact normal-`rawr` executable identity.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Build {
    pub executable: file::Identity,
    pub source_tree: Box<str>,
    pub lockfile_digest: [u8; 32],
    pub features_digest: [u8; 32],
}

/// Complete parent-owned request facts before one child exists.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct RequestCore {
    schema: Box<str>,
    pub corpus_id: [u8; 32],
    pub launch_id: [u8; 32],
    pub launched_ns: u64,
    pub build: Build,
    pub project: project::Identity,
    pub fixture_digest: [u8; 32],
    pub owner: Command,
    pub scenario: Scenario,
    pub role: Role,
    pub profile: Profile,
    pub ordinal: u8,
    pub bounds: Bounds,
}

/// Canonical request plus its independently checked digest.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Request {
    pub core: RequestCore,
    pub request_digest: [u8; 32],
}

/// Exact typed production-owner result.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum Outcome {
    Success { generation: [u8; 32] },
    Failure { reason: Box<str> },
}

/// Direct peer PID/UID retained in a deterministic capsule.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Peer {
    pub pid: u32,
    pub uid: u32,
}

/// One setup execution, never a retained cell.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct SetupCapsule {
    schema: Box<str>,
    pub request: Request,
    pub child: Peer,
    pub executable: file::Identity,
    pub owner_entry_ns: u64,
    pub owner_return_ns: u64,
    pub counters: Snapshot,
    pub outcome: Outcome,
}

/// One retained deterministic cell execution.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct CellCapsule {
    schema: Box<str>,
    pub request: Request,
    pub child: Peer,
    pub executable: file::Identity,
    pub owner_entry_ns: u64,
    pub owner_return_ns: u64,
    pub counters: Snapshot,
    pub outcome: Outcome,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
struct SetupEnvelope {
    payload: SetupCapsule,
    payload_digest: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
struct CellEnvelope {
    payload: CellCapsule,
    payload_digest: [u8; 32],
}

/// The role-specific capsule admitted by the parent.
pub enum Capsule {
    Setup(SetupCapsule),
    Retained(CellCapsule),
}

impl Capsule {
    /// Retain an already authenticated diagnostic within the existing report bound.
    /// This serialization does not turn a failed owner into an admitted product.
    pub fn canonical_bytes(&self) -> Result<Box<[u8]>, Error> {
        match self {
            Self::Setup(capsule) => canonical(capsule),
            Self::Retained(capsule) => canonical(capsule),
        }
    }
}

/// Exclusive mode-0700 directory and mode-0600 cell listener.
pub struct Listening {
    root: PathBuf,
    socket: PathBuf,
    listener: UnixListener,
    launch_id: [u8; 32],
}

/// Parent side after exact child peer authentication.
pub struct Parent {
    receiver: Receiver,
    closure: Closure,
}

/// Exact authenticated child awaiting the existing request and write-half EOF.
/// This parent-only state adds no child handshake or readiness authority.
pub struct Pending {
    stream: UnixStream,
    request: Request,
    child: Peer,
}

/// Cancellable parent-side request dispatch and final reception for one real child.
pub struct Exchange {
    pending: Pending,
    deadline: Instant,
}

/// Consuming parent-side reader for exactly one role-specific final capsule.
pub struct Receiver {
    stream: UnixStream,
    request: Request,
    child: Peer,
}

/// A rejected reception retains decoded payload bytes without admitting them.
#[derive(Debug, thiserror::Error)]
#[error("{source}")]
pub struct ReceptionFailure {
    #[source]
    pub source: Error,
    pub canonical_capsule: Option<Box<[u8]>>,
}

/// Parent-owned failure closure for the otherwise observation-only capability.
pub struct Closure {
    stream: UnixStream,
}

/// Child side after exact parent, request, executable, fixture, and project validation.
pub struct Session {
    stream: UnixStream,
    request: Request,
    child: Peer,
    executable: file::Identity,
}

impl Bounds {
    #[must_use]
    pub const fn standard() -> Self {
        Self {
            counter_limit: 1_000_000,
            owner_deadline_ns: 30 * 60 * 1_000_000_000,
            capsule_bytes: REPORT_BYTES as u64,
        }
    }
}

impl Build {
    pub fn transform(
        executable: &Path,
        source_tree: impl Into<Box<str>>,
        lockfile_digest: [u8; 32],
    ) -> Result<Self, Error> {
        let source_tree = source_tree.into();
        let build = Self {
            executable: file::Identity::read(executable).map_err(Error::Evidence)?,
            source_tree,
            lockfile_digest,
            features_digest: features_digest(),
        };
        build.validate()?;
        Ok(build)
    }

    fn validate(&self) -> Result<(), Error> {
        if self.source_tree.is_empty()
            || self.source_tree.len() > TEXT_BYTES
            || self.lockfile_digest == [0; 32]
            || self.features_digest != features_digest()
            || self.executable.digest == [0; 32]
            || self.executable.path.is_empty()
        {
            return Err(Error::Build);
        }
        Ok(())
    }
}

impl Request {
    #[allow(clippy::too_many_arguments)]
    pub fn transform(
        corpus_id: [u8; 32],
        launch_id: [u8; 32],
        launched_ns: u64,
        build: Build,
        project: project::Identity,
        fixture_digest: [u8; 32],
        owner: Command,
        scenario: Scenario,
        role: Role,
        profile: Profile,
        ordinal: u8,
    ) -> Result<Self, Error> {
        let core = RequestCore {
            schema: REQUEST_SCHEMA.into(),
            corpus_id,
            launch_id,
            launched_ns,
            build,
            project,
            fixture_digest,
            owner,
            scenario,
            role,
            profile,
            ordinal,
            bounds: Bounds::standard(),
        };
        validate_core(&core)?;
        let request_digest = digest(b"rawr.phase9.cell.request-digest/v1", &core)?;
        Ok(Self {
            core,
            request_digest,
        })
    }

    pub fn validate(&self) -> Result<(), Error> {
        validate_core(&self.core)?;
        if self.request_digest != digest(b"rawr.phase9.cell.request-digest/v1", &self.core)? {
            return Err(Error::Request("digest"));
        }
        Ok(())
    }
}

impl Outcome {
    fn validate(&self) -> Result<(), Error> {
        match self {
            Self::Success { generation } if *generation != [0; 32] => Ok(()),
            Self::Failure { reason } if !reason.is_empty() && reason.len() <= TEXT_BYTES => Ok(()),
            Self::Success { .. } | Self::Failure { .. } => Err(Error::Outcome),
        }
    }
}

impl SetupCapsule {
    /// Hash-link this exact canonical setup payload into a parent report.
    pub fn digest(&self) -> Result<[u8; 32], Error> {
        digest(b"rawr.phase9.cell.setup-report-link/v3", self)
    }

    fn transform(
        session: &Session,
        owner_entry_ns: u64,
        owner_return_ns: u64,
        counters: Snapshot,
        outcome: Outcome,
    ) -> Result<Self, Error> {
        let capsule = Self {
            schema: SETUP_SCHEMA.into(),
            request: session.request.clone(),
            child: session.child,
            executable: session.executable.clone(),
            owner_entry_ns,
            owner_return_ns,
            counters,
            outcome,
        };
        capsule.validate(&session.request, session.child)?;
        Ok(capsule)
    }

    fn validate(&self, request: &Request, child: Peer) -> Result<(), Error> {
        validate_capsule(
            self.schema.as_ref() == SETUP_SCHEMA && self.request.core.role == Role::Setup,
            &self.request,
            request,
            self.child,
            child,
            &self.executable,
            self.owner_entry_ns,
            self.owner_return_ns,
            &self.counters,
            &self.outcome,
        )
    }
}

impl CellCapsule {
    /// Hash-link this exact canonical retained payload into a parent report.
    pub fn digest(&self) -> Result<[u8; 32], Error> {
        digest(b"rawr.phase9.cell.retained-report-link/v3", self)
    }

    fn transform(
        session: &Session,
        owner_entry_ns: u64,
        owner_return_ns: u64,
        counters: Snapshot,
        outcome: Outcome,
    ) -> Result<Self, Error> {
        let capsule = Self {
            schema: CELL_SCHEMA.into(),
            request: session.request.clone(),
            child: session.child,
            executable: session.executable.clone(),
            owner_entry_ns,
            owner_return_ns,
            counters,
            outcome,
        };
        capsule.validate(&session.request, session.child)?;
        Ok(capsule)
    }

    fn validate(&self, request: &Request, child: Peer) -> Result<(), Error> {
        validate_capsule(
            self.schema.as_ref() == CELL_SCHEMA && self.request.core.role == Role::RetainedCell,
            &self.request,
            request,
            self.child,
            child,
            &self.executable,
            self.owner_entry_ns,
            self.owner_return_ns,
            &self.counters,
            &self.outcome,
        )
    }
}

impl SetupEnvelope {
    fn seal(payload: SetupCapsule) -> Result<Self, Error> {
        let payload_digest = digest(b"rawr.phase9.cell.setup-payload/v3", &payload)?;
        Ok(Self {
            payload,
            payload_digest,
        })
    }

    fn open(self) -> Result<SetupCapsule, Error> {
        if self.payload_digest != digest(b"rawr.phase9.cell.setup-payload/v3", &self.payload)? {
            return Err(Error::Digest);
        }
        Ok(self.payload)
    }
}

impl CellEnvelope {
    fn seal(payload: CellCapsule) -> Result<Self, Error> {
        let payload_digest = digest(b"rawr.phase9.cell.retained-payload/v3", &payload)?;
        Ok(Self {
            payload,
            payload_digest,
        })
    }

    fn open(self) -> Result<CellCapsule, Error> {
        if self.payload_digest != digest(b"rawr.phase9.cell.retained-payload/v3", &self.payload)? {
            return Err(Error::Digest);
        }
        Ok(self.payload)
    }
}

impl Listening {
    pub fn transform() -> Result<Self, Error> {
        let base = fs::canonicalize("/tmp").map_err(Error::Io)?;
        for _ in 0..16 {
            let launch_id = random()?;
            let root = base.join(format!(
                "rawr-baseline-cell-{}-{}-{}",
                std::process::id(),
                SOCKET_ID.fetch_add(1, Ordering::Relaxed),
                super::evidence::hex(&launch_id[..8]),
            ));
            match fs::create_dir(&root) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(Error::Io(error)),
            }
            if let Err(error) = seal_mode(&root, 0o700) {
                let _ = fs::remove_dir(&root);
                return Err(error);
            }
            let socket = root.join("cell.sock");
            let listener = match UnixListener::bind(&socket) {
                Ok(listener) => listener,
                Err(error) => {
                    let _ = fs::remove_dir(&root);
                    return Err(Error::Io(error));
                }
            };
            if let Err(error) = seal_mode(&socket, 0o600) {
                let _ = fs::remove_file(&socket);
                let _ = fs::remove_dir(&root);
                return Err(error);
            }
            if let Err(error) = listener.set_nonblocking(true) {
                let _ = fs::remove_file(&socket);
                let _ = fs::remove_dir(&root);
                return Err(Error::Io(error));
            }
            return Ok(Self {
                root,
                socket,
                listener,
                launch_id,
            });
        }
        Err(Error::Collision)
    }

    #[must_use]
    pub const fn launch_id(&self) -> [u8; 32] {
        self.launch_id
    }

    #[must_use]
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    pub fn accept(self, child: &mut Child, request: Request) -> Result<Parent, Error> {
        self.authenticate(child, request, Instant::now() + CONNECT_TIMEOUT)?
            .send()
    }

    /// Authenticate without sending; a shared deadline cannot renew a paired launch.
    pub fn authenticate(
        mut self,
        child: &mut Child,
        request: Request,
        deadline: Instant,
    ) -> Result<Pending, Error> {
        request.validate()?;
        if request.core.launch_id != self.launch_id {
            return Err(Error::Request("listener launch"));
        }
        let deadline = deadline.min(Instant::now() + CONNECT_TIMEOUT);
        let stream = loop {
            if Instant::now() >= deadline {
                return Err(Error::Timeout);
            }
            match self.listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if child.try_wait().map_err(Error::Io)?.is_some() {
                        return Err(Error::EarlyExit);
                    }
                    if Instant::now() >= deadline {
                        return Err(Error::Timeout);
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(error) => return Err(Error::Io(error)),
            }
        };
        configure(&stream)?;
        let expected = Peer {
            pid: child.id(),
            uid: effective_uid(),
        };
        let observed = peer(&stream)?;
        if observed != expected {
            return Err(Error::Peer { expected, observed });
        }
        fs::remove_file(&self.socket).map_err(Error::Io)?;
        self.socket = PathBuf::new();
        Ok(Pending {
            stream,
            request,
            child: expected,
        })
    }
}

impl Pending {
    /// Send exactly the original Request followed by EOF, once.
    pub fn send(self) -> Result<Parent, Error> {
        self.send_before(Instant::now() + OWNER_TIMEOUT)
    }

    /// Arm cancellation before dispatch so a parent can poll both exchanges.
    pub fn exchange(self, deadline: Instant) -> Result<(Exchange, Closure), Error> {
        let stream = self.stream.try_clone().map_err(Error::Io)?;
        Ok((
            Exchange {
                pending: self,
                deadline,
            },
            Closure { stream },
        ))
    }

    fn send_before(mut self, deadline: Instant) -> Result<Parent, Error> {
        let elapsed = super::monotonic_ns()
            .and_then(|now| now.checked_sub(self.request.core.launched_ns))
            .ok_or(Error::Request("launch clock"))?;
        let remaining = self
            .request
            .core
            .bounds
            .owner_deadline_ns
            .checked_sub(elapsed)
            .filter(|remaining| *remaining != 0)
            .ok_or(Error::Timeout)?;
        let deadline = deadline.min(Instant::now() + Duration::from_nanos(remaining));
        frame::send_before(
            &mut self.stream,
            frame::Tag::Request,
            &self.request,
            deadline,
        )?;
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        self.stream
            .shutdown(std::net::Shutdown::Write)
            .map_err(Error::Io)?;
        if Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        let closure = self.stream.try_clone().map_err(Error::Io)?;
        Ok(Parent {
            receiver: Receiver {
                stream: self.stream,
                request: self.request,
                child: self.child,
            },
            closure: Closure { stream: closure },
        })
    }
}

impl Exchange {
    /// Perform the unchanged Request+EOF and capsule+EOF exchange once.
    pub fn finish(self) -> Result<Capsule, ReceptionFailure> {
        self.pending
            .send_before(self.deadline)
            .map_err(|source| ReceptionFailure {
                source,
                canonical_capsule: None,
            })?
            .receiver
            .receive()
    }
}

impl Drop for Listening {
    fn drop(&mut self) {
        if !self.socket.as_os_str().is_empty() {
            let _ = fs::remove_file(&self.socket);
        }
        let _ = fs::remove_dir(&self.root);
    }
}

impl Parent {
    /// Split successful reception from failure-only capability closure.
    #[must_use]
    pub fn split(self) -> (Receiver, Closure) {
        (self.receiver, self.closure)
    }
}

impl Receiver {
    pub fn finish(self) -> Result<Capsule, Error> {
        self.receive().map_err(|failure| failure.source)
    }

    /// Preserve any decoded capsule if its digest, binding, or final EOF fails.
    pub fn receive(mut self) -> Result<Capsule, ReceptionFailure> {
        let mut observed = None;
        let result = (|| {
            let capsule = match self.request.core.role {
                Role::Setup => {
                    let envelope: SetupEnvelope =
                        frame::receive(&mut self.stream, frame::Tag::SetupCapsule)?;
                    observed = Some(canonical(&envelope.payload)?);
                    let capsule = envelope.open()?;
                    capsule.validate(&self.request, self.child)?;
                    Capsule::Setup(capsule)
                }
                Role::RetainedCell => {
                    let envelope: CellEnvelope =
                        frame::receive(&mut self.stream, frame::Tag::CellCapsule)?;
                    observed = Some(canonical(&envelope.payload)?);
                    let capsule = envelope.open()?;
                    capsule.validate(&self.request, self.child)?;
                    Capsule::Retained(capsule)
                }
            };
            require_eof(&mut self.stream)?;
            Ok(capsule)
        })();
        result.map_err(|source| ReceptionFailure {
            source,
            canonical_capsule: observed,
        })
    }
}

impl Closure {
    /// Close both halves only after the parent has invalidated this attempt.
    pub fn invalidate(self) -> Result<(), Error> {
        self.stream
            .shutdown(std::net::Shutdown::Both)
            .map_err(Error::Io)
    }
}

impl Session {
    /// Authenticate one direct parent and consume exactly one canonical request.
    pub fn transform(
        socket: &Path,
        launch_id: [u8; 32],
        request_digest: [u8; 32],
        expected_build: &Build,
        fixture_digest: [u8; 32],
    ) -> Result<Self, Error> {
        if !socket.is_absolute() {
            return Err(Error::Socket);
        }
        let metadata = fs::symlink_metadata(socket).map_err(Error::Io)?;
        if !metadata.file_type().is_socket() || metadata.permissions().mode() & 0o777 != 0o600 {
            return Err(Error::Socket);
        }
        let mut stream = UnixStream::connect(socket).map_err(Error::Io)?;
        configure(&stream)?;
        let expected_parent = Peer {
            pid: parent_pid()?,
            uid: effective_uid(),
        };
        let observed_parent = peer(&stream)?;
        if observed_parent != expected_parent {
            return Err(Error::Peer {
                expected: expected_parent,
                observed: observed_parent,
            });
        }
        let request: Request = frame::receive(&mut stream, frame::Tag::Request)?;
        request.validate()?;
        require_eof(&mut stream)?;
        let current_executable = file::Identity::read(&std::env::current_exe().map_err(Error::Io)?)
            .map_err(Error::Evidence)?;
        let current_project = project::Identity::read(&request.core.project.path()?)?;
        if request.core.launch_id != launch_id
            || request.request_digest != request_digest
            || &request.core.build != expected_build
            || request.core.build.executable != current_executable
            || request.core.fixture_digest != fixture_digest
            || request.core.project != current_project
        {
            return Err(Error::Request("child binding"));
        }
        Ok(Self {
            stream,
            request,
            child: Peer {
                pid: std::process::id(),
                uid: effective_uid(),
            },
            executable: current_executable,
        })
    }

    #[must_use]
    pub const fn request(&self) -> &Request {
        &self.request
    }

    /// Emit exactly one role-specific final capsule and close the write half.
    pub fn finish(
        mut self,
        owner_entry_ns: u64,
        owner_return_ns: u64,
        mut counters: Snapshot,
        outcome: Outcome,
    ) -> Result<(), Error> {
        if let Some(principal) = &mut counters.principal {
            principal
                .prepare_cell_transport(
                    &mut counters.accounting,
                    self.request.core.bounds.counter_limit,
                )
                .map_err(Error::Project)?;
        }
        match self.request.core.role {
            Role::Setup => {
                let capsule = SetupCapsule::transform(
                    &self,
                    owner_entry_ns,
                    owner_return_ns,
                    counters,
                    outcome,
                )?;
                frame::send(
                    &mut self.stream,
                    frame::Tag::SetupCapsule,
                    &SetupEnvelope::seal(capsule)?,
                )?;
            }
            Role::RetainedCell => {
                let capsule = CellCapsule::transform(
                    &self,
                    owner_entry_ns,
                    owner_return_ns,
                    counters,
                    outcome,
                )?;
                frame::send(
                    &mut self.stream,
                    frame::Tag::CellCapsule,
                    &CellEnvelope::seal(capsule)?,
                )?;
            }
        }
        self.stream
            .shutdown(std::net::Shutdown::Write)
            .map_err(Error::Io)
    }
}

fn validate_core(core: &RequestCore) -> Result<(), Error> {
    core.build.validate()?;
    core.project.validate()?;
    let profile_valid = match (core.scenario, core.role, core.profile) {
        (Scenario::ProfileRule, Role::Setup, Profile::Baseline)
        | (Scenario::ProfileRule, Role::RetainedCell, Profile::Variant) => true,
        (Scenario::ProfileRule, _, _) => false,
        (_, _, Profile::Inactive) => true,
        (_, _, Profile::Baseline | Profile::Variant) => false,
    };
    let ordinal_valid = match core.role {
        Role::Setup => {
            core.ordinal == 0
                && !matches!(
                    core.scenario,
                    Scenario::CleanCold | Scenario::ConcurrentCold
                )
        }
        Role::RetainedCell => core.ordinal == 1,
    };
    if core.schema.as_ref() != REQUEST_SCHEMA
        || core.corpus_id == [0; 32]
        || core.launch_id == [0; 32]
        || core.launched_ns == 0
        || core.fixture_digest == [0; 32]
        || (!Scenario::CURRENT.contains(&core.scenario)
            && !matches!(
                core.scenario,
                Scenario::CorruptRunner | Scenario::CorruptResolution | Scenario::ConcurrentCold
            ))
        || core.bounds != Bounds::standard()
        || !profile_valid
        || !ordinal_valid
    {
        return Err(Error::Request("core"));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_capsule(
    schema_role: bool,
    observed_request: &Request,
    expected_request: &Request,
    observed_child: Peer,
    expected_child: Peer,
    executable: &file::Identity,
    owner_entry_ns: u64,
    owner_return_ns: u64,
    counters: &Snapshot,
    outcome: &Outcome,
) -> Result<(), Error> {
    observed_request.validate()?;
    outcome.validate()?;
    let runner_matches = match (outcome, counters.runner.as_ref()) {
        (Outcome::Success { generation }, Some(runner)) => {
            *generation == runner.generation_key
                && runner.runner_pid != observed_child.pid
                && runner.runner_uid == observed_child.uid
        }
        (Outcome::Failure { .. }, _) => true,
        (Outcome::Success { .. }, None) => false,
    };
    let pipeline_matches = matches!(outcome, Outcome::Failure { .. })
        || (counters.profile.is_some() && counters.pipeline.is_some());
    if !schema_role
        || observed_request != expected_request
        || observed_child != expected_child
        || executable != &expected_request.core.build.executable
        || owner_entry_ns < expected_request.core.launched_ns
        || owner_return_ns < owner_entry_ns
        || owner_return_ns.saturating_sub(owner_entry_ns)
            > expected_request.core.bounds.owner_deadline_ns
        || counters.nodes.len() != 44
        || counters.processes.len() != 13
        || counters.milestones.len() != 6
        || !counters.accounting.valid()
        || !pipeline_matches
        || !runner_matches
    {
        return Err(Error::Capsule);
    }
    Ok(())
}

fn configure(stream: &UnixStream) -> Result<(), Error> {
    stream.set_nonblocking(false).map_err(Error::Io)?;
    stream
        .set_read_timeout(Some(OWNER_TIMEOUT))
        .map_err(Error::Io)?;
    stream
        .set_write_timeout(Some(OWNER_TIMEOUT))
        .map_err(Error::Io)
}

fn require_eof(stream: &mut UnixStream) -> Result<(), Error> {
    let mut trailing = [0_u8; 1];
    match stream.read(&mut trailing) {
        Ok(0) => Ok(()),
        Ok(_) => Err(Error::Trailing),
        Err(error) => Err(Error::Io(error)),
    }
}

#[cfg(target_os = "macos")]
fn peer(stream: &UnixStream) -> Result<Peer, Error> {
    use nix::sys::socket::{getsockopt, sockopt};

    let pid = getsockopt(stream, sockopt::LocalPeerPid).map_err(Error::Nix)?;
    let credentials = getsockopt(stream, sockopt::LocalPeerCred).map_err(Error::Nix)?;
    if credentials.version() != 0 {
        return Err(Error::Credentials);
    }
    Ok(Peer {
        pid: u32::try_from(pid).map_err(|_| Error::Credentials)?,
        uid: credentials.uid(),
    })
}

#[cfg(not(target_os = "macos"))]
fn peer(_stream: &UnixStream) -> Result<Peer, Error> {
    Err(Error::Unsupported)
}

fn parent_pid() -> Result<u32, Error> {
    u32::try_from(nix::unistd::getppid().as_raw()).map_err(|_| Error::Credentials)
}

fn effective_uid() -> u32 {
    nix::unistd::Uid::effective().as_raw()
}

fn seal_mode(path: &Path, expected: u32) -> Result<(), Error> {
    fs::set_permissions(path, fs::Permissions::from_mode(expected)).map_err(Error::Io)?;
    let actual = fs::symlink_metadata(path)
        .map_err(Error::Io)?
        .permissions()
        .mode()
        & 0o777;
    if actual == expected {
        Ok(())
    } else {
        Err(Error::Mode { expected, actual })
    }
}

fn random() -> Result<[u8; 32], Error> {
    let mut file = fs::File::open("/dev/urandom").map_err(Error::Io)?;
    let mut bytes = [0_u8; 32];
    file.read_exact(&mut bytes).map_err(Error::Io)?;
    Ok(bytes)
}

/// Generate one unpredictable corpus identity owned by the parent harness.
pub fn corpus_id() -> Result<[u8; 32], Error> {
    random()
}

fn features_digest() -> [u8; 32] {
    Sha256::digest(b"rawr.phase9.cell.features/v1\0default\0native-runner\0phase9-baseline").into()
}

fn digest(domain: &'static [u8], value: &impl Serialize) -> Result<[u8; 32], Error> {
    let bytes = canonical(value)?;
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(bytes);
    Ok(digest.finalize().into())
}

fn canonical(value: &impl Serialize) -> Result<Box<[u8]>, Error> {
    encode::canonical(value)
}

fn decode<T>(bytes: &[u8]) -> Result<T, Error>
where
    T: serde::de::DeserializeOwned + Serialize,
{
    let value = serde_json::from_slice(bytes).map_err(Error::Json)?;
    if canonical(&value)?.as_ref() != bytes {
        return Err(Error::Canonical);
    }
    Ok(value)
}

/// Closed cell protocol failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cell protocol I/O failed")]
    Io(#[source] std::io::Error),
    #[error("cell protocol socket option failed")]
    Nix(#[source] nix::Error),
    #[error("cell peer validation is unsupported")]
    Unsupported,
    #[error("cell peer credentials are malformed")]
    Credentials,
    #[error("cell peer differs from the exact expected process")]
    Peer { expected: Peer, observed: Peer },
    #[error("cell endpoint mode differs: expected {expected:o}, observed {actual:o}")]
    Mode { expected: u32, actual: u32 },
    #[error("cell endpoint name collided repeatedly")]
    Collision,
    #[error("cell child exited before connecting")]
    EarlyExit,
    #[error("cell operation exceeded its fixed deadline")]
    Timeout,
    #[error("cell socket is not one absolute mode-0600 socket")]
    Socket,
    #[error("cell build identity is invalid")]
    Build,
    #[error("cell project identity is invalid at {0}")]
    Project(&'static str),
    #[error("cell request is invalid at {0}")]
    Request(&'static str),
    #[error("cell owner outcome is invalid")]
    Outcome,
    #[error("cell capsule is invalid")]
    Capsule,
    #[error("cell protocol contains trailing bytes")]
    Trailing,
    #[error("cell protocol serialization is invalid")]
    Json(#[source] serde_json::Error),
    #[error("cell frame magic, version, tag, or length is invalid")]
    Frame,
    #[error("cell protocol payload is not canonical")]
    Canonical,
    #[error("cell protocol payload digest differs")]
    Digest,
    #[error("shared executable evidence is invalid")]
    Evidence(#[source] super::evidence::Error),
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{Read as _, Write as _};

    #[test]
    fn configured_stream_waits_for_a_delayed_frame_after_nonblocking_accept() {
        let (mut writer, mut reader) = std::os::unix::net::UnixStream::pair().expect("socket pair");
        reader
            .set_nonblocking(true)
            .expect("accepted stream starts nonblocking");
        super::configure(&reader).expect("cell stream configures");
        let delayed = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(10));
            writer.write_all(&[19]).expect("delayed byte writes");
        });
        let mut byte = [0_u8; 1];
        reader.read_exact(&mut byte).expect("configured read waits");
        delayed.join().expect("delayed writer joins");
        assert_eq!(byte, [19]);
    }

    #[test]
    fn invalidation_closes_every_receiver_clone_without_a_control_frame() {
        let (stream, _peer) = std::os::unix::net::UnixStream::pair().expect("socket pair");
        let mut receiver = stream.try_clone().expect("receiver clone");
        let waiting = std::thread::spawn(move || {
            let mut byte = [0_u8; 1];
            receiver.read(&mut byte)
        });
        super::Closure { stream }
            .invalidate()
            .expect("failure closure shuts down the evidence capability");
        assert_eq!(
            waiting
                .join()
                .expect("receiver scope joins")
                .expect("read completes"),
            0
        );
    }

    #[test]
    fn every_closed_request_state_rejects_illegal_profile_or_role_pairs() {
        use super::{Profile, Role};
        use crate::metrics::report::Scenario;

        for (scenario, role, profile, valid) in [
            (Scenario::ProfileRule, Role::Setup, Profile::Baseline, true),
            (
                Scenario::ProfileRule,
                Role::RetainedCell,
                Profile::Variant,
                true,
            ),
            (
                Scenario::ProfileRule,
                Role::RetainedCell,
                Profile::Inactive,
                false,
            ),
            (
                Scenario::IdenticalWarm,
                Role::RetainedCell,
                Profile::Inactive,
                true,
            ),
            (
                Scenario::IdenticalWarm,
                Role::RetainedCell,
                Profile::Variant,
                false,
            ),
        ] {
            let observed = match (scenario, role, profile) {
                (Scenario::ProfileRule, Role::Setup, Profile::Baseline)
                | (Scenario::ProfileRule, Role::RetainedCell, Profile::Variant) => true,
                (Scenario::ProfileRule, _, _) => false,
                (_, _, Profile::Inactive) => true,
                (_, _, Profile::Baseline | Profile::Variant) => false,
            };
            assert_eq!(observed, valid);
        }
    }

    #[test]
    fn cell_and_evidence_frame_domains_are_distinct() {
        assert_ne!(super::frame::MAGIC, *b"R9EV");
    }

    #[test]
    fn request_binds_project_cache_executable_and_excludes_runtime_title() {
        use super::{Build, Profile, Request, Role};
        use crate::metrics::report::{Command, Scenario};

        let root = std::env::temp_dir().join(format!(
            "rawr-cell-request-{}-{}",
            std::process::id(),
            super::SOCKET_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ));
        for relative in ["native/src", "src"] {
            fs::create_dir_all(root.join(relative)).expect("fixture directory creates");
        }
        for (relative, bytes) in [
            (
                "Cargo.toml",
                b"[package]\nname='cell'\nversion='0.0.0'\n".as_slice(),
            ),
            ("rawr.toml", b"[application]\nsource='src/main.rawr'\n"),
            (
                "native/Cargo.toml",
                b"[package]\nname='native'\nversion='0.0.0'\n",
            ),
            ("native/src/lib.rs", b"pub fn value() -> u8 { 1 }\n"),
            ("src/main.rawr", b"fn main() {}\n"),
        ] {
            fs::write(root.join(relative), bytes).expect("fixture member writes");
        }
        let root = fs::canonicalize(&root).expect("fixture canonicalizes");
        let before = super::project::Identity::read(&root).expect("project identity seals");
        fs::write(root.join(".phase9-window-title"), b"Title One\n").expect("runtime title writes");
        let titled = super::project::Identity::read(&root).expect("titled identity seals");
        assert_eq!(before, titled);

        let build = Build::transform(
            &std::env::current_exe().expect("test executable resolves"),
            "unit-source-tree",
            [4; 32],
        )
        .expect("build identity seals");
        let request = Request::transform(
            [1; 32],
            [2; 32],
            crate::metrics::monotonic_ns().expect("clock reads"),
            build,
            before,
            [3; 32],
            Command::Check,
            Scenario::IdenticalWarm,
            Role::Setup,
            Profile::Inactive,
            0,
        )
        .expect("legal request seals");
        request.validate().expect("sealed request validates");
        for scenario in [Scenario::CorruptRunner, Scenario::CorruptResolution] {
            for (role, ordinal) in [(Role::Setup, 0), (Role::RetainedCell, 1)] {
                Request::transform(
                    [1; 32],
                    [2; 32],
                    request.core.launched_ns,
                    request.core.build.clone(),
                    request.core.project.clone(),
                    [3; 32],
                    Command::Check,
                    scenario,
                    role,
                    Profile::Inactive,
                    ordinal,
                )
                .expect("implemented corruption uses the same authenticated child request");
            }
        }
        assert!(
            Request::transform(
                [1; 32],
                [2; 32],
                request.core.launched_ns,
                request.core.build.clone(),
                request.core.project.clone(),
                [3; 32],
                Command::Check,
                Scenario::ConcurrentCold,
                Role::RetainedCell,
                Profile::Inactive,
                1,
            )
            .is_ok(),
            "one concurrent capsule is transport evidence; only the parent can admit a pair"
        );
        assert!(
            Request::transform(
                [1; 32],
                [2; 32],
                request.core.launched_ns,
                request.core.build.clone(),
                request.core.project.clone(),
                [3; 32],
                Command::Check,
                Scenario::ConcurrentCold,
                Role::Setup,
                Profile::Inactive,
                0,
            )
            .is_err(),
            "a paired cell has no setup-child authority"
        );

        // Parent-only deferral changes no wire bytes and sends nothing early.
        let (writer, mut reader) = std::os::unix::net::UnixStream::pair().unwrap();
        reader.set_nonblocking(true).unwrap();
        let pending = super::Pending {
            stream: writer,
            request: request.clone(),
            child: super::Peer { pid: 42, uid: 501 },
        };
        let mut byte = [0];
        assert_eq!(
            std::io::Read::read(&mut reader, &mut byte)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::WouldBlock
        );
        reader.set_nonblocking(false).unwrap();
        let parent = pending.send().unwrap();
        let received: Request =
            super::frame::receive(&mut reader, super::frame::Tag::Request).unwrap();
        assert_eq!(received, request);
        super::require_eof(&mut reader).unwrap();
        drop(parent);

        // An invalidated unsent exchange cannot become an empty successful request.
        let (writer, _reader) = std::os::unix::net::UnixStream::pair().unwrap();
        let pending = super::Pending {
            stream: writer,
            request: request.clone(),
            child: super::Peer { pid: 42, uid: 501 },
        };
        let (exchange, closure) = pending
            .exchange(std::time::Instant::now() + std::time::Duration::from_secs(1))
            .unwrap();
        closure.invalidate().unwrap();
        assert!(exchange.finish().is_err());

        rejected_payload_survives(request.clone());
        let mut changed = request;
        changed.core.project.cache.digest[0] ^= 1;
        assert!(changed.validate().is_err());
        fs::remove_dir_all(root).expect("fixture retires");
    }
    fn rejected_payload_survives(request: super::Request) {
        use std::io::Write as _;
        for failure in 0..3 {
            let (mut writer, reader) = std::os::unix::net::UnixStream::pair().unwrap();
            let child = super::Peer { pid: 42, uid: 501 };
            let (session_stream, _session_peer) = std::os::unix::net::UnixStream::pair().unwrap();
            let session = super::Session {
                stream: session_stream,
                request: request.clone(),
                child,
                executable: request.core.build.executable.clone(),
            };
            let snapshot =
                crate::metrics::recorder::Recorder::transform_at(100, request.core.launched_ns)
                    .unwrap()
                    .snapshot()
                    .unwrap();
            let mut payload = super::SetupCapsule::transform(
                &session,
                request.core.launched_ns,
                request.core.launched_ns + 1,
                snapshot,
                super::Outcome::Failure {
                    reason: "original owner refusal".into(),
                },
            )
            .unwrap();
            if failure == 1 {
                payload.child.pid += 1;
            }
            let expected = super::canonical(&payload).unwrap();
            let mut envelope = super::SetupEnvelope::seal(payload).unwrap();
            if failure == 2 {
                envelope.payload_digest[0] ^= 1;
            }
            let sender = std::thread::spawn(move || {
                super::frame::send(&mut writer, super::frame::Tag::SetupCapsule, &envelope)
                    .unwrap();
                if failure == 0 {
                    writer.write_all(b"trailing").unwrap();
                }
                writer.shutdown(std::net::Shutdown::Write).unwrap();
            });
            let failure = super::Receiver {
                stream: reader,
                request: request.clone(),
                child,
            }
            .receive()
            .err()
            .unwrap();
            sender.join().unwrap();
            assert_eq!(failure.canonical_capsule.unwrap(), expected);
        }
    }
}
