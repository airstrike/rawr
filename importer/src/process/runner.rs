//! Authenticated Unix-socket runner process ownership and staged handshake.

use std::fs::{self, File};
use std::io::{IsTerminal as _, Read as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt as _;
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
#[cfg(unix)]
use std::os::unix::process::CommandExt as _;

static SOCKET_ID: AtomicU64 = AtomicU64::new(1);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const IO_TIMEOUT: Duration = Duration::from_secs(10);
/// Read and write poll interval while a guarded exchange awaits its peer.
const POLL_INTERVAL: Duration = Duration::from_millis(20);
const WORK_PROGRESS_LIMIT: u8 = 16;
const WORK_PROGRESS_BYTES: usize = 32 + 16 + 3;

mod accounting;
mod resident;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
enum WorkOperation {
    Prepare = 1,
    Admit = 2,
    Compile = 3,
    Snapshot = 4,
}

impl WorkOperation {
    const fn permits(self, stage: u8) -> bool {
        match self {
            Self::Prepare => matches!(stage, 1..=6),
            Self::Admit => matches!(stage, 7 | 8),
            Self::Compile => matches!(stage, 1..=5 | 8 | 9),
            Self::Snapshot => stage == 10,
        }
    }
}

struct ProgressState {
    key: [u8; 32],
    nonce: [u8; 16],
    operation: WorkOperation,
    ordinal: u8,
    stage: u8,
}

impl ProgressState {
    const fn new(key: [u8; 32], nonce: [u8; 16], operation: WorkOperation) -> Self {
        Self {
            key,
            nonce,
            operation,
            ordinal: 0,
            stage: 0,
        }
    }

    fn observe(&mut self, payload: &[u8]) -> Result<(), Error> {
        if payload.len() != WORK_PROGRESS_BYTES
            || payload[..32] != self.key
            || payload[32..48] != self.nonce
            || payload[48] != self.operation as u8
        {
            return Err(Error::WorkProgress("identity"));
        }
        let ordinal = payload[49];
        let stage = payload[50];
        if ordinal == 0 || ordinal > WORK_PROGRESS_LIMIT {
            return Err(Error::WorkProgress("cardinality"));
        }
        if ordinal != self.ordinal.saturating_add(1) {
            return Err(Error::WorkProgress("ordinal"));
        }
        if stage <= self.stage || !self.operation.permits(stage) {
            return Err(Error::WorkProgress("stage"));
        }
        self.ordinal = ordinal;
        self.stage = stage;
        Ok(())
    }
}

/// Owner of one safe runtime-socket directory.
pub struct Owner {
    root: PathBuf,
    project: PathBuf,
    lifecycle: Lifecycle,
    program: Option<Arguments>,
}

/// What a program `rawr run` runs gets as `cargo run` gives it (D500.1):
/// its name as its first argument, the arguments after `--`, and stdin.
#[derive(Clone, Debug, Default)]
pub struct Arguments {
    /// The first argument, the path of the binary `cargo run` would run.
    pub name: std::ffi::OsString,
    /// The arguments after `--`.
    pub arguments: Vec<std::ffi::OsString>,
    /// What the owner adds to each runner's command before it spawns, such
    /// as clearing a signal mask its own threads keep, which a spawned
    /// program would otherwise inherit.
    pub prepare: Option<fn(&mut Command)>,
}

/// Closed generated-runner product lifecycle fixed before process admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lifecycle {
    Check,
    Run,
    Preview,
    SnapshotCheck,
    SnapshotUpdate,
}

/// Candidate whose manifest and capability handshake succeeded.
pub struct Spawned {
    _private: (),
}

/// Side-effect-free snapshot preparation completed.
pub struct Prepared {
    _private: (),
}

/// A single-use generation capability was accepted.
pub struct Activated {
    _private: (),
}

/// The route swap was acknowledged by the candidate.
pub struct Admitted {
    _private: (),
}

/// Opaque process-group ownership indexed by handshake state.
pub struct Candidate<P, B> {
    inner: Option<Inner<B>>,
    _phase: P,
}

struct Inner<B> {
    child: super::ownership::Child,
    stream: UnixStream,
    key: crate::supervisor::GenerationKey,
    epoch: crate::supervisor::Epoch,
    manifest: crate::runner::Manifest<crate::runner::Built, B>,
    // The embedded carrier cannot change when ordinary hot metadata advances.
    resident_carrier: [u8; 32],
    route: Option<crate::profile::activation::Route>,
    sequence: crate::profile::message::Sequence,
    stdout: Option<thread::JoinHandle<std::io::Result<Box<[u8]>>>>,
    stderr: Option<thread::JoinHandle<std::io::Result<Box<[u8]>>>>,
    reaped: bool,
    lifecycle: Lifecycle,
    started: bool,
    observed: bool,
    hot_sequence: u64,
    accounting: accounting::Pending,
}

impl Lifecycle {
    const fn code(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Run => "run",
            Self::Preview => "preview",
            Self::SnapshotCheck => "snapshot-check",
            Self::SnapshotUpdate => "snapshot-update",
        }
    }
}

impl Owner {
    /// Create and seal an owner-only bounded socket directory while retaining
    /// `runtime_root` as the generated product context.
    #[cfg(unix)]
    pub fn new(runtime_root: &Path) -> Result<Self, Error> {
        Self::for_lifecycle(runtime_root, Lifecycle::Check)
    }

    /// Create one runner owner with a product mode sealed before spawn.
    #[cfg(unix)]
    pub fn for_lifecycle(runtime_root: &Path, lifecycle: Lifecycle) -> Result<Self, Error> {
        let runtime_root = fs::canonicalize(runtime_root).map_err(|source| Error::Io {
            operation: "canonicalize runner runtime root",
            path: runtime_root.to_path_buf(),
            source,
        })?;
        let socket_base = fs::canonicalize("/tmp").map_err(|source| Error::Io {
            operation: "canonicalize runner socket base",
            path: PathBuf::from("/tmp"),
            source,
        })?;
        let nonce = hex(&random()?);
        let root = socket_base.join(format!(".rawr-r-{}-{}", std::process::id(), &nonce[..16]));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .map_err(|source| Error::Io {
                operation: "create exclusive runner socket directory",
                path: root.clone(),
                source,
            })?;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).map_err(|source| {
            Error::Io {
                operation: "seal runner socket directory mode",
                path: root.clone(),
                source,
            }
        })?;
        let mode = fs::symlink_metadata(&root)
            .map_err(|source| Error::Io {
                operation: "revalidate runner socket directory",
                path: root.clone(),
                source,
            })?
            .permissions()
            .mode()
            & 0o777;
        if mode != 0o700 {
            return Err(Error::SocketMode { actual: mode });
        }
        Ok(Self {
            root,
            project: runtime_root,
            lifecycle,
            program: None,
        })
    }

    /// Give each runner `rawr run` starts `program`'s arguments and this
    /// process's stdin. When stdin is the terminal this process has in the
    /// foreground, the runner's process group gets it while it runs, so it
    /// can read it, and this process takes it back when the runner ends.
    /// That takes it back from the background, so the caller blocks
    /// `SIGTTOU` on the thread that owns the runner.
    #[must_use]
    pub fn with_program(mut self, program: Arguments) -> Self {
        self.program = Some(program);
        self
    }

    #[cfg(not(unix))]
    pub fn new(_runtime_root: &Path) -> Result<Self, Error> {
        Err(Error::Unsupported)
    }

    #[cfg(not(unix))]
    pub fn for_lifecycle(_runtime_root: &Path, _lifecycle: Lifecycle) -> Result<Self, Error> {
        Err(Error::Unsupported)
    }
}

#[cfg(unix)]
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.root);
    }
}

