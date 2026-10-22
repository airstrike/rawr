//! Authenticated fresh-process evidence for the private baseline feature.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Read as _;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{FileTypeExt as _, PermissionsExt as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::recorder::Recorder;
use super::{Milestone, NodeEvent, NodeKind, ProcessKind, Scope, Sink};

pub mod capsule;
mod frame;
pub mod process;
pub mod runner;
pub mod title;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const IO_TIMEOUT: Duration = Duration::from_secs(10);
const OWNER_ENVELOPE_NS: u64 = 30 * 60 * 1_000_000_000;
const REPORT_BYTES: usize = 8 * 1024 * 1024;
const COUNTER_LIMIT: u64 = 1_000_000;
const CHALLENGE_SCHEMA: &str = "rawr.phase9.evidence.challenge/v1";
static SOCKET_ID: AtomicU64 = AtomicU64::new(1);

/// Closed role of one sample in the governed warm launch series.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Trial {
    Warmup,
    Measured,
}

/// Absolute owner/evidence envelope derived once from the pre-spawn clock origin.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Envelope {
    origin_ns: u64,
    deadline_ns: u64,
}

impl Envelope {
    /// Transform the exact pre-spawn origin into the fixed thirty-minute envelope.
    pub fn transform(origin_ns: u64) -> Result<Self, Error> {
        if origin_ns == 0 {
            return Err(Error::Clock);
        }
        let deadline_ns = origin_ns
            .checked_add(OWNER_ENVELOPE_NS)
            .ok_or(Error::Clock)?;
        Ok(Self {
            origin_ns,
            deadline_ns,
        })
    }

    #[must_use]
    pub const fn origin_ns(self) -> u64 {
        self.origin_ns
    }

    #[must_use]
    pub const fn deadline_ns(self) -> u64 {
        self.deadline_ns
    }
}

/// Fixed parent-to-child launch challenge.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Challenge {
    schema: Box<str>,
    pub launch_id: [u8; 32],
    pub t0_ns: u64,
    pub trial: Trial,
    pub ordinal: u8,
    pub fixture_digest: [u8; 32],
    pub command_digest: [u8; 32],
    pub executable_digest: [u8; 32],
    pub title: title::Identity,
    pub title_digest: [u8; 32],
}

impl Challenge {
    /// Seal every immutable fact known before the measured process is spawned.
    pub fn transform(
        launch_id: [u8; 32],
        t0_ns: u64,
        trial: Trial,
        ordinal: u8,
        fixture_digest: [u8; 32],
        command_digest: [u8; 32],
        executable_digest: [u8; 32],
        title: title::Identity,
    ) -> Result<Self, Error> {
        Envelope::transform(t0_ns)?;
        let valid = match trial {
            Trial::Warmup => (1..=3).contains(&ordinal),
            Trial::Measured => (1..=5).contains(&ordinal),
        };
        title.validate()?;
        if !valid {
            return Err(Error::Challenge);
        }
        let title_digest = title.digest();
        Ok(Self {
            schema: CHALLENGE_SCHEMA.into(),
            launch_id,
            t0_ns,
            trial,
            ordinal,
            fixture_digest,
            command_digest,
            executable_digest,
            title,
            title_digest,
        })
    }

    fn validate(&self) -> Result<(), Error> {
        Self::transform(
            self.launch_id,
            self.t0_ns,
            self.trial,
            self.ordinal,
            self.fixture_digest,
            self.command_digest,
            self.executable_digest,
            self.title.clone(),
        )
        .and_then(|challenge| {
            if challenge.schema == self.schema && self.schema.as_ref() == CHALLENGE_SCHEMA {
                Ok(())
            } else {
                Err(Error::Challenge)
            }
        })
    }
}

/// Exclusive mode-0700 directory and mode-0600 one-shot listener.
pub struct Listening {
    root: PathBuf,
    socket: PathBuf,
    listener: UnixListener,
    launch_id: [u8; 32],
}