impl<B> crate::runner::Manifest<crate::runner::Built, B> {
    /// Spawn and authenticate one new whole-process epoch.
    #[cfg(unix)]
    pub fn spawn(
        self,
        epoch: crate::supervisor::Epoch,
        process: &Owner,
    ) -> Result<Candidate<Spawned, B>, Error> {
        self.revalidate_native()?;
        let key = crate::supervisor::GenerationKey::seal(
            self.generated.build_request_key(),
            self.request.snapshot,
            &self.request.canonical,
            self.revision,
            epoch,
            &self.native_identity(),
        );
        let carrier = self.generated.carrier()?;
        let socket = socket_path(&process.root)?;
        let listener = UnixListener::bind(&socket).map_err(|source| Error::Io {
            operation: "bind runner protocol socket",
            path: socket.clone(),
            source,
        })?;
        fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).map_err(|source| {
            Error::Io {
                operation: "seal runner protocol socket mode",
                path: socket.clone(),
                source,
            }
        })?;
        let mode = fs::symlink_metadata(&socket)
            .map_err(|source| Error::Io {
                operation: "revalidate runner protocol socket",
                path: socket.clone(),
                source,
            })?
            .permissions()
            .mode()
            & 0o777;
        if mode != 0o600 {
            let _ = fs::remove_file(&socket);
            return Err(Error::ProtocolSocketMode { actual: mode });
        }
        listener.set_nonblocking(true).map_err(|source| Error::Io {
            operation: "configure runner protocol listener",
            path: socket.clone(),
            source,
        })?;
        let capability = random()?;
        let mut command = Command::new(self.artifact().executable());
        // A program `rawr run` runs sees the environment `cargo run` would
        // give it; every other lifecycle starts from none.
        if process.lifecycle != Lifecycle::Run {
            command.env_clear();
        }
        command
            .env("RAWR_PROTOCOL_SOCKET", &socket)
            .env("RAWR_PROTOCOL_CAPABILITY", hex(&capability))
            .env("RAWR_GENERATION_KEY", hex(key.as_bytes()))
            .env("RAWR_RUNNER_LIFECYCLE", process.lifecycle.code())
            .env("RAWR_PROJECT_ROOT", &process.project)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let program = process
            .program
            .as_ref()
            .filter(|_| process.lifecycle == Lifecycle::Run);
        if let Some(program) = program {
            command
                .arg0(&program.name)
                .args(&program.arguments)
                .stdin(Stdio::inherit());
            // At a terminal, the program writes to it itself, so it sees a
            // terminal as under `cargo run` (D534.5). The protocol has its
            // own socket. A failure's stderr is then on the screen, not in
            // the error.
            if std::io::stdout().is_terminal() {
                command.stdout(Stdio::inherit());
            }
            if std::io::stderr().is_terminal() {
                command.stderr(Stdio::inherit());
            }
            if let Some(prepare) = program.prepare {
                prepare(&mut command);
            }
        }
        // One preview rendered alone (`rawr mcp`'s `render_preview`).
        if let (Lifecycle::SnapshotCheck | Lifecycle::SnapshotUpdate, Some(selection)) =
            (process.lifecycle, std::env::var_os("RAWR_SNAPSHOT_ONLY"))
        {
            command.env("RAWR_SNAPSHOT_ONLY", selection);
        }
        let mut child = if program.is_some() {
            super::ownership::Child::spawn_with_terminal(&mut command)
        } else {
            super::ownership::Child::spawn(&mut command)
        }
        .map_err(resource_error)?;
        let evidence_spawned_ns = crate::metrics::monotonic_ns();
        let runner_pid = child.id();
        crate::metrics::record_process(crate::metrics::ProcessKind::Runner);
        crate::metrics::record_milestone(crate::metrics::Milestone::RunnerSpawned);
        // What a program writes to the terminal itself has no log.
        let stdout = child.stdout();
        let stderr = child.stderr();
        if program.is_none() && (stdout.is_none() || stderr.is_none()) {
            terminate(&mut child);
            let _ = fs::remove_file(&socket);
            return Err(Error::MissingLog(if stdout.is_none() {
                "stdout"
            } else {
                "stderr"
            }));
        }
        // `rawr run` shows what the program prints as it prints it.
        let forward = process.lifecycle == Lifecycle::Run;
        let stdout = stdout
            .map(|stdout| thread::spawn(move || tail(stdout, forward.then_some(Forward::Stdout))));
        let stderr = stderr
            .map(|stderr| thread::spawn(move || tail(stderr, forward.then_some(Forward::Stderr))));
        let stream = match accept(&listener, &mut child) {
            Ok(stream) => stream,
            Err(error) => {
                terminate(&mut child);
                let _ = fs::remove_file(&socket);
                return Err(error);
            }
        };
        fs::remove_file(&socket).map_err(|source| Error::Io {
            operation: "remove accepted runner socket",
            path: socket,
            source,
        })?;
        if let Err(source) = stream
            .set_nonblocking(false)
            .and_then(|()| stream.set_read_timeout(Some(IO_TIMEOUT)))
            .and_then(|()| stream.set_write_timeout(Some(IO_TIMEOUT)))
        {
            let failure = Error::ProtocolIo(source);
            let cleanup = child.terminate();
            if child.is_reaped() {
                let _ = stdout.map(thread::JoinHandle::join);
                let _ = stderr.map(thread::JoinHandle::join);
            }
            return match cleanup {
                Ok(()) => Err(failure),
                Err(cleanup) => Err(Error::OwnerLost {
                    reason: format!("{failure}; runner cleanup failed: {cleanup}").into_boxed_str(),
                }),
            };
        }
        let mut inner = Inner {
            child,
            stream,
            key,
            epoch,
            resident_carrier: crate::runner::manifest::carrier_identity(&carrier),
            manifest: self,
            route: None,
            sequence: crate::profile::message::Sequence::first(),
            stdout,
            stderr,
            reaped: false,
            lifecycle: process.lifecycle,
            started: false,
            observed: false,
            hot_sequence: 0,
            accounting: accounting::Pending::default(),
        };
        let authentication = guarded(&mut inner, |inner| receive_guarded(inner, IO_TIMEOUT))?;
        if !authenticates(&authentication, &capability, &carrier) {
            return Err(Error::Authentication);
        }
        crate::metrics::record_runner_owned(&crate::metrics::runner::Owned {
            runner_pid,
            runner_uid: rustix::process::geteuid().as_raw(),
            build_request: *inner.manifest.generated.build_request_key().as_bytes(),
            generated_manifest: inner.manifest.generated.digest(),
            generation_key: *inner.key.as_bytes(),
            epoch: inner.epoch.get(),
            artifact: inner.manifest.native_identity(),
            executable_digest: inner.manifest.artifact().executable_digest(),
            executable_bytes: inner.manifest.artifact().executable_bytes(),
            executable_path: inner
                .manifest
                .artifact()
                .executable()
                .as_os_str()
                .as_bytes()
                .into(),
            spawned_ns: evidence_spawned_ns,
        });
        Ok(Candidate {
            inner: Some(inner),
            _phase: Spawned { _private: () },
        })
    }

    #[cfg(not(unix))]
    pub fn spawn(
        self,
        _epoch: crate::supervisor::Epoch,
        _process: &Owner,
    ) -> Result<Candidate<Spawned, B>, Error> {
        let _ = self;
        Err(Error::Unsupported)
    }
}

impl<B> Candidate<Spawned, B> {
    /// Send exact snapshot bytes and require an equal-key preparation receipt.
    pub fn prepare(
        mut self,
        snapshot: crate::supervisor::snapshot::Bytes,
    ) -> Result<Candidate<Prepared, B>, Error> {
        let expected = self
            .inner
            .as_ref()
            .ok_or(Error::Consumed)?
            .manifest
            .request
            .snapshot;
        if !snapshot.matches(expected) {
            return Err(Error::Snapshot(
                crate::supervisor::snapshot::Error::Mismatch,
            ));
        }
        let mut inner = self.take()?;
        let key = *inner.key.as_bytes();
        let nonce = transition_nonce()?;
        let mut payload = Vec::from(inner.key.as_bytes().as_slice());
        payload.extend_from_slice(&nonce);
        payload.extend_from_slice(inner.manifest.profile.application_semantic());
        payload.extend_from_slice(&snapshot.into_inner());
        let mut expected = Vec::from(key);
        expected.extend_from_slice(&nonce);
        work_exchange(
            &mut inner,
            crate::protocol::Tag::Prepare,
            payload,
            crate::protocol::Tag::Prepared,
            &expected,
            ProgressState::new(key, nonce, WorkOperation::Prepare),
        )?;
        crate::metrics::record_milestone(crate::metrics::Milestone::Prepared);
        Ok(Candidate {
            inner: Some(inner),
            _phase: Prepared { _private: () },
        })
    }
}

impl<B> Candidate<Prepared, B> {
    /// Consume a generation-bound capability and require its exact receipt.
    pub fn activate(
        mut self,
        capability: crate::supervisor::activation::Capability,
    ) -> Result<Candidate<Activated, B>, Error> {
        let mut inner = self.take()?;
        let (capability, receipt) = capability.consume(inner.key)?;
        let route = crate::profile::activation::Route::new(inner.key, receipt);
        let mut payload = Vec::from(inner.key.as_bytes().as_slice());
        payload.extend_from_slice(&capability);
        payload.extend_from_slice(receipt.as_bytes());
        let mut expected = Vec::from(inner.key.as_bytes().as_slice());
        expected.extend_from_slice(receipt.as_bytes());
        exchange(
            &mut inner,
            crate::protocol::Tag::Activate,
            payload,
            crate::protocol::Tag::Activated,
            &expected,
        )?;
        crate::metrics::record_milestone(crate::metrics::Milestone::Activated);
        inner.route = Some(route);
        Ok(Candidate {
            inner: Some(inner),
            _phase: Activated { _private: () },
        })
    }
}

impl<B> Candidate<Activated, B> {
    /// Acknowledge the atomic routing swap for this exact generation.
    pub fn admit(mut self) -> Result<Candidate<Admitted, B>, Error> {
        let mut inner = self.take()?;
        let route = inner.route.ok_or(Error::MissingRoute)?;
        if route.key() != inner.key {
            return Err(Error::Stale);
        }
        let key = *inner.key.as_bytes();
        let nonce = transition_nonce()?;
        let mut payload = Vec::from(key);
        payload.extend_from_slice(&nonce);
        let mut expected = Vec::from(key);
        expected.extend_from_slice(&nonce);
        work_exchange(
            &mut inner,
            crate::protocol::Tag::Admit,
            payload,
            crate::protocol::Tag::Admitted,
            &expected,
            ProgressState::new(key, nonce, WorkOperation::Admit),
        )?;
        crate::metrics::record_milestone(crate::metrics::Milestone::Admitted);
        Ok(Candidate {
            inner: Some(inner),
            _phase: Admitted { _private: () },
        })
    }
}

impl<B> Candidate<Admitted, B> {
    pub(crate) fn restore_prepared_read_set(
        &mut self,
        read_set: crate::node::read_set::Prepared,
    ) -> Result<(), crate::node::Error> {
        self.inner
            .as_mut()
            .ok_or(crate::node::Error::State(
                "admitted runner lost its manifest before read-set retention",
            ))?
            .manifest
            .restore_prepared_read_set(read_set)
    }

    pub(crate) fn pin_live_last_good(&self) -> Result<(), crate::node::Error> {
        self.inner
            .as_ref()
            .ok_or(crate::node::Error::State(
                "admitted runner lost its manifest before pin promotion",
            ))?
            .manifest
            .pin_live_last_good()
    }

    /// Establish child observation before final parent validation and startup.
    pub fn observe_initial(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let inner = self.inner.as_mut().ok_or(Error::Consumed)?;
        if inner.started
            || inner.observed
            || !matches!(inner.lifecycle, Lifecycle::Run | Lifecycle::Preview)
        {
            return Err(Error::Lifecycle);
        }
        let key = *inner.key.as_bytes();
        let payload = observation_payload(&key, bytes)?;
        exchange(
            inner,
            crate::protocol::Tag::ObserveInitial,
            payload,
            crate::protocol::Tag::InitialObserved,
            &key,
        )?;
        inner.observed = true;
        // The child request is already captured and its observer can revoke it
        // throughout this full validation and the remaining startup/pin work.
        inner.manifest.revalidate_prepared()?;
        Ok(())
    }

    /// Start the already-admitted live application or preview lifecycle.
    pub fn start(&mut self) -> Result<(), Error> {
        let inner = self.inner.as_mut().ok_or(Error::Consumed)?;
        if inner.started || !matches!(inner.lifecycle, Lifecycle::Run | Lifecycle::Preview) {
            return Err(Error::Lifecycle);
        }
        let key = *inner.key.as_bytes();
        // A runner spawned while the one it replaces had the terminal gets
        // it now, once that one has ended.
        inner.child.claim_terminal();
        exchange(
            inner,
            crate::protocol::Tag::Start,
            key.to_vec(),
            crate::protocol::Tag::Started,
            &key,
        )?;
        inner.started = true;
        crate::metrics::record_milestone(crate::metrics::Milestone::Started);
        Ok(())
    }

    /// Execute one admitted deterministic snapshot mode and return only its
    /// bounded profile-owned outcome bytes.
    pub fn snapshot(&mut self) -> Result<Box<[u8]>, Error> {
        let inner = self.inner.as_mut().ok_or(Error::Consumed)?;
        if inner.started
            || !matches!(
                inner.lifecycle,
                Lifecycle::SnapshotCheck | Lifecycle::SnapshotUpdate
            )
        {
            return Err(Error::Lifecycle);
        }
        let key = *inner.key.as_bytes();
        let nonce = transition_nonce()?;
        let mut payload = Vec::from(key);
        payload.extend_from_slice(&nonce);
        let response = match work_request(
            inner,
            crate::protocol::Tag::Snapshot,
            payload,
            ProgressState::new(key, nonce, WorkOperation::Snapshot),
        ) {
            Ok(response) => response,
            Err(error) => return Err(terminal_owner_loss(inner, error)),
        };
        snapshot_outcome(response, &key, &nonce)
    }

    /// Publish one bounded rendered diagnostic to a live last-good owner.
    pub fn report(&mut self, report: &str) -> Result<(), Error> {
        let inner = self.inner.as_mut().ok_or(Error::Consumed)?;
        if !inner.started {
            return Err(Error::Lifecycle);
        }
        let mut payload = Vec::from(inner.key.as_bytes().as_slice());
        payload.extend_from_slice(report.as_bytes());
        let key = *inner.key.as_bytes();
        exchange(
            inner,
            crate::protocol::Tag::Report,
            payload,
            crate::protocol::Tag::Report,
            &key,
        )
    }

    /// Observe and reap a naturally closed live window without signalling it.
    pub fn reap_if_exited(&mut self) -> Result<bool, Error> {
        let inner = self.inner.as_mut().ok_or(Error::Consumed)?;
        if inner.reaped {
            return Ok(true);
        }
        let Some(status) = inner.child.try_wait().map_err(resource_error)? else {
            return Ok(false);
        };
        inner.reaped = true;
        join_logs(inner)?;
        if status.success() {
            Ok(true)
        } else {
            // A program a signal ended gives the status a shell gives it.
            #[cfg(unix)]
            let code = {
                use std::os::unix::process::ExitStatusExt as _;
                status
                    .code()
                    .or_else(|| status.signal().map(|signal| 128 + signal))
            };
            #[cfg(not(unix))]
            let code = status.code();
            Err(Error::Exit(code))
        }
    }

    /// Invoke one exact compiled adapter with a closed move-only value payload.
    pub fn invoke(
        &mut self,
        entry: crate::runner::manifest::Entry,
        arguments: Box<[super::value::Value]>,
    ) -> Result<super::value::Value, Error> {
        let inner = self.inner.as_mut().ok_or(Error::Consumed)?;
        let expected = usize::try_from(entry.arguments()).map_err(|_| {
            Error::Invocation(super::invoke::Error::Cardinality {
                expected: usize::MAX,
                actual: arguments.len(),
            })
        })?;
        if arguments.len() != expected {
            return Err(Error::Invocation(super::invoke::Error::Cardinality {
                expected,
                actual: arguments.len(),
            }));
        }
        if !inner
            .manifest
            .generated
            .entries()
            .any(|candidate| candidate == &entry)
        {
            return Err(Error::InvokeRejected(
                super::invoke::Rejection::MissingAdapter,
            ));
        }
        let mut payload = Vec::new();
        let capacity = 100_usize
            .checked_add(arguments.len())
            .ok_or(super::invoke::Error::Limit)?;
        payload
            .try_reserve_exact(capacity)
            .map_err(|_| super::invoke::Error::Allocation)?;
        payload.extend_from_slice(inner.key.as_bytes());
        payload.extend_from_slice(entry.identity());
        payload.extend_from_slice(entry.signature());
        payload.extend_from_slice(&entry.arguments().to_le_bytes());
        for argument in arguments {
            argument.encode(&mut payload)?;
        }
        crate::protocol::send(
            &mut inner.stream,
            &crate::protocol::Frame::new(crate::protocol::Tag::Invoke, payload.into_boxed_slice())?,
        )?;
        let response = crate::protocol::receive(&mut inner.stream)?;
        if response.payload().len() < 32 || &response.payload()[..32] != inner.key.as_bytes() {
            return Err(Error::Stale);
        }
        match response.tag() {
            crate::protocol::Tag::Invoked => {
                let mut input = &response.payload()[32..];
                let value = super::value::Value::decode(&mut input)?;
                if !input.is_empty() {
                    return Err(Error::Invocation(super::invoke::Error::Malformed));
                }
                Ok(value)
            }
            crate::protocol::Tag::InvokeRejected if response.payload().len() == 33 => {
                let rejection = super::invoke::Rejection::decode(response.payload()[32])
                    .ok_or(Error::Invocation(super::invoke::Error::Malformed))?;
                Err(Error::InvokeRejected(rejection))
            }
            crate::protocol::Tag::Authenticate
            | crate::protocol::Tag::Hello
            | crate::protocol::Tag::Prepare
            | crate::protocol::Tag::Prepared
            | crate::protocol::Tag::Activate
            | crate::protocol::Tag::Activated
            | crate::protocol::Tag::Admit
            | crate::protocol::Tag::Admitted
            | crate::protocol::Tag::Compile
            | crate::protocol::Tag::HotCommit
            | crate::protocol::Tag::HotAbort
            | crate::protocol::Tag::HotAborted
            | crate::protocol::Tag::HotConsumedRejected
            | crate::protocol::Tag::HotBegin
            | crate::protocol::Tag::HotBegan
            | crate::protocol::Tag::HotCancel
            | crate::protocol::Tag::HotCancelled
            | crate::protocol::Tag::HotObserve
            | crate::protocol::Tag::HotObserved
            | crate::protocol::Tag::ObserveInitial
            | crate::protocol::Tag::InitialObserved
            | crate::protocol::Tag::ResidentCompile
            | crate::protocol::Tag::ResidentPrepared
            | crate::protocol::Tag::ResidentMiss
            | crate::protocol::Tag::ResidentDiagnostic
            | crate::protocol::Tag::ResidentObsolete
            | crate::protocol::Tag::ResidentResource
            | crate::protocol::Tag::RefreshPoll
            | crate::protocol::Tag::RefreshStatus
            | crate::protocol::Tag::SurfaceMiss
            | crate::protocol::Tag::Report
            | crate::protocol::Tag::Shutdown
            | crate::protocol::Tag::Exited
            | crate::protocol::Tag::ProfileMessage
            | crate::protocol::Tag::ProfileReply
            | crate::protocol::Tag::ProfileRejected
            | crate::protocol::Tag::Invoke
            | crate::protocol::Tag::InvokeRejected
            | crate::protocol::Tag::Start
            | crate::protocol::Tag::Started
            | crate::protocol::Tag::Snapshot
            | crate::protocol::Tag::SnapshotOutcome
            | crate::protocol::Tag::WorkProgress
            | crate::protocol::Tag::Account
            | crate::protocol::Tag::ExportState
            | crate::protocol::Tag::StateExported
            | crate::protocol::Tag::SeedState
            | crate::protocol::Tag::StateSeeded => Err(Error::Stale),
        }
    }