impl Listening {
    /// Create one unpredictable, exclusive evidence endpoint outside product state.
    pub fn transform() -> Result<Self, Error> {
        let base = fs::canonicalize("/tmp").map_err(Error::Io)?;
        for _ in 0..16 {
            let launch_id = random()?;
            let root = base.join(format!(
                "rawr-baseline-evidence-{}-{}-{}",
                std::process::id(),
                SOCKET_ID.fetch_add(1, Ordering::Relaxed),
                hex(&launch_id[..8]),
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
            let socket = root.join("evidence.sock");
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
            listener.set_nonblocking(true).map_err(Error::Io)?;
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

    /// Accept only the exact spawned CLI peer and send the immutable challenge.
    pub fn accept(self, child: &mut Child, challenge: Challenge) -> Result<Parent, Error> {
        let deadline_ns = Envelope::transform(challenge.t0_ns)?.deadline_ns();
        self.accept_before(child, challenge, deadline_ns)
    }

    /// Narrow startup transport to a parent's absolute deadline.
    ///
    /// This does not mint visibility or release authority. The same deadline
    /// bounds acceptance, challenge delivery and the complete startup response.
    pub fn accept_before(
        mut self,
        child: &mut Child,
        challenge: Challenge,
        deadline_ns: u64,
    ) -> Result<Parent, Error> {
        if challenge.launch_id != self.launch_id {
            return Err(Error::Binding("listener launch id"));
        }
        let envelope = Envelope::transform(challenge.t0_ns)?;
        if deadline_ns <= envelope.origin_ns() || deadline_ns > envelope.deadline_ns() {
            return Err(Error::Binding("startup transport deadline"));
        }
        let deadline = Instant::now() + CONNECT_TIMEOUT;
        let stream = loop {
            frame::remaining(deadline_ns)?;
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
        let expected = process::Identity::transform(child.id(), effective_uid());
        let observed = peer(&stream)?;
        if observed != expected {
            return Err(Error::Peer { expected, observed });
        }
        fs::remove_file(&self.socket).map_err(Error::Io)?;
        self.socket = PathBuf::new();
        let mut stream = stream;
        frame::send_before(&mut stream, frame::Tag::Challenge, &challenge, deadline_ns)?;
        stream
            .shutdown(std::net::Shutdown::Write)
            .map_err(Error::Io)?;
        Ok(Parent {
            stream,
            challenge,
            child: expected,
            runner: None,
            startup_received: false,
            startup_deadline_ns: deadline_ns,
        })
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

/// Parent side of one authenticated and strictly ordered evidence channel.
pub struct Parent {
    stream: UnixStream,
    challenge: Challenge,
    child: process::Identity,
    runner: Option<runner::Receipt>,
    startup_received: bool,
    startup_deadline_ns: u64,
}

/// The sole closed startup response, before any successful final capsule.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "status", content = "evidence", rename_all = "kebab-case")]
enum Startup {
    Runner(runner::Receipt),
    Failed(capsule::Failed),
}

impl Parent {
    /// Receive the sole runner receipt before any final capsule.
    pub fn runner_owned(&mut self, child: &mut Child) -> Result<&runner::Receipt, Error> {
        if self.startup_received {
            return Err(Error::Sequence);
        }
        wait_readable_until(&self.stream, child, self.startup_deadline_ns)?;
        self.receive_startup()
    }

    fn receive_startup(&mut self) -> Result<&runner::Receipt, Error> {
        if std::mem::replace(&mut self.startup_received, true) {
            return Err(Error::Sequence);
        }
        let startup = frame::receive_before(
            &mut self.stream,
            frame::Tag::Startup,
            self.startup_deadline_ns,
        )?;
        match startup {
            Startup::Runner(receipt) => {
                receipt.validate(&self.challenge, self.child)?;
                frame::remaining(self.startup_deadline_ns)?;
                self.runner = Some(receipt);
                self.runner.as_ref().ok_or(Error::Sequence)
            }
            Startup::Failed(envelope) => {
                let failure = envelope.open()?;
                failure.validate(&self.challenge, process::Identity::current(), self.child)?;
                frame::eof_before(&mut self.stream, self.startup_deadline_ns)?;
                Err(Error::Startup(Box::new(failure)))
            }
        }
    }

    /// Receive the sole final capsule and require immediate channel EOF.
    pub fn finish(mut self) -> Result<capsule::Payload, Error> {
        let runner = self.runner.take().ok_or(Error::Sequence)?;
        let envelope: capsule::Envelope = frame::receive(&mut self.stream, frame::Tag::Capsule)?;
        let payload = envelope.open()?;
        payload.validate(&self.challenge, self.child, &runner)?;
        require_eof(&mut self.stream)?;
        Ok(payload)
    }

    /// Inspect already-queued final evidence once, without waiting for a new reply.
    /// A ready stream still uses the existing bounded decoder and EOF checks.
    pub fn finish_if_ready(self) -> Result<Option<capsule::Payload>, Error> {
        use nix::poll::{poll, PollFd, PollFlags};

        if self.runner.is_none() {
            return Err(Error::Sequence);
        }
        let mut descriptors = [PollFd::new(&self.stream, PollFlags::POLLIN)];
        poll(&mut descriptors, 0).map_err(Error::Nix)?;
        let events = descriptors[0].revents().unwrap_or_else(PollFlags::empty);
        if events.intersects(PollFlags::POLLERR | PollFlags::POLLNVAL) {
            return Err(Error::Frame);
        }
        if events.intersects(PollFlags::POLLIN | PollFlags::POLLHUP) {
            self.finish().map(Some)
        } else {
            Ok(None)
        }
    }
}

/// Nonblocking ownership of the sole final capsule and actual channel EOF.
/// Dropping this reader closes its stream; no thread or blocking join is created.
pub struct Finishing {
    parent: Parent,
    frame: frame::Pending,
    finished: bool,
}

impl Parent {
    /// Transfer the authenticated channel to the outer deadline owner's poll loop.
    pub fn into_finishing(self) -> Result<Finishing, Error> {
        if self.runner.is_none() {
            return Err(Error::Sequence);
        }
        self.stream.set_nonblocking(true).map_err(Error::Io)?;
        Ok(Finishing {
            parent: self,
            frame: frame::Pending::new(),
            finished: false,
        })
    }
}

impl Finishing {
    /// Read a bounded chunk without waiting; a payload is returned only after EOF.
    pub fn poll(&mut self) -> Result<Option<capsule::Payload>, Error> {
        if self.finished {
            return Err(Error::Sequence);
        }
        let Some(bytes) = self.frame.poll(&mut self.parent.stream)? else {
            return Ok(None);
        };
        self.finished = true;
        let envelope: capsule::Envelope = decode(&bytes)?;
        let payload = envelope.open()?;
        let runner = self.parent.runner.as_ref().ok_or(Error::Sequence)?;
        payload.validate(&self.parent.challenge, self.parent.child, runner)?;
        Ok(Some(payload))
    }
}

/// Child-side proof that activation authenticated before ordinary dispatch.
pub struct Active {
    sink: Arc<LaunchSink>,
    scope: Option<Scope>,
}

impl Active {
    /// Authenticate the direct parent and install one launch-local recorder.
    pub fn transform(
        socket: &Path,
        launch_id: [u8; 32],
        command_digest: [u8; 32],
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
        let expected_parent = process::Identity::transform(parent_pid()?, effective_uid());
        let observed_parent = peer(&stream)?;
        if observed_parent != expected_parent {
            return Err(Error::Peer {
                expected: expected_parent,
                observed: observed_parent,
            });
        }
        let challenge: Challenge = frame::receive(&mut stream, frame::Tag::Challenge)?;
        challenge.validate()?;
        let executable =
            process::file::Identity::read(&std::env::current_exe().map_err(Error::Io)?)?;
        if challenge.launch_id != launch_id
            || challenge.command_digest != command_digest
            || challenge.executable_digest != executable.digest
            || super::monotonic_ns().is_none_or(|now| challenge.t0_ns > now)
        {
            return Err(Error::Binding("child challenge"));
        }
        let recorder = Recorder::transform_at(COUNTER_LIMIT, challenge.t0_ns)?;
        let sink = Arc::new(LaunchSink {
            recorder,
            state: Mutex::new(Output {
                stream,
                receipt: None,
                fault: None,
            }),
            challenge,
            parent: expected_parent,
            child: process::Identity::current(),
            executable,
        });
        let scoped: Arc<dyn Sink> = sink.clone();
        let scope = Scope::transform_at(scoped, sink.challenge.t0_ns)?;
        Ok(Self {
            sink,
            scope: Some(scope),
        })
    }

    /// Join the recorder and emit the sole final capsule without steering `product`.
    pub fn finish(mut self, product: capsule::Product) {
        drop(self.scope.take());
        self.sink.finish(product);
    }

    /// Retain the bounded original failure diagnostic without changing its exit.
    pub fn finish_failure(mut self, reason: &dyn std::fmt::Display) {
        drop(self.scope.take());
        self.sink
            .finish_with_reason(capsule::Product::Failure, Some(reason));
    }
}

struct LaunchSink {
    recorder: Recorder,
    state: Mutex<Output>,
    challenge: Challenge,
    parent: process::Identity,
    child: process::Identity,
    executable: process::file::Identity,
}

struct Output {
    stream: UnixStream,
    receipt: Option<runner::Receipt>,
    fault: Option<Fault>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Fault {
    Runner,
    Io,
    Recorder,
}

impl LaunchSink {
    fn finish(&self, product: capsule::Product) {
        self.finish_with_reason(product, None);
    }

    fn finish_with_reason(
        &self,
        product: capsule::Product,
        reason: Option<&dyn std::fmt::Display>,
    ) {
        let mut counters = match self.recorder.snapshot() {
            Ok(counters) => counters,
            Err(_) => {
                lock(&self.state).fault.get_or_insert(Fault::Recorder);
                return;
            }
        };
        if let Some(principal) = &mut counters.principal {
            principal
                .fault
                .get_or_insert(super::principal::Fault::UnsupportedTransport);
        }
        let mut output = lock(&self.state);
        if output.fault.is_some() {
            return;
        }
        let Some(receipt) = output.receipt.clone() else {
            if product != capsule::Product::Failure {
                output.fault = Some(Fault::Runner);
                return;
            }
            let failure = capsule::Failure::transform(
                &self.challenge,
                self.parent,
                self.child,
                self.executable.clone(),
                counters,
                reason,
            );
            let result = failure.and_then(capsule::Failed::seal).and_then(|failure| {
                frame::send(
                    &mut output.stream,
                    frame::Tag::Startup,
                    &Startup::Failed(failure),
                )
            });
            if result.is_err() {
                output.fault = Some(Fault::Io);
            }
            return;
        };
        let payload = capsule::Payload::transform(
            &self.challenge,
            self.parent,
            self.child,
            self.executable.clone(),
            receipt,
            counters,
            product,
        )
        .and_then(|payload| match reason {
            Some(reason) => payload.with_failure_reason(reason),
            None => Ok(payload),
        });
        match payload
            .and_then(capsule::Envelope::seal)
            .and_then(|envelope| frame::send(&mut output.stream, frame::Tag::Capsule, &envelope))
        {
            Ok(()) => {}
            Err(_) => output.fault = Some(Fault::Io),
        }
    }
}

impl Sink for LaunchSink {
    fn principal(&self, observation: &super::principal::Observation<'_>) {
        // R3 currently joins the paired cell transport only. Do not present an
        // empty history as complete on this independent launch-capsule path.
        self.recorder.principal(&super::principal::Observation {
            event: Err(super::principal::Fault::UnsupportedTransport),
            budget: observation.budget,
        });
    }

    fn publication(&self, class: super::publication::Class, event: super::publication::Event) {
        self.recorder.publication(class, event);
    }

    fn bundle(
        &self,
        kind: NodeKind,
        target: super::pipeline::target::Target,
        event: super::bundle::Event,
    ) {
        self.recorder.bundle(kind, target, event);
    }

    fn target(&self, kind: super::pipeline::target::Kind, target: super::pipeline::target::Target) {
        self.recorder.target(kind, target);
    }
    fn native(
        &self,
        runner_pid: u32,
        epoch: u64,
        observation: crate::protocol::account::Observation,
    ) {
        self.recorder.native(runner_pid, epoch, observation);
    }

    fn native_fault(&self, runner_pid: u32, epoch: u64, cause: super::accounting::NativeFault) {
        self.recorder.native_fault(runner_pid, epoch, cause);
    }
    fn scalar_overflow(&self, scalar: super::accounting::Scalar) {
        self.recorder.scalar_overflow(scalar);
    }

    fn scalar(&self, scalar: super::accounting::Scalar, value: u64) {
        self.recorder.scalar(scalar, value);
    }

    fn graph(&self, graph: super::accounting::Graph) {
        self.recorder.graph(graph);
    }

    fn node(&self, kind: NodeKind, event: NodeEvent) {
        self.recorder.node(kind, event);
    }

    fn process(&self, kind: ProcessKind) {
        self.recorder.process(kind);
    }

    fn milestone(&self, milestone: Milestone, observed_ns: Option<u64>) {
        self.recorder.milestone(milestone, observed_ns);
    }

    fn runner_owned(&self, owned: &super::runner::Owned) {
        self.recorder.runner_owned(owned);
        let receipt = runner::Receipt::transform(&self.challenge, self.child, owned);
        let mut output = lock(&self.state);
        if output.receipt.is_some() || output.fault.is_some() {
            output.fault.get_or_insert(Fault::Runner);
            return;
        }
        let Ok(receipt) = receipt else {
            output.fault = Some(Fault::Runner);
            return;
        };
        if frame::send(
            &mut output.stream,
            frame::Tag::Startup,
            &Startup::Runner(receipt.clone()),
        )
        .is_err()
        {
            output.fault = Some(Fault::Io);
            return;
        }
        output.receipt = Some(receipt);
    }

    fn profile(&self, evidence: &super::profile::Evidence) {
        self.recorder.profile(evidence);
    }

    fn pipeline(&self, evidence: &super::pipeline::Evidence) {
        self.recorder.pipeline(evidence);
    }

    fn scope_opened(&self) {
        self.recorder.scope_opened();
    }

    fn scope_joined(&self) {
        self.recorder.scope_joined();
    }
}

fn require_eof(stream: &mut UnixStream) -> Result<(), Error> {
    let mut trailing = [0_u8; 1];
    match stream.read(&mut trailing) {
        Ok(0) => Ok(()),
        Ok(_) => Err(Error::Trailing),
        Err(error) => Err(Error::Io(error)),
    }
}

/// Digest exact forwarded `OsString` bytes with unambiguous boundaries.
pub fn command_digest(arguments: &[OsString]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"rawr.phase9.forwarded-command/v1");
    for argument in arguments {
        let bytes = argument.as_os_str().as_bytes();
        digest.update((bytes.len() as u64).to_le_bytes());
        digest.update(bytes);
    }
    digest.finalize().into()
}

/// Parse one exact 256-bit lowercase hexadecimal identity.
pub fn parse_digest(value: &OsStr) -> Result<[u8; 32], Error> {
    let bytes = value.as_bytes();
    if bytes.len() != 64
        || bytes
            .iter()
            .any(|byte| !byte.is_ascii_digit() && !(b'a'..=b'f').contains(byte))
    {
        return Err(Error::Hex);
    }
    let mut output = [0_u8; 32];
    for (index, byte) in output.iter_mut().enumerate() {
        *byte = (nibble(bytes[index * 2])? << 4) | nibble(bytes[index * 2 + 1])?;
    }
    Ok(output)
}

#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn nibble(byte: u8) -> Result<u8, Error> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(Error::Hex),
    }
}

fn random() -> Result<[u8; 32], Error> {
    let mut file = fs::File::open("/dev/urandom").map_err(Error::Io)?;
    let mut bytes = [0_u8; 32];
    file.read_exact(&mut bytes).map_err(Error::Io)?;
    Ok(bytes)
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

fn configure(stream: &UnixStream) -> Result<(), Error> {
    configure_with_timeout(stream, IO_TIMEOUT)
}

fn configure_with_timeout(stream: &UnixStream, timeout: Duration) -> Result<(), Error> {
    stream.set_nonblocking(false).map_err(Error::Io)?;
    stream.set_read_timeout(Some(timeout)).map_err(Error::Io)?;
    stream.set_write_timeout(Some(timeout)).map_err(Error::Io)
}

#[cfg(test)]
fn wait_readable(stream: &UnixStream, child: &mut Child, timeout: Duration) -> Result<(), Error> {
    use nix::poll::{poll, PollFd, PollFlags};

    let deadline = Instant::now() + timeout;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(Error::Timeout);
        }
        let remaining = deadline.saturating_duration_since(now);
        let poll_time = remaining.min(Duration::from_millis(100));
        let milliseconds = poll_time.as_micros().saturating_add(999) / 1_000;
        let mut descriptors = [PollFd::new(stream, PollFlags::POLLIN)];
        match poll(
            &mut descriptors,
            i32::try_from(milliseconds.max(1)).unwrap_or(i32::MAX),
        ) {
            Ok(0) => {}
            Ok(_) => {
                let events = descriptors[0].revents().unwrap_or_else(PollFlags::empty);
                if events.intersects(PollFlags::POLLERR | PollFlags::POLLNVAL) {
                    return Err(Error::Frame);
                }
                if events.intersects(PollFlags::POLLIN | PollFlags::POLLHUP) {
                    return Ok(());
                }
            }
            Err(nix::errno::Errno::EINTR) => continue,
            Err(error) => return Err(Error::Nix(error)),
        }
        if child.try_wait().map_err(Error::Io)?.is_some() {
            return Err(Error::EarlyExit);
        }
    }
}

fn wait_readable_until(
    stream: &UnixStream,
    child: &mut Child,
    deadline_ns: u64,
) -> Result<(), Error> {
    use nix::poll::{poll, PollFd, PollFlags};

    loop {
        let now_ns = super::monotonic_ns().ok_or(Error::Clock)?;
        let Some(remaining_ns) = deadline_ns.checked_sub(now_ns) else {
            return Err(Error::Timeout);
        };
        if remaining_ns == 0 {
            return Err(Error::Timeout);
        }
        let poll_time = Duration::from_nanos(remaining_ns).min(Duration::from_millis(100));
        let milliseconds = poll_time.as_micros().saturating_add(999) / 1_000;
        let mut descriptors = [PollFd::new(stream, PollFlags::POLLIN)];
        match poll(
            &mut descriptors,
            i32::try_from(milliseconds.max(1)).unwrap_or(i32::MAX),
        ) {
            Ok(0) => {}
            Ok(_) => {
                let events = descriptors[0].revents().unwrap_or_else(PollFlags::empty);
                if events.intersects(PollFlags::POLLERR | PollFlags::POLLNVAL) {
                    return Err(Error::Frame);
                }
                if events.intersects(PollFlags::POLLIN | PollFlags::POLLHUP) {
                    return match super::monotonic_ns() {
                        Some(observed_ns) if observed_ns <= deadline_ns => Ok(()),
                        Some(_) => Err(Error::Timeout),
                        None => Err(Error::Clock),
                    };
                }
            }
            Err(nix::Error::EINTR) => {}
            Err(error) => return Err(Error::Nix(error)),
        }
        if child.try_wait().map_err(Error::Io)?.is_some() {
            // The final frame may have arrived between the timed poll and reap.
            // Drain that authenticated stream instead of losing a startup refusal.
            let mut descriptors = [PollFd::new(stream, PollFlags::POLLIN)];
            match poll(&mut descriptors, 0) {
                Ok(_) => {
                    let events = descriptors[0].revents().unwrap_or_else(PollFlags::empty);
                    if events.intersects(PollFlags::POLLERR | PollFlags::POLLNVAL) {
                        return Err(Error::Frame);
                    }
                    if events.intersects(PollFlags::POLLIN | PollFlags::POLLHUP) {
                        return match super::monotonic_ns() {
                            Some(observed_ns) if observed_ns <= deadline_ns => Ok(()),
                            Some(_) => Err(Error::Timeout),
                            None => Err(Error::Clock),
                        };
                    }
                }
                Err(nix::Error::EINTR) => continue,
                Err(error) => return Err(Error::Nix(error)),
            }
            return Err(Error::EarlyExit);
        }
    }
}

#[cfg(target_os = "macos")]
fn peer(stream: &UnixStream) -> Result<process::Identity, Error> {
    use nix::sys::socket::{getsockopt, sockopt};

    let pid = getsockopt(stream, sockopt::LocalPeerPid).map_err(Error::Nix)?;
    let credentials = getsockopt(stream, sockopt::LocalPeerCred).map_err(Error::Nix)?;
    if credentials.version() != 0 {
        return Err(Error::Credentials);
    }
    let pid = u32::try_from(pid).map_err(|_| Error::Credentials)?;
    Ok(process::Identity::transform(pid, credentials.uid()))
}

#[cfg(not(target_os = "macos"))]
fn peer(_stream: &UnixStream) -> Result<process::Identity, Error> {
    Err(Error::Unsupported)
}

fn parent_pid() -> Result<u32, Error> {
    u32::try_from(nix::unistd::getppid().as_raw()).map_err(|_| Error::Credentials)
}

fn effective_uid() -> u32 {
    nix::unistd::Uid::effective().as_raw()
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Authenticated evidence failure. Product execution never consumes this value.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the authenticated product failed before runner ownership")]
    Startup(Box<capsule::Failure>),
    #[error("evidence I/O failed")]
    Io(#[source] std::io::Error),
    #[error("evidence socket option failed")]
    Nix(#[source] nix::Error),
    #[error("peer credential validation is unsupported on this platform")]
    Unsupported,
    #[error("evidence peer credentials are malformed")]
    Credentials,
    #[error("evidence peer differs from the exact expected process")]
    Peer {
        expected: process::Identity,
        observed: process::Identity,
    },
    #[error("evidence endpoint mode differs: expected {expected:o}, observed {actual:o}")]
    Mode { expected: u32, actual: u32 },
    #[error("evidence endpoint name collided repeatedly")]
    Collision,
    #[error("evidence child exited before connecting")]
    EarlyExit,
    #[error("evidence operation exceeded its fixed deadline")]
    Timeout,
    #[error("the process-shared evidence clock is unavailable or overflowed")]
    Clock,
    #[error("evidence socket is not one absolute mode-0600 socket")]
    Socket,
    #[error("evidence challenge is invalid")]
    Challenge,
    #[error("evidence hexadecimal identity is invalid")]
    Hex,
    #[error("evidence text exceeds its fixed bound")]
    Text,
    #[error("evidence title file is invalid at {0}")]
    Title(&'static str),
    #[error("evidence sequence or cardinality is invalid")]
    Sequence,
    #[error("evidence contains trailing bytes after the final capsule")]
    Trailing,
    #[error("evidence binding differs at {0}")]
    Binding(&'static str),
    #[error("evidence serialization is invalid")]
    Json(#[source] serde_json::Error),
    #[error("evidence frame magic, version, tag, or length is invalid")]
    Frame,
    #[error("evidence payload is not canonical")]
    Canonical,
    #[error("evidence payload digest differs")]
    Digest,
    #[error("evidence executable is not one canonical regular file")]
    Executable,
    #[error(transparent)]
    Recorder(#[from] super::recorder::Error),
    #[error(transparent)]
    Scope(#[from] super::ScopeError),
}

impl Error {
    /// Borrow authenticated failed-startup facts for bounded diagnostic retention.
    pub fn startup_failure(&self) -> Option<&capsule::Failure> {
        match self {
            Self::Startup(failure) => Some(failure),
            _ => None,
        }
    }
}

fn canonical<T>(value: &T) -> Result<Box<[u8]>, Error>
where
    T: Serialize,
{
    let bytes = serde_json::to_vec(value).map_err(Error::Json)?;
    if bytes.len() > REPORT_BYTES {
        return Err(Error::Frame);
    }
    Ok(bytes.into_boxed_slice())
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

#[cfg(test)]
mod tests {
    use std::ffi::{OsStr, OsString};
    use std::io::{Read as _, Write as _};
    use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};

    fn startup_fixture() -> (super::LaunchSink, super::Parent) {
        let parent = super::process::Identity::current();
        let child = super::process::Identity::transform(parent.pid.saturating_add(1), parent.uid);
        let challenge = super::Challenge::transform(
            [1; 32],
            1,
            super::Trial::Measured,
            1,
            [2; 32],
            [3; 32],
            [4; 32],
            super::title::Identity::testing(),
        )
        .unwrap();
        let (writer, reader) = std::os::unix::net::UnixStream::pair().unwrap();
        super::configure(&writer).unwrap();
        super::configure(&reader).unwrap();
        let sink = super::LaunchSink {
            recorder: super::Recorder::transform_at(10, 1).unwrap(),
            state: std::sync::Mutex::new(super::Output {
                stream: writer,
                receipt: None,
                fault: None,
            }),
            challenge: challenge.clone(),
            parent,
            child,
            executable: super::process::file::Identity {
                path: Box::from(&b"/fixture/rawr"[..]),
                digest: [4; 32],
                device: 1,
                inode: 2,
                size: 3,
            },
        };
        let receiver = super::Parent {
            stream: reader,
            challenge,
            child,
            runner: None,
            startup_received: false,
            startup_deadline_ns: crate::metrics::monotonic_ns().unwrap() + 30_000_000_000,
        };
        (sink, receiver)
    }

    fn owned(child: super::process::Identity) -> crate::metrics::runner::Owned {
        crate::metrics::runner::Owned {
            runner_pid: child.pid.saturating_add(1),
            runner_uid: child.uid,
            build_request: [5; 32],
            generated_manifest: [6; 32],
            generation_key: [7; 32],
            epoch: 1,
            artifact: [8; 32],
            executable_path: Box::from(&b"/fixture/runner"[..]),
            executable_digest: [23; 32],
            executable_bytes: 42,
            spawned_ns: Some(2),
        }
    }

    #[test]
    fn launch_sink_forwards_scoped_bundle_targets_and_publication_history_into_final_capsule() {
        use crate::metrics::{self, bundle, pipeline, publication};
        let (sink, mut parent) = startup_fixture();
        let target = pipeline::target::Target::observe(crate::store::bundle::BundleRef::new(
            crate::node::ExecutionKey::from_bytes([21; 32]),
            crate::store::bundle::BundleId::from_bytes([22; 32]),
        ));
        let writer = std::thread::spawn(move || {
            let sink = std::sync::Arc::new(sink);
            let scope = super::Scope::transform_at(sink.clone(), 1).unwrap();
            metrics::record_target(pipeline::target::Kind::Resolution, target);
            metrics::record_pipeline(&pipeline::Evidence {
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
            for event in [
                bundle::Event::Allocated,
                bundle::Event::Initialized,
                bundle::Event::SealStarted,
                bundle::Event::Sealed,
                bundle::Event::PublicationStarted,
                bundle::Event::Published,
                bundle::Event::Retained,
            ] {
                metrics::record_bundle(metrics::NodeKind::Resolution, target, event);
            }
            for event in [
                publication::Event::Started,
                publication::Event::Created,
                publication::Event::Renamed,
                publication::Event::Returned,
            ] {
                metrics::record_publication(
                    publication::Class::Action(metrics::NodeKind::Resolution),
                    event,
                );
            }
            drop(scope);
            sink.finish_with_reason(
                super::capsule::Product::Failure,
                Some(&"later owner failure"),
            );
            let fault = super::lock(&sink.state).fault;
            drop(sink);
            fault
        });
        let error = parent.receive_startup().unwrap_err();
        let failure = error
            .startup_failure()
            .unwrap_or_else(|| panic!("final failure capsule retained: {error:?}"));
        assert_eq!(
            failure
                .counters
                .pipeline
                .as_ref()
                .unwrap()
                .resolution_target,
            Some(target)
        );
        let bundles = failure.counters.accounting.bundles.unwrap();
        assert_eq!(bundles.fault, None);
        assert!(bundles.resolution.retained_once(target));
        let history = failure.counters.accounting.publications.unwrap();
        assert!(history.closed());
        assert!(history
            .nodes
            .iter()
            .find(|row| row.kind == metrics::NodeKind::Resolution)
            .unwrap()
            .action
            .winners(1));
        assert_eq!(writer.join().unwrap(), None);
    }

    #[test]
    fn failed_startup_retains_reached_snapshot_and_original_reason_without_runner() {
        use crate::metrics::Sink as _;
        let (sink, mut parent) = startup_fixture();
        let writer = std::thread::spawn(move || {
            sink.scalar(crate::metrics::accounting::Scalar::OwnerInputBytes, 37);
            sink.finish_with_reason(
                super::capsule::Product::Failure,
                Some(&"original resource refusal"),
            );
            let fault = super::lock(&sink.state).fault;
            drop(sink);
            fault
        });
        let error = parent.receive_startup().unwrap_err();
        let failure = error
            .startup_failure()
            .unwrap_or_else(|| panic!("failure is structured evidence: {error:?}"));
        assert_eq!(
            failure.reason,
            super::capsule::Reason::Complete {
                message: "original resource refusal".into()
            }
        );
        assert!(failure.counters.runner.is_none());
        assert_eq!(
            failure
                .counters
                .accounting
                .scalar(crate::metrics::accounting::Scalar::OwnerInputBytes)
                .unwrap()
                .total,
            37
        );
        assert!(!failure.canonical_bytes().unwrap().is_empty());
        assert!(matches!(
            parent.receive_startup(),
            Err(super::Error::Sequence)
        ));
        assert!(matches!(parent.finish(), Err(super::Error::Sequence)));
        assert_eq!(writer.join().unwrap(), None);
    }

    #[test]
    fn failed_startup_rejects_identity_mismatch_and_trailing_frame() {
        for mismatch in [true, false] {
            let (mut sink, mut parent) = startup_fixture();
            if mismatch {
                sink.challenge.command_digest[0] ^= 1;
            }
            let writer = std::thread::spawn(move || {
                sink.finish(super::capsule::Product::Failure);
                if !mismatch {
                    super::lock(&sink.state).stream.write_all(b"extra").unwrap();
                }
            });
            let error = parent.receive_startup().unwrap_err();
            if mismatch {
                assert!(
                    matches!(error, super::Error::Binding("startup failure")),
                    "{error:?}"
                );
            } else {
                assert!(matches!(error, super::Error::Trailing));
            }
            assert!(parent.runner.is_none());
            writer.join().unwrap();
        }
    }

    #[test]
    fn successful_startup_still_requires_receipt_before_final_capsule() {
        use crate::metrics::Sink as _;
        let (sink, mut parent) = startup_fixture();
        let writer = std::thread::spawn(move || {
            sink.runner_owned(&owned(sink.child));
            sink.finish(super::capsule::Product::Success);
            let fault = super::lock(&sink.state).fault;
            drop(sink);
            fault
        });
        let receipt = parent.receive_startup().unwrap().clone();
        assert!(matches!(
            parent.receive_startup(),
            Err(super::Error::Sequence)
        ));
        let payload = parent.finish().unwrap();
        assert_eq!(payload.product, super::capsule::Product::Success);
        assert_eq!(payload.failure_reason, None);
        assert_eq!(payload.runner, receipt);
        assert_eq!(writer.join().unwrap(), None);

        let (sink, mut parent) = startup_fixture();
        let writer = std::thread::spawn(move || {
            sink.finish(super::capsule::Product::Success);
            let fault = super::lock(&sink.state).fault;
            drop(sink);
            fault
        });
        assert!(matches!(parent.receive_startup(), Err(super::Error::Io(_))));
        assert_eq!(writer.join().unwrap(), Some(super::Fault::Runner));
    }

    #[test]
    fn failed_product_after_runner_retains_original_reason_and_reached_counters() {
        use crate::metrics::Sink as _;
        let (sink, mut parent) = startup_fixture();
        let writer = std::thread::spawn(move || {
            sink.scalar(crate::metrics::accounting::Scalar::OwnerInputBytes, 71);
            sink.runner_owned(&owned(sink.child));
            let active = super::Active {
                sink: std::sync::Arc::new(sink),
                scope: None,
            };
            active.finish_failure(&"original failure after runner ownership");
        });
        parent.receive_startup().unwrap();
        let challenge = parent.challenge.clone();
        // Only the fixture waits for its writer. The method under test performs
        // one zero-timeout poll against bytes already queued on the same channel.
        let mut descriptors = [nix::poll::PollFd::new(
            &parent.stream,
            nix::poll::PollFlags::POLLIN,
        )];
        assert!(nix::poll::poll(&mut descriptors, 1000).unwrap() > 0);
        let mut payload = parent
            .finish_if_ready()
            .unwrap()
            .expect("queued final capsule");
        assert_eq!(payload.product, super::capsule::Product::Failure);
        assert_eq!(
            payload.failure_reason,
            Some(super::capsule::Reason::Complete {
                message: "original failure after runner ownership".into()
            })
        );
        assert_eq!(
            payload
                .counters
                .accounting
                .scalar(crate::metrics::accounting::Scalar::OwnerInputBytes)
                .unwrap()
                .total,
            71
        );
        let encoded = payload.canonical_bytes().unwrap();
        let decoded: super::capsule::Payload = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, payload);
        payload.product = super::capsule::Product::Success;
        assert!(payload
            .validate(&challenge, payload.cli, &payload.runner)
            .is_err());
        assert!(payload
            .with_failure_reason(&"invalid success diagnostic")
            .is_err());
        writer.join().unwrap();
    }

    #[test]
    fn opportunistic_finish_keeps_an_unqueued_capsule_explicitly_unknown() {
        use crate::metrics::Sink as _;
        let (sink, mut parent) = startup_fixture();
        sink.runner_owned(&owned(sink.child));
        parent.receive_startup().unwrap();
        assert!(parent.finish_if_ready().unwrap().is_none());
        drop(sink);
    }

    #[test]
    fn startup_rejects_a_final_capsule_tag_and_truncated_failure() {
        for wrong_tag in [true, false] {
            let (sink, mut parent) = startup_fixture();
            let writer = std::thread::spawn(move || {
                let mut output = super::lock(&sink.state);
                if wrong_tag {
                    super::frame::send(
                        &mut output.stream,
                        super::frame::Tag::Capsule,
                        &super::Trial::Measured,
                    )
                    .unwrap();
                } else {
                    // Valid current startup header, deliberately incomplete payload.
                    output
                        .stream
                        .write_all(b"R9EV\0\x03\x02\0\0\0\x20short")
                        .unwrap();
                }
            });
            let error = parent.receive_startup().unwrap_err();
            assert!(if wrong_tag {
                matches!(error, super::Error::Frame)
            } else {
                matches!(error, super::Error::Io(_))
            });
            writer.join().unwrap();
        }
    }

    #[test]
    fn owner_envelope_is_absolute_from_the_exact_pre_spawn_origin() {
        let envelope = super::Envelope::transform(7).expect("owner envelope seals");
        assert_eq!(envelope.origin_ns(), 7);
        assert_eq!(envelope.deadline_ns(), 7 + 1_800_000_000_000);
        assert!(super::Envelope::transform(0).is_err());
        assert!(super::Envelope::transform(u64::MAX).is_err());
    }

    #[test]
    fn configured_stream_waits_for_a_delayed_frame_after_nonblocking_accept() {
        let (mut writer, mut reader) = std::os::unix::net::UnixStream::pair().expect("socket pair");
        reader
            .set_nonblocking(true)
            .expect("accepted stream starts nonblocking");
        super::configure(&reader).expect("evidence stream configures");
        let delayed = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(10));
            writer.write_all(&[23]).expect("delayed byte writes");
        });
        let mut byte = [0_u8; 1];
        reader.read_exact(&mut byte).expect("configured read waits");
        delayed.join().expect("delayed writer joins");
        assert_eq!(byte, [23]);
    }

    #[test]
    fn runner_receipt_waits_for_frame_start_without_widening_partial_frame_io() {
        let (mut writer, mut reader) = std::os::unix::net::UnixStream::pair().expect("socket pair");
        super::configure_with_timeout(&reader, std::time::Duration::from_millis(5))
            .expect("short partial-frame timeout configures");
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("1")
            .spawn()
            .expect("live child starts");
        let delayed = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(30));
            writer.write_all(&[29]).expect("delayed frame starts");
        });
        super::wait_readable(&reader, &mut child, std::time::Duration::from_millis(200))
            .expect("empty pre-frame interval remains bounded separately");
        let mut byte = [0_u8; 1];
        reader
            .read_exact(&mut byte)
            .expect("started frame completes under the short I/O timeout");
        delayed.join().expect("delayed writer joins");
        child.kill().expect("test child terminates");
        child.wait().expect("test child reaps");
        assert_eq!(byte, [29]);
    }

    #[test]
    fn started_runner_receipt_frame_retains_the_short_io_deadline() {
        let (mut writer, mut reader) = std::os::unix::net::UnixStream::pair().expect("socket pair");
        super::configure_with_timeout(&reader, std::time::Duration::from_millis(5))
            .expect("short partial-frame timeout configures");
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("1")
            .spawn()
            .expect("live child starts");
        let delayed = std::thread::spawn(move || {
            writer.write_all(&[31]).expect("frame starts");
            std::thread::sleep(std::time::Duration::from_millis(30));
            writer.write_all(&[37]).expect("late frame byte writes");
        });
        super::wait_readable(&reader, &mut child, std::time::Duration::from_millis(200))
            .expect("frame start becomes readable");
        let mut bytes = [0_u8; 2];
        let error = reader
            .read_exact(&mut bytes)
            .expect_err("partial frame exceeds the short I/O deadline");
        assert!(matches!(
            error.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        ));
        delayed.join().expect("delayed writer joins");
        child.kill().expect("test child terminates");
        child.wait().expect("test child reaps");
    }

    #[test]
    fn command_digest_retains_non_utf8_bytes_and_argument_boundaries() {
        let opaque = OsString::from_vec(vec![b'a', 0xff]);
        assert_ne!(
            super::command_digest(&[opaque]),
            super::command_digest(&[OsString::from("a"), OsString::from_vec(vec![0xff])])
        );
    }

    #[test]
    fn lowercase_launch_identity_is_exact() {
        let valid = "ab".repeat(32);
        assert_eq!(
            super::parse_digest(OsStr::new(&valid)).expect("lowercase digest parses"),
            [0xab; 32]
        );
        let upper = valid.to_uppercase();
        assert!(super::parse_digest(OsStr::new(&upper)).is_err());
        assert!(super::parse_digest(OsStr::from_bytes(&[0xff; 64])).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn peer_credentials_bind_exact_pid_and_uid() {
        let (left, _right) = std::os::unix::net::UnixStream::pair().expect("socket pair");
        assert_eq!(
            super::peer(&left).expect("macOS peer credentials"),
            super::process::Identity::current()
        );
    }

    #[test]
    fn observation_io_fault_latches_without_returning_product_authority() {
        use crate::metrics::Sink as _;

        let now = crate::metrics::monotonic_ns().expect("shared clock");
        let challenge = super::Challenge::transform(
            [1; 32],
            now,
            super::Trial::Measured,
            1,
            [2; 32],
            [3; 32],
            [4; 32],
            super::title::Identity::testing(),
        )
        .expect("challenge seals");
        let (stream, peer) = std::os::unix::net::UnixStream::pair().expect("socket pair");
        drop(peer);
        let recorder = super::Recorder::transform_at(2, now).expect("recorder seals");
        let sink = super::LaunchSink {
            recorder: recorder.clone(),
            state: std::sync::Mutex::new(super::Output {
                stream,
                receipt: None,
                fault: None,
            }),
            challenge,
            parent: super::process::Identity::current(),
            child: super::process::Identity::current(),
            executable: super::process::file::Identity {
                path: Box::from(&b"/fixture/rawr"[..]),
                digest: [4; 32],
                device: 1,
                inode: 2,
                size: 3,
            },
        };
        sink.node(super::NodeKind::Api, super::NodeEvent::ExecutedMiss);
        sink.scalar(crate::metrics::accounting::Scalar::ApiCanonicalBytes, 42);
        sink.graph(crate::metrics::accounting::Graph {
            account: 1,
            stage: crate::metrics::accounting::Stage::Node(super::NodeKind::Api),
            admitted: [1, 2, 3, 4, 5, 6],
            exceedance: None,
        });
        sink.runner_owned(&crate::metrics::runner::Owned {
            runner_pid: std::process::id().saturating_add(1),
            runner_uid: nix::unistd::Uid::effective().as_raw(),
            build_request: [5; 32],
            generated_manifest: [8; 32],
            generation_key: [6; 32],
            epoch: 1,
            artifact: [7; 32],
            executable_path: Box::from(&b"/fixture/runner"[..]),
            executable_digest: [23; 32],
            executable_bytes: 42,
            spawned_ns: Some(now),
        });
        sink.finish(super::capsule::Product::Success);
        assert_eq!(super::lock(&sink.state).fault, Some(super::Fault::Io));
        assert!(recorder
            .snapshot()
            .expect("product-independent counters remain readable")
            .nodes
            .iter()
            .any(|row| row.kind == super::NodeKind::Api && row.count == 1));
        let accounting = recorder
            .snapshot()
            .expect("accounting remains readable")
            .accounting;
        assert_eq!(
            accounting
                .scalar(crate::metrics::accounting::Scalar::ApiCanonicalBytes)
                .expect("scalar survives output failure")
                .total,
            42,
        );
        assert_eq!(
            accounting.graphs[0]
                .expect("original graph remains")
                .account,
            1
        );
    }
}