    /// Validate a queued profile envelope against the current exact route.
    pub fn validate_message<M>(
        &self,
        envelope: &crate::profile::message::Envelope<M>,
    ) -> Result<(), Error> {
        let inner = self.inner.as_ref().ok_or(Error::Consumed)?;
        let route = inner.route.ok_or(Error::MissingRoute)?;
        if route != envelope.route() || route.key() != inner.key {
            return Err(Error::Stale);
        }
        Ok(())
    }

    /// Send one bounded authenticated profile message through the admitted route.
    pub fn message<M: crate::profile::message::Message>(
        &mut self,
        message: M,
    ) -> Result<crate::profile::message::Envelope<M>, Error> {
        let inner = self.inner.as_mut().ok_or(Error::Consumed)?;
        if !inner.manifest.profile.has_generated_projections() {
            return Err(Error::NoProfile);
        }
        let route = inner.route.ok_or(Error::MissingRoute)?;
        if route.key() != inner.key {
            return Err(Error::Stale);
        }
        let sequence = inner.sequence;
        let payload = crate::profile::message::Envelope::new(route, sequence, message).encode()?;
        crate::protocol::send(
            &mut inner.stream,
            &crate::protocol::Frame::new(crate::protocol::Tag::ProfileMessage, payload)?,
        )?;
        let response = crate::protocol::receive(&mut inner.stream)?;
        if response.tag() == crate::protocol::Tag::ProfileRejected {
            return Err(profile_rejected(response.payload()));
        }
        if response.tag() != crate::protocol::Tag::ProfileReply {
            return Err(Error::Stale);
        }
        let envelope =
            crate::profile::message::Envelope::decode(route, sequence, response.payload())?;
        inner.sequence = sequence.next()?;
        Ok(envelope)
    }

    /// Hot-admit one exact same-surface snapshot without acquiring Cargo or
    /// replacing the current process. A miss returns every cold-build input.
    pub fn compile(
        &mut self,
        request: crate::bridge::GenerationRequestKey<B>,
        snapshot: crate::supervisor::snapshot::Bytes,
    ) -> Result<super::hot::Outcome<B>, Error>
    where
        B: crate::surface::Runner,
    {
        match self.prepare_compile(request, snapshot)? {
            super::hot::Preparation::Prepared(prepared) => {
                self.commit_compile(prepared).map(super::hot::Outcome::Hit)
            }
            super::hot::Preparation::Miss(miss) => Ok(super::hot::Outcome::Miss(miss)),
        }
    }

    /// Capture child authority before any mutable preparation in the parent.
    pub fn begin_compile(&mut self) -> Result<super::hot::Started, Error> {
        let inner = self.inner.as_mut().ok_or(Error::Consumed)?;
        let sequence = inner.hot_sequence.checked_add(1).ok_or(Error::Stale)?;
        let started = super::hot::Started {
            previous: inner.key,
            nonce: transition_nonce()?,
            sequence,
            live: inner.started && inner.manifest.profile.has_generated_projections(),
        };
        if started.live {
            let identity = started.identity();
            if let Err(error) = exchange(
                inner,
                crate::protocol::Tag::HotBegin,
                identity.to_vec(),
                crate::protocol::Tag::HotBegan,
                &identity,
            ) {
                return Err(terminal_owner_loss(inner, error));
            }
        }
        inner.hot_sequence = sequence;
        Ok(started)
    }

    /// Cancel an early request whose native/source preparation did not finish.
    pub fn cancel_compile(&mut self, started: super::hot::Started) -> Result<(), Error> {
        let inner = self.inner.as_mut().ok_or(Error::Consumed)?;
        cancel_started(inner, &started)
    }

    /// Convenience entry for callers that already own complete inputs.
    pub fn prepare_compile(
        &mut self,
        request: crate::bridge::GenerationRequestKey<B>,
        snapshot: crate::supervisor::snapshot::Bytes,
    ) -> Result<super::hot::Preparation<B>, Error>
    where
        B: crate::surface::Runner,
    {
        let started = self.begin_compile()?;
        self.prepare_started_compile(started, request, snapshot, None)
    }

    /// Bind complete inputs to the original child request without refreshing it.
    /// Every failure before child preparation cancels that exact early request.
    pub fn prepare_started_compile(
        &mut self,
        started: super::hot::Started,
        request: crate::bridge::GenerationRequestKey<B>,
        snapshot: crate::supervisor::snapshot::Bytes,
        observation: Option<&[u8]>,
    ) -> Result<super::hot::Preparation<B>, Error>
    where
        B: crate::surface::Runner,
    {
        let inner = self.inner.as_mut().ok_or(Error::Consumed)?;
        if inner.key != started.previous || inner.hot_sequence != started.sequence {
            return Err(Error::Stale);
        }
        let outcome = (|| {
            if inner.manifest.generated.build_request_key() != request.build_request_key() {
                return Ok(super::hot::Preparation::Miss(super::hot::Miss::new(
                    request, snapshot,
                )));
            }
            if !snapshot.matches(request.snapshot()) {
                return Err(Error::Snapshot(
                    crate::supervisor::snapshot::Error::Mismatch,
                ));
            }
            if started.live
                && let Some(bytes) = observation
            {
                let identity = started.identity();
                let payload = observation_payload(&identity, bytes)?;
                exchange(
                    inner,
                    crate::protocol::Tag::HotObserve,
                    payload,
                    crate::protocol::Tag::HotObserved,
                    &identity,
                )?;
            }
            let next = crate::supervisor::GenerationKey::seal(
                request.build_request_key(),
                request.snapshot(),
                request.canonical_bytes(),
                request.revision(),
                inner.epoch,
                &inner.manifest.native_identity(),
            );
            let next_route = inner.route.map(|route| route.advance(next));
            let nonce = started.nonce;
            let mut payload = Vec::from(next.as_bytes().as_slice());
            payload.extend_from_slice(request.surface_fingerprint().as_bytes());
            if inner.manifest.profile.has_generated_projections() {
                let route = next_route.ok_or(Error::MissingRoute)?;
                payload.extend_from_slice(route.receipt().as_bytes());
            } else {
                payload.extend_from_slice(&[0; 32]);
            }
            payload.extend_from_slice(&nonce);
            payload.extend_from_slice(request.application_semantic());
            let limits = request.shared_limits()?;
            let mut manifest = request.generate(&limits)?;
            let mut read_set = manifest.replay_hot(&inner.manifest)?;
            read_set.bind_snapshot(&snapshot)?;
            read_set.pin_prepared()?;
            payload.extend_from_slice(&snapshot.into_inner());
            let response = match work_request(
                inner,
                crate::protocol::Tag::Compile,
                payload,
                ProgressState::new(*next.as_bytes(), nonce, WorkOperation::Compile),
            ) {
                Ok(response) => response,
                Err(error) => return Err(terminal_owner_loss(inner, error)),
            };
            if response.tag() == crate::protocol::Tag::ProfileRejected {
                return Err(profile_rejected(response.payload()));
            }
            let mut expected = Vec::from(next.as_bytes().as_slice());
            expected.extend_from_slice(&nonce);
            if response.tag() != crate::protocol::Tag::Prepared || response.payload() != expected {
                return Err(terminal_owner_loss(inner, Error::Stale));
            }
            Ok(super::hot::Preparation::Prepared(super::hot::Prepared {
                previous: inner.key,
                next,
                nonce,
                route: next_route,
                manifest,
                read_set,
            }))
        })();
        match outcome {
            Ok(super::hot::Preparation::Prepared(prepared)) => {
                Ok(super::hot::Preparation::Prepared(prepared))
            }
            Ok(miss) => {
                cancel_started(inner, &started)?;
                Ok(miss)
            }
            Err(failure) if failure.is_owner_lost() => Err(failure),
            Err(failure) => match cancel_started(inner, &started) {
                Ok(()) => Err(failure),
                Err(cleanup) => Err(Error::HotCleanup {
                    failure: Box::new(failure),
                    cleanup: Box::new(cleanup),
                }),
            },
        }
    }

    /// Commit one externally admitted hot preparation into the live runner.
    pub fn commit_compile(
        &mut self,
        prepared: super::hot::Prepared<B>,
    ) -> Result<crate::supervisor::GenerationKey, Error> {
        let inner = self.inner.as_mut().ok_or(Error::Consumed)?;
        if inner.key != prepared.previous {
            return Err(Error::Stale);
        }
        let mut identity = [0_u8; 48];
        identity[..32].copy_from_slice(prepared.next.as_bytes());
        identity[32..].copy_from_slice(&prepared.nonce);
        prepared.commit(|transition| {
            let result = match transition {
                // A program blocked reading the terminal draws again with
                // the generation it just got (D534.4).
                super::hot::Transition::Commit => {
                    hot_commit_exchange(inner, &identity).inspect(|()| inner.child.wake())
                }
                super::hot::Transition::Abort => exchange(
                    inner,
                    crate::protocol::Tag::HotAbort,
                    identity.to_vec(),
                    crate::protocol::Tag::HotAborted,
                    &identity,
                ),
            };
            result.map_err(|error| {
                if transition == super::hot::Transition::Commit && error.is_hot_consumed_rejection()
                {
                    error
                } else {
                    terminal_owner_loss(inner, error)
                }
            })
        })?;
        inner
            .manifest
            .accept_hot(prepared.manifest, prepared.read_set);
        inner.key = prepared.next;
        inner.route = prepared.route;
        inner.sequence = crate::profile::message::Sequence::first();
        Ok(prepared.next)
    }

    /// Drop one externally revoked hot preparation without changing the live
    /// generation.
    pub fn abort_compile(&mut self, prepared: super::hot::Prepared<B>) -> Result<(), Error> {
        let inner = self.inner.as_mut().ok_or(Error::Consumed)?;
        if inner.key != prepared.previous {
            return Err(Error::Stale);
        }
        abort_hot(inner, &prepared)
    }

    /// Whether its program has the terminal.
    #[must_use]
    pub fn has_terminal(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|inner| inner.child.has_terminal())
    }

    /// Shut down and reap this exact admitted process group.
    pub fn shutdown(self) -> Result<(), Error> {
        self.close(None)
    }

    /// Retire a replaced owner, preserving its acknowledged refresh request
    /// even if subsequent process cleanup fails.
    pub fn retire(self, refresh: &mut bool) -> Result<(), Error> {
        self.close(Some(refresh))
    }

    fn close(mut self, refresh: Option<&mut bool>) -> Result<(), Error> {
        let mut inner = self.take()?;
        if inner.reaped {
            return Ok(());
        }
        if let Some(refresh) =
            refresh.filter(|_| inner.started && inner.manifest.profile.owns_product_lifecycle())
        {
            resident::retire(&mut inner, refresh)?;
        } else {
            let key = *inner.key.as_bytes();
            exchange(
                &mut inner,
                crate::protocol::Tag::Shutdown,
                key.to_vec(),
                crate::protocol::Tag::Exited,
                &key,
            )?;
        }
        let status = inner.child.wait().map_err(resource_error)?;
        inner.reaped = true;
        if !status.success() {
            return Err(Error::Exit(status.code()));
        }
        join_logs(&mut inner)?;
        Ok(())
    }
}

impl<P, B> Candidate<P, B> {
    #[must_use]
    pub fn key(&self) -> crate::supervisor::GenerationKey {
        self.inner.as_ref().map_or_else(
            || unreachable!("candidate transition always retains inner ownership"),
            |inner| inner.key,
        )
    }

    pub(crate) fn revision(&self) -> crate::supervisor::Revision {
        self.inner.as_ref().map_or_else(
            || unreachable!("candidate transition always retains inner ownership"),
            |inner| inner.manifest.revision,
        )
    }

    fn take(&mut self) -> Result<Inner<B>, Error> {
        self.inner.take().ok_or(Error::Consumed)
    }
}

impl<P, B> Drop for Candidate<P, B> {
    fn drop(&mut self) {
        drop(self.inner.take());
    }
}

impl<B> Inner<B> {
    fn retire_before_reap(&mut self) {
        if self.route.is_none() {
            return;
        }
        let payload = Vec::from(self.key.as_bytes().as_slice()).into_boxed_slice();
        let key = *self.key.as_bytes();
        if exchange(
            self,
            crate::protocol::Tag::Shutdown,
            payload.into_vec(),
            crate::protocol::Tag::Exited,
            &key,
        )
        .is_err()
        {
            return;
        }
        let deadline = Instant::now() + IO_TIMEOUT;
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => {
                    self.reaped = true;
                    return;
                }
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(2)),
                Ok(None) | Err(_) => return,
            }
        }
    }
}

impl<B> Drop for Inner<B> {
    fn drop(&mut self) {
        if !self.reaped {
            self.retire_before_reap();
        }
        if !self.reaped {
            let _ = self.child.terminate();
            self.reaped = self.child.is_reaped();
        }
        if self.reaped {
            let _ = join_logs(self);
        }
    }
}

#[cfg(unix)]
fn accept(
    listener: &UnixListener,
    child: &mut super::ownership::Child,
) -> Result<UnixStream, Error> {
    accept_with_timeout(listener, child, CONNECT_TIMEOUT)
}

#[cfg(unix)]
fn accept_with_timeout(
    listener: &UnixListener,
    child: &mut super::ownership::Child,
    timeout: Duration,
) -> Result<UnixStream, Error> {
    let deadline = Instant::now() + timeout;
    loop {
        match listener.accept() {
            Ok((stream, _)) => return Ok(stream),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if child.try_wait().map_err(resource_error)?.is_some() {
                    return Err(Error::EarlyExit);
                }
                if Instant::now() >= deadline {
                    return Err(Error::ConnectTimeout);
                }
                thread::sleep(Duration::from_millis(2));
            }
            Err(source) => return Err(Error::ProtocolIo(source)),
        }
    }
}

fn authenticates(frame: &crate::protocol::Frame, capability: &[u8; 32], carrier: &[u8]) -> bool {
    frame.tag() == crate::protocol::Tag::Authenticate
        && frame.payload().len() >= capability.len()
        && frame.payload()[..capability.len()] == *capability
        && frame.payload()[capability.len()..] == *carrier
}

fn socket_path(root: &Path) -> Result<PathBuf, Error> {
    let socket = root.join(format!(
        "runner-{}-{}.sock",
        std::process::id(),
        SOCKET_ID.fetch_add(1, Ordering::Relaxed)
    ));
    if socket.as_os_str().len() > 100 {
        return Err(Error::SocketPath(socket));
    }
    Ok(socket)
}

fn exchange<B>(
    inner: &mut Inner<B>,
    send: crate::protocol::Tag,
    payload: Vec<u8>,
    receive: crate::protocol::Tag,
    expected: &[u8],
) -> Result<(), Error> {
    guarded(inner, |inner| {
        send_guarded(
            inner,
            &crate::protocol::Frame::new(send, payload.into_boxed_slice())?,
        )?;
        let response = receive_guarded(inner, IO_TIMEOUT)?;
        if response.tag() == crate::protocol::Tag::ProfileRejected {
            return Err(profile_rejected(response.payload()));
        }
        if response.tag() != receive || response.payload() != expected {
            return Err(Error::Stale);
        }
        Ok(())
    })
}

fn observation_payload<const N: usize>(identity: &[u8; N], bytes: &[u8]) -> Result<Vec<u8>, Error> {
    let length = identity
        .len()
        .checked_add(bytes.len())
        .ok_or(Error::Stale)?;
    if bytes.is_empty() {
        return Err(Error::Stale);
    }
    if length > crate::protocol::FRAME_BYTES - 5 {
        return Err(Error::Protocol(crate::protocol::Error::Oversized {
            limit: crate::protocol::FRAME_BYTES - 5 - identity.len(),
            actual: bytes.len(),
        }));
    }
    let mut payload = Vec::new();
    payload
        .try_reserve_exact(length)
        .map_err(|_| Error::Protocol(crate::protocol::Error::Allocation))?;
    payload.extend_from_slice(identity);
    payload.extend_from_slice(bytes);
    Ok(payload)
}

/// Cancellation acknowledges the exact early request or loses the owner.
fn cancel_started<B>(inner: &mut Inner<B>, started: &super::hot::Started) -> Result<(), Error> {
    if inner.key != started.previous || inner.hot_sequence != started.sequence {
        return Err(Error::Stale);
    }
    if !started.live {
        return Ok(());
    }
    let identity = started.identity();
    exchange(
        inner,
        crate::protocol::Tag::HotCancel,
        identity.to_vec(),
        crate::protocol::Tag::HotCancelled,
        &identity,
    )
    .map_err(|error| terminal_owner_loss(inner, error))
}

/// Commit has a narrower response family than generic profile work. A peer
/// that cannot prove consumed-but-not-installed must be treated as owner loss.
fn hot_commit_exchange<B>(inner: &mut Inner<B>, identity: &[u8; 48]) -> Result<(), Error> {
    guarded(inner, |inner| {
        send_guarded(
            inner,
            &crate::protocol::Frame::new(
                crate::protocol::Tag::HotCommit,
                Box::from(identity.as_slice()),
            )?,
        )?;
        hot_commit_outcome(receive_guarded(inner, IO_TIMEOUT)?, identity)
    })
}

fn hot_commit_outcome(response: crate::protocol::Frame, identity: &[u8; 48]) -> Result<(), Error> {
    match response.tag() {
        crate::protocol::Tag::Admitted if response.payload() == identity => Ok(()),
        crate::protocol::Tag::HotConsumedRejected => {
            let reason = response
                .payload()
                .strip_prefix(identity.as_slice())
                .ok_or(Error::Stale)?;
            let reason = std::str::from_utf8(reason).map_err(|_| Error::Stale)?;
            // Frame decoding already bounded the complete key/nonce/diagnostic.
            // The child sends no reason only when a newer event superseded it.
            Err(if reason.is_empty() {
                Error::HotSuperseded
            } else {
                Error::HotConsumedRejected(reason.into())
            })
        }
        _ => Err(Error::Stale),
    }
}

fn work_exchange<B>(
    inner: &mut Inner<B>,
    send: crate::protocol::Tag,
    payload: Vec<u8>,
    receive: crate::protocol::Tag,
    expected: &[u8],
    progress: ProgressState,
) -> Result<(), Error> {
    let response = work_request(inner, send, payload, progress)?;
    if response.tag() == crate::protocol::Tag::ProfileRejected {
        return Err(profile_rejected(response.payload()));
    }
    if response.tag() != receive || response.payload() != expected {
        return Err(Error::Stale);
    }
    Ok(())
}

fn work_request<B>(
    inner: &mut Inner<B>,
    send: crate::protocol::Tag,
    payload: Vec<u8>,
    mut progress: ProgressState,
) -> Result<crate::protocol::Frame, Error> {
    guarded(inner, |inner| {
        send_guarded(
            inner,
            &crate::protocol::Frame::new(send, payload.into_boxed_slice())?,
        )?;
        let mut partial_started = None;
        let mut decoder = crate::protocol::Decoder::new();
        loop {
            let before = Instant::now();
            let decoded = match decoder.receive_step(&mut inner.stream) {
                Ok(decoded) => decoded,
                Err(error) => return Err(work_protocol_error(inner, error)),
            };
            match decoded {
                Some(frame) => {
                    partial_started = None;
                    if frame.tag() == crate::protocol::Tag::WorkProgress {
                        progress.observe(frame.payload())?;
                        continue;
                    }
                    if inner
                        .accounting
                        .consume(inner.child.id(), inner.epoch.get(), &frame)
                    {
                        continue;
                    }
                    inner.accounting.finish(inner.child.id(), inner.epoch.get());
                    return Ok(frame);
                }
                // The read itself blocks up to `POLL_INTERVAL`; a header just
                // read is followed by its body at once, never after a sleep.
                None => {
                    if decoder.has_partial_frame() {
                        let partial_started = *partial_started.get_or_insert(before);
                        if partial_started.elapsed() >= IO_TIMEOUT {
                            return Err(Error::WorkFrameTimeout);
                        }
                    }
                }
            }
        }
    })
}

fn guarded<B, T>(
    inner: &mut Inner<B>,
    body: impl FnOnce(&mut Inner<B>) -> Result<T, Error>,
) -> Result<T, Error> {
    if let Err(source) = inner
        .stream
        .set_read_timeout(Some(POLL_INTERVAL))
        .and_then(|()| inner.stream.set_write_timeout(Some(POLL_INTERVAL)))
    {
        // Darwin refuses socket options once the peer is gone (EINVAL).
        return Err(exited_peer(inner, &source).unwrap_or(Error::ProtocolIo(source)));
    }
    let result = body(inner);
    // Missing evidence never changes the original product result, including a
    // failure before the child can send its terminal reply.
    inner.accounting.finish(inner.child.id(), inner.epoch.get());
    let restored = inner
        .stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .and_then(|()| inner.stream.set_write_timeout(Some(IO_TIMEOUT)));
    match (result, restored) {
        // The peer's exit is already proven with its status and stderr.
        (Err(error @ Error::WorkPeer { .. }), _) => Err(error),
        (Err(Error::Protocol(error)), _) => Err(match exited_peer(inner, &error) {
            Some(exited) => exited,
            None => terminal_owner_loss(inner, Error::Protocol(error)),
        }),
        (Err(error @ (Error::Resource(_) | Error::WorkFrameTimeout)), _) => {
            Err(terminal_owner_loss(inner, error))
        }
        (Ok(value), Err(source)) => match exited_peer(inner, &source) {
            Some(exited) => after_reply(value, exited),
            None => Err(terminal_owner_loss(inner, Error::ProtocolIo(source))),
        },
        (_, Err(source)) => Err(match exited_peer(inner, &source) {
            Some(exited) => exited,
            None => terminal_owner_loss(inner, Error::ProtocolIo(source)),
        }),
        (result, Ok(())) => result,
    }
}

/// The reply already completed the exchange. A runner that then exits
/// cleanly, as it does right after acknowledging a shutdown, leaves Darwin
/// refusing the timeout restore; that is not a failure of the reply.
fn after_reply<T>(value: T, exited: Error) -> Result<T, Error> {
    match exited {
        Error::WorkPeer {
            status: Some(0), ..
        } => Ok(value),
        exited => Err(exited),
    }
}

fn receive_guarded<B>(
    inner: &mut Inner<B>,
    timeout: Duration,
) -> Result<crate::protocol::Frame, Error> {
    let deadline = Instant::now() + timeout;
    let mut decoder = crate::protocol::Decoder::new();
    loop {
        if Instant::now() >= deadline {
            return Err(Error::WorkFrameTimeout);
        }
        if let Some(frame) = decoder.receive_step(&mut inner.stream)? {
            if inner
                .accounting
                .consume(inner.child.id(), inner.epoch.get(), &frame)
            {
                continue;
            }
            inner.accounting.finish(inner.child.id(), inner.epoch.get());
            return Ok(frame);
        }
    }
}

fn send_guarded<B>(inner: &mut Inner<B>, frame: &crate::protocol::Frame) -> Result<(), Error> {
    let deadline = Instant::now() + IO_TIMEOUT;
    let mut encoder = crate::protocol::Encoder::frame(frame)?;
    inner
        .accounting
        .begin(inner.child.id(), inner.epoch.get(), frame);
    loop {
        if Instant::now() >= deadline {
            return Err(Error::WorkFrameTimeout);
        }
        if encoder.send_step(&mut inner.stream)? {
            return Ok(());
        }
    }
}

fn resource_error(error: crate::Error) -> Error {
    Error::Resource(Box::new(error))
}

fn terminal_owner_loss<B>(inner: &mut Inner<B>, error: Error) -> Error {
    let mut reason = error.to_string();
    if !inner.reaped {
        if let Err(cleanup) = inner.child.terminate() {
            reason.push_str(&format!("; runner cleanup failed: {cleanup}"));
        }
        inner.reaped = inner.child.is_reaped();
    }
    let logs = if inner.reaped {
        join_logs(inner).err().map(|error| error.to_string())
    } else {
        Some("log readers remain owned until the process group is retired".into())
    };
    Error::OwnerLost {
        reason: match logs {
            Some(logs) => format!("{reason}; log retirement failed: {logs}").into_boxed_str(),
            None => reason.into_boxed_str(),
        },
    }
}

fn work_protocol_error<B>(inner: &mut Inner<B>, error: crate::protocol::Error) -> Error {
    exited_peer(inner, &error).unwrap_or(Error::Protocol(error))
}

/// Attribute a broken channel to the runner when it exits within the deadline,
/// keeping its status and stderr instead of an opaque socket failure.
fn exited_peer<B>(inner: &mut Inner<B>, failure: &dyn std::fmt::Display) -> Option<Error> {
    if inner.reaped {
        return None;
    }
    let status = match exit_within(&mut inner.child, IO_TIMEOUT)? {
        Ok(status) => status,
        Err(error) => return Some(error),
    };
    inner.reaped = true;
    let stderr = match take_logs(inner) {
        Ok((_, stderr)) => String::from_utf8_lossy(&stderr).into_owned(),
        Err(error) => format!("cannot retain bounded child logs: {error}"),
    };
    Some(Error::WorkPeer {
        status: status.code(),
        protocol: failure.to_string().into_boxed_str(),
        stderr: stderr.into_boxed_str(),
    })
}

fn exit_within(
    child: &mut super::ownership::Child,
    timeout: Duration,
) -> Option<Result<std::process::ExitStatus, Error>> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(Ok(status)),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(2)),
            Ok(None) => return None,
            Err(source) => return Some(Err(resource_error(source))),
        }
    }
}

fn abort_hot<B>(inner: &mut Inner<B>, prepared: &super::hot::Prepared<B>) -> Result<(), Error> {
    let mut payload = Vec::from(prepared.next.as_bytes().as_slice());
    payload.extend_from_slice(&prepared.nonce);
    exchange(
        inner,
        crate::protocol::Tag::HotAbort,
        payload.clone(),
        crate::protocol::Tag::HotAborted,
        &payload,
    )
    .map_err(|error| terminal_owner_loss(inner, error))
}

fn profile_rejected(payload: &[u8]) -> Error {
    Error::ProfileRejected(
        String::from_utf8_lossy(payload)
            .into_owned()
            .into_boxed_str(),
    )
}

fn snapshot_outcome(
    response: crate::protocol::Frame,
    key: &[u8; 32],
    nonce: &[u8; 16],
) -> Result<Box<[u8]>, Error> {
    if response.tag() == crate::protocol::Tag::ProfileRejected {
        return Err(profile_rejected(response.payload()));
    }
    if response.tag() != crate::protocol::Tag::SnapshotOutcome
        || response.payload().len() < key.len() + nonce.len()
        || response.payload()[..key.len()] != *key
        || response.payload()[key.len()..key.len() + nonce.len()] != *nonce
    {
        return Err(Error::Stale);
    }
    Ok(Box::from(&response.payload()[key.len() + nonce.len()..]))
}

fn random() -> Result<[u8; 32], Error> {
    let mut source = File::open("/dev/urandom").map_err(Error::ProtocolIo)?;
    let mut bytes = [0_u8; 32];
    source.read_exact(&mut bytes).map_err(Error::ProtocolIo)?;
    Ok(bytes)
}

fn transition_nonce() -> Result<[u8; 16], Error> {
    let random = random()?;
    random[..16]
        .try_into()
        .map_err(|_| Error::WorkProgress("nonce"))
}

fn hex(bytes: &[u8; 32]) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(64);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

/// Where a running program's output also goes as it arrives.
#[derive(Clone, Copy)]
enum Forward {
    Stdout,
    Stderr,
}

fn tail(mut reader: impl std::io::Read, forward: Option<Forward>) -> std::io::Result<Box<[u8]>> {
    use std::io::Write as _;
    let mut retained = Vec::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            return Ok(retained.into_boxed_slice());
        }
        // Best effort: a closed terminal never fails the runner.
        match forward {
            Some(Forward::Stdout) => {
                let mut out = std::io::stdout().lock();
                let _ = out.write_all(&buffer[..count]).and_then(|()| out.flush());
            }
            Some(Forward::Stderr) => {
                let _ = std::io::stderr().lock().write_all(&buffer[..count]);
            }
            None => {}
        }
        retained.extend_from_slice(&buffer[..count]);
        if retained.len() > crate::protocol::STREAM_TAIL_BYTES {
            let remove = retained.len() - crate::protocol::STREAM_TAIL_BYTES;
            retained.drain(..remove);
        }
    }
}

fn terminate(child: &mut super::ownership::Child) {
    let _ = child.terminate();
}

fn join_logs<B>(inner: &mut Inner<B>) -> Result<(), Error> {
    let _ = take_logs(inner)?;
    Ok(())
}

/// A child's stdout and stderr logs.
type Logs = (Box<[u8]>, Box<[u8]>);

fn take_logs<B>(inner: &mut Inner<B>) -> Result<Logs, Error> {
    fn take(
        reader: &mut Option<thread::JoinHandle<std::io::Result<Box<[u8]>>>>,
    ) -> Result<Box<[u8]>, Error> {
        match reader.take() {
            Some(reader) => reader
                .join()
                .map_err(|_| Error::LogPanic)?
                .map_err(Error::ProtocolIo),
            None => Ok(Box::default()),
        }
    }
    let stdout = take(&mut inner.stdout)?;
    let stderr = take(&mut inner.stderr)?;
    Ok((stdout, stderr))
}

/// Candidate launch, authentication, or state-transition failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("runner process failed: {0}")]
    Resource(#[source] Box<crate::Error>),
    #[error(transparent)]
    Node(#[from] crate::node::Error),
    #[error(transparent)]
    Generation(#[from] crate::bridge::request::Error),
    #[error("hot admission failed: {failure}; pin rollback also failed: {cleanup}")]
    PinCleanup {
        #[source]
        failure: Box<Error>,
        cleanup: crate::node::Error,
    },
    #[error("hot admission failed: {failure}; hot abort also failed: {cleanup}")]
    HotCleanup {
        #[source]
        failure: Box<Error>,
        cleanup: Box<Error>,
    },
    #[error("runner sockets are unsupported on this target")]
    Unsupported,
    #[error("runner socket path is unsafe or too long: {0}")]
    SocketPath(PathBuf),
    #[error("runner socket path has an unsupported shape: {0}")]
    SocketShape(PathBuf),
    #[error("runner socket directory mode must be 0700, observed {actual:o}")]
    SocketMode { actual: u32 },
    #[error("runner protocol socket mode must be 0600, observed {actual:o}")]
    ProtocolSocketMode { actual: u32 },
    #[error("runner I/O failed while attempting to {operation} at {path}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("runner protocol I/O failed")]
    ProtocolIo(#[source] std::io::Error),
    #[error(transparent)]
    Protocol(#[from] crate::protocol::Error),
    #[error(transparent)]
    Resident(#[from] crate::protocol::resident::Error),
    #[error(transparent)]
    Artifact(#[from] crate::runner::artifact::Error),
    #[error(transparent)]
    Manifest(#[from] crate::runner::manifest::Error),
    #[error(transparent)]
    Activation(#[from] crate::supervisor::activation::Error),
    #[error(transparent)]
    Snapshot(#[from] crate::supervisor::snapshot::Error),
    #[error("runner exited before connecting its protocol socket")]
    EarlyExit,
    #[error("runner did not connect before the protocol deadline")]
    ConnectTimeout,
    #[error("runner authentication capability or compiled manifest mismatched")]
    Authentication,
    #[error("runner response carried a stale tag, key, capability, or receipt")]
    Stale,
    #[error("runner semantic progress was invalid: {0}")]
    WorkProgress(&'static str),
    #[error("runner work began a frame but did not complete it before the frame deadline")]
    WorkFrameTimeout,
    #[error("generated owner was terminated after unproven work failure: {reason}")]
    OwnerLost { reason: Box<str> },
    #[error(
        "generated owner exited during authenticated work with status {status:?}: {protocol}; bounded stderr: {stderr}"
    )]
    WorkPeer {
        status: Option<i32>,
        protocol: Box<str>,
        stderr: Box<str>,
    },
    #[error("runner profile route is absent")]
    MissingRoute,
    #[error("runner lifecycle command does not match the sealed process mode")]
    Lifecycle,
    #[error("the admitted runner selected no profile")]
    NoProfile,
    #[error("no admitted runner owns the profile route")]
    NoCurrent,
    #[error("the generated owner rejected profile work: {0}")]
    ProfileRejected(Box<str>),
    #[error("the generated owner consumed hot preparation without installing: {0}")]
    HotConsumedRejected(Box<str>),
    #[error("a newer edit superseded the generated owner's hot preparation")]
    HotSuperseded,
    #[error(transparent)]
    Invocation(#[from] super::invoke::Error),
    #[error("generated adapter rejected invocation: {0}")]
    InvokeRejected(super::invoke::Rejection),
    #[error(transparent)]
    Message(#[from] crate::profile::message::Error),
    #[error("runner candidate state was already consumed")]
    Consumed,
    #[error("runner {0} log pipe is absent")]
    MissingLog(&'static str),
    #[error("runner log reader panicked")]
    LogPanic,
    #[error("runner exited with status {0:?}")]
    Exit(Option<i32>),
}

impl Error {
    pub(crate) fn is_hot_consumed_rejection(&self) -> bool {
        match self {
            Self::HotConsumedRejected(_) | Self::HotSuperseded => true,
            Self::PinCleanup { failure, .. } | Self::HotCleanup { failure, .. } => {
                failure.is_hot_consumed_rejection()
            }
            _ => false,
        }
    }
    pub(crate) fn is_owner_lost(&self) -> bool {
        match self {
            Self::OwnerLost { .. } => true,
            Self::PinCleanup { failure, .. } => failure.is_owner_lost(),
            Self::HotCleanup { failure, cleanup } => {
                failure.is_owner_lost() || cleanup.is_owner_lost()
            }
            _ => false,
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn authentication_is_exact_missing_and_per_spawn() {
        let capability = [7_u8; 32];
        let next_capability = [8_u8; 32];
        let carrier = b"exact compiled manifest";
        let mut payload = Vec::from(capability);
        payload.extend_from_slice(carrier);
        let frame = crate::protocol::Frame::new(
            crate::protocol::Tag::Authenticate,
            payload.into_boxed_slice(),
        )
        .expect("the authentication frame is bounded");
        assert!(authenticates(&frame, &capability, carrier));
        assert!(!authenticates(&frame, &next_capability, carrier));
        assert!(!authenticates(&frame, &capability, b"stale manifest"));

        let missing = crate::protocol::Frame::new(
            crate::protocol::Tag::Authenticate,
            Box::from(&capability[..31]),
        )
        .expect("the missing capability is still a bounded frame");
        assert!(!authenticates(&missing, &capability, carrier));
    }

    #[test]
    fn snapshot_outcome_preserves_profile_rejection_and_exact_key() {
        let key = [9_u8; 32];
        let nonce = [7_u8; 16];
        let rejection = crate::protocol::Frame::new(
            crate::protocol::Tag::ProfileRejected,
            Box::from(&b"snapshot failed"[..]),
        )
        .expect("the rejection is bounded");
        assert!(matches!(
            snapshot_outcome(rejection, &key, &nonce),
            Err(Error::ProfileRejected(reason)) if reason.as_ref() == "snapshot failed"
        ));

        let mut payload = Vec::from(key);
        payload.extend_from_slice(&nonce);
        payload.extend_from_slice(b"outcome");
        let outcome = crate::protocol::Frame::new(
            crate::protocol::Tag::SnapshotOutcome,
            payload.into_boxed_slice(),
        )
        .expect("the outcome is bounded");
        assert_eq!(
            snapshot_outcome(outcome, &key, &nonce)
                .expect("the key matches")
                .as_ref(),
            b"outcome"
        );

        let stale = crate::protocol::Frame::new(
            crate::protocol::Tag::SnapshotOutcome,
            Box::from(&[8_u8; 32][..]),
        )
        .expect("the stale outcome is bounded");
        assert!(matches!(
            snapshot_outcome(stale, &key, &nonce),
            Err(Error::Stale)
        ));
    }

    #[test]
    fn progress_is_exact_closed_and_bounded() {
        let key = [3_u8; 32];
        let nonce = [4_u8; 16];
        let mut progress = ProgressState::new(key, nonce, WorkOperation::Compile);
        let frame = |ordinal, stage| {
            let mut payload = Vec::from(key);
            payload.extend_from_slice(&nonce);
            payload.extend_from_slice(&[WorkOperation::Compile as u8, ordinal, stage]);
            payload
        };
        progress.observe(&frame(1, 1)).expect("snapshot stage");
        progress.observe(&frame(2, 4)).expect("generation stage");
        progress.observe(&frame(3, 9)).expect("installation stage");
        assert!(matches!(
            progress.observe(&frame(3, 9)),
            Err(Error::WorkProgress("ordinal"))
        ));

        let mut wrong = frame(4, 9);
        wrong[32] ^= 1;
        assert!(matches!(
            progress.observe(&wrong),
            Err(Error::WorkProgress("identity"))
        ));
        let mut excess = ProgressState::new(key, nonce, WorkOperation::Compile);
        excess.ordinal = WORK_PROGRESS_LIMIT;
        assert!(matches!(
            excess.observe(&frame(WORK_PROGRESS_LIMIT.saturating_add(1), 9)),
            Err(Error::WorkProgress("cardinality"))
        ));
    }

    #[test]
    fn socket_owner_is_private_bounded_and_independent_of_project_depth() {
        let root = temporary("socket-safety");
        let project = root.join("x".repeat(128));
        fs::create_dir_all(&project).expect("the deep socket test project exists");
        let owner = Owner::new(&project).expect("the private socket owner binds");
        let mode = fs::symlink_metadata(&owner.root)
            .expect("the socket owner is inspectable")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700);
        assert!(!owner.root.starts_with(&project));
        assert!(socket_path(&owner.root).is_ok());
        assert!(matches!(
            socket_path(&root.join("x".repeat(128))),
            Err(Error::SocketPath(_))
        ));
        let socket_root = owner.root.clone();
        drop(owner);
        assert!(!socket_root.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn accept_timeout_and_child_death_are_bounded_and_reaped() {
        let root = temporary("accept");
        fs::create_dir(&root).expect("the accept test root exists");
        let socket = root.join("runner.sock");
        let listener = UnixListener::bind(&socket).expect("the test listener binds");
        listener
            .set_nonblocking(true)
            .expect("the test listener is nonblocking");
        let mut sleeper = Command::new("/bin/sh");
        sleeper.args(["-c", "sleep 30"]).process_group(0);
        let mut sleeper = super::super::ownership::Child::spawn(&mut sleeper)
            .expect("the isolated sleeper starts");
        assert!(matches!(
            accept_with_timeout(&listener, &mut sleeper, Duration::from_millis(10)),
            Err(Error::ConnectTimeout)
        ));
        terminate(&mut sleeper);
        assert!(sleeper
            .try_wait()
            .expect("the timed-out child is inspectable")
            .is_some());

        let mut exited = Command::new("/bin/sh");
        exited.args(["-c", "exit 0"]).process_group(0);
        let mut exited = super::super::ownership::Child::spawn(&mut exited)
            .expect("the short-lived child starts");
        assert!(matches!(
            accept_with_timeout(&listener, &mut exited, Duration::from_secs(1)),
            Err(Error::EarlyExit)
        ));
        terminate(&mut exited);
        assert!(exited
            .try_wait()
            .expect("the early-exit child is inspectable")
            .is_some());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn runner_exit_explains_a_socket_that_refuses_options() {
        let (parent, runner) = UnixStream::pair().expect("the protocol pair opens");
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "exit 3"])
            .stdin(Stdio::from(std::os::fd::OwnedFd::from(runner)))
            .process_group(0);
        let mut child = super::super::ownership::Child::spawn(&mut command)
            .expect("the runner stand-in starts");
        drop(command);
        let status = exit_within(&mut child, Duration::from_secs(5))
            .expect("the runner exits within the deadline")
            .expect("the exit is observable");
        assert_eq!(status.code(), Some(3));
        // Darwin rejects socket options once the peer has gone (EINVAL); this
        // is the failure that must be attributed to the exit above.
        #[cfg(target_os = "macos")]
        assert!(parent.set_read_timeout(Some(POLL_INTERVAL)).is_err());
        drop(parent);
    }

    #[test]
    fn acknowledged_shutdown_stands_when_the_runner_exits_before_timeout_restore() {
        use std::io::Read as _;

        let (mut parent, runner) = UnixStream::pair().expect("the protocol pair opens");
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "printf ack >&0"])
            .stdin(Stdio::from(std::os::fd::OwnedFd::from(runner)))
            .process_group(0);
        let mut child = super::super::ownership::Child::spawn(&mut command)
            .expect("the runner stand-in starts");
        drop(command);
        let mut reply = [0; 3];
        parent
            .read_exact(&mut reply)
            .expect("the final acknowledgement arrives");
        let status = exit_within(&mut child, Duration::from_secs(5))
            .expect("the runner exits after acknowledging")
            .expect("the exit is observable");
        assert!(status.success());
        #[cfg(target_os = "macos")]
        assert!(parent.set_read_timeout(Some(IO_TIMEOUT)).is_err());
        let exited = |status| Error::WorkPeer {
            status,
            protocol: "Invalid argument (os error 22)".into(),
            stderr: "rawr runner stderr log".into(),
        };
        assert_eq!(
            after_reply(reply, exited(status.code())).expect("the reply stands"),
            *b"ack"
        );
        assert!(matches!(
            after_reply(reply, exited(Some(3))),
            Err(Error::WorkPeer {
                status: Some(3),
                ..
            })
        ));
    }

    #[test]
    fn stdout_and_stderr_bytes_remain_bounded_non_protocol_logs() {
        let mut bytes = vec![0x55; crate::protocol::STREAM_TAIL_BYTES + 37];
        bytes[..9].copy_from_slice(&[0, 0, 0, 5, 0, 0, 0, 1, 12]);
        let retained = tail(Cursor::new(bytes), None).expect("the log stream drains");
        assert_eq!(retained.len(), crate::protocol::STREAM_TAIL_BYTES);
        assert!(retained.iter().all(|byte| *byte == 0x55));
    }

    fn temporary(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "rawr-runner-{label}-{}-{}",
            std::process::id(),
            SOCKET_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }
}

#[cfg(all(test, unix))]
mod hot_outcome_tests;
