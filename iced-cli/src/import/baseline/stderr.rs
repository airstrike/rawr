//! Bounded parent-owned diagnostics for one ordinary evidence child.

use std::fmt;
use std::io::Read as _;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::process::{Child, ExitStatus, Stdio};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use rawr_import::metrics::evidence;

const READ_WAIT: Duration = Duration::from_millis(50);
const SETTLE_WAIT: Duration = Duration::from_secs(10);
const READ_BYTES: usize = 16 * 1024;

/// One named parent stage that failed without changing the primary error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Stage {
    EvidenceAccept,
    RunnerReceipt,
    Observation,
    Retirement,
}

/// Bounded exit and stderr facts attached to a primary launch failure.
#[derive(Debug)]
pub(super) struct Context {
    status: Status,
    tail: Box<[u8]>,
    observed: u64,
    truncated: bool,
    reader: Reader,
}

/// Concurrent stderr drain owned by the harness parent.
pub(super) struct Capture {
    shared: Arc<Shared>,
    handle: Option<thread::JoinHandle<()>>,
    deadline_ns: u64,
}

/// Typed diagnostic failure bundled with inert facts from the same drain.
#[derive(Debug)]
pub(super) struct Failure {
    source: Error,
    context: Context,
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}

struct State {
    tail: Tail,
    reader: Reader,
}

struct Tail {
    bytes: Vec<u8>,
    observed: u64,
    truncated: bool,
}

#[derive(Clone, Debug)]
enum Reader {
    Draining,
    Complete,
    Deadline,
    Clock,
    Failed(Box<str>),
    Panicked,
}

#[derive(Debug)]
enum Status {
    Exited(Box<str>),
    Running,
    InspectionFailed(Box<str>),
}

/// Failure to establish or complete the bounded diagnostic drain.
#[derive(Debug, thiserror::Error)]
pub(super) enum Error {
    #[error("failed to create the bounded CLI stderr stream")]
    Pair(#[source] std::io::Error),
    #[error("failed to configure the bounded CLI stderr stream")]
    Configure(#[source] std::io::Error),
    #[error("failed to spawn the bounded CLI stderr reader")]
    Spawn(#[source] std::io::Error),
    #[error("the bounded CLI stderr reader did not retire within ten seconds")]
    Settle,
    #[error("the bounded CLI stderr reader exceeded the absolute evidence envelope")]
    Deadline,
    #[error("the bounded CLI stderr reader lost its monotonic clock")]
    Clock,
    #[error("the bounded CLI stderr reader failed: {reason}")]
    Read { reason: Box<str> },
    #[error("the bounded CLI stderr reader panicked")]
    Panicked,
    #[error("CLI stderr exceeded the fixed {limit}-byte retained tail")]
    Limit { limit: usize },
}

impl fmt::Display for Stage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EvidenceAccept => formatter.write_str("evidence accept"),
            Self::RunnerReceipt => formatter.write_str("runner receipt"),
            Self::Observation => formatter.write_str("window observation"),
            Self::Retirement => formatter.write_str("window retirement"),
        }
    }
}

impl fmt::Display for Context {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let tail = if self.tail.is_empty() {
            std::borrow::Cow::Borrowed("<empty>")
        } else {
            String::from_utf8_lossy(&self.tail)
        };
        write!(
            formatter,
            "CLI status {}; stderr observed={} retained={} truncated={}; reader={}; bounded stderr tail: {}",
            self.status,
            self.observed,
            self.tail.len(),
            self.truncated,
            self.reader,
            tail,
        )
    }
}

impl fmt::Display for Status {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exited(status) => write!(formatter, "{status}"),
            Self::Running => formatter.write_str("still running"),
            Self::InspectionFailed(reason) => write!(formatter, "inspection failed: {reason}"),
        }
    }
}

impl fmt::Display for Reader {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Draining => formatter.write_str("bounded retirement in progress"),
            Self::Complete => formatter.write_str("complete"),
            Self::Deadline => formatter.write_str("absolute envelope exceeded"),
            Self::Clock => formatter.write_str("clock unavailable"),
            Self::Failed(reason) => write!(formatter, "failed: {reason}"),
            Self::Panicked => formatter.write_str("panicked"),
        }
    }
}

impl Capture {
    /// Transform the absolute launch envelope into one fixed-cap diagnostic stream.
    pub(super) fn transform(envelope: evidence::Envelope) -> Result<(Stdio, Self), Error> {
        let (reader, writer) = UnixStream::pair().map_err(Error::Pair)?;
        reader.set_nonblocking(true).map_err(Error::Configure)?;
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                tail: Tail::new(),
                reader: Reader::Draining,
            }),
            changed: Condvar::new(),
        });
        let worker = Arc::clone(&shared);
        let handle = thread::Builder::new()
            .name("rawr-baseline-stderr".into())
            .spawn(move || drain(reader, envelope.deadline_ns(), worker))
            .map_err(Error::Spawn)?;
        let writer: OwnedFd = writer.into();
        Ok((
            Stdio::from(writer),
            Self {
                shared,
                handle: Some(handle),
                deadline_ns: envelope.deadline_ns(),
            },
        ))
    }

    /// Seal the reader with the known status after a successful child retirement.
    pub(super) fn finish(self, status: &ExitStatus) -> Result<(), Failure> {
        self.seal(status, None)
    }

    /// Finish within the existing post-press grace, without starting another one.
    pub(super) fn finish_before(
        self,
        status: &ExitStatus,
        deadline_ns: u64,
    ) -> Result<(), Failure> {
        let deadline_ns = deadline_ns.min(self.deadline_ns);
        self.seal(status, Some(deadline_ns))
    }

    fn seal(mut self, status: &ExitStatus, deadline_ns: Option<u64>) -> Result<(), Failure> {
        let late = if let Some(deadline_ns) = deadline_ns {
            let _ = self.settle_until(deadline_ns);
            // A terminal reader state precedes the worker's actual return. Only
            // join after that return is observed, and never wait past this grace.
            while self
                .handle
                .as_ref()
                .is_some_and(|handle| !handle.is_finished())
            {
                let Some(remaining) = rawr_import::metrics::monotonic_ns()
                    .and_then(|now| deadline_ns.checked_sub(now))
                    .filter(|remaining| *remaining > 0)
                else {
                    break;
                };
                thread::park_timeout(Duration::from_nanos(remaining.min(2_000_000)));
            }
            if self
                .handle
                .as_ref()
                .is_some_and(thread::JoinHandle::is_finished)
            {
                self.join();
            }
            rawr_import::metrics::monotonic_ns().is_none_or(|now| now >= deadline_ns)
                || self.handle.is_some()
        } else {
            let _ = self.settle();
            self.join_if_retired();
            false
        };
        let state = lock(&self.shared.state);
        let source = match &state.reader {
            _ if late => Error::Deadline,
            Reader::Complete if state.tail.truncated => Error::Limit {
                limit: super::OUTPUT_BYTES,
            },
            Reader::Complete => return Ok(()),
            Reader::Deadline => Error::Deadline,
            Reader::Clock => Error::Clock,
            Reader::Failed(reason) => Error::Read {
                reason: reason.clone(),
            },
            Reader::Panicked => Error::Panicked,
            Reader::Draining => Error::Settle,
        };
        let context = Context {
            status: Status::Exited(status.to_string().into_boxed_str()),
            tail: state.tail.bytes.clone().into_boxed_slice(),
            observed: state.tail.observed,
            truncated: state.tail.truncated,
            reader: state.reader.clone(),
        };
        drop(state);
        Err(Failure { source, context })
    }

    /// Boundedly retire diagnostics when no child exists to provide report context.
    pub(super) fn retire(mut self) {
        let _ = self.settle();
        self.join_if_retired();
    }

    /// Consume the capture into inert bounded context for an existing primary failure.
    pub(super) fn failure_context(self, child: &mut Child) -> Context {
        let Some(now_ns) = rawr_import::metrics::monotonic_ns() else {
            return self.failure_context_without_wait();
        };
        let deadline_ns = self.settlement_deadline(now_ns);
        self.failure_context_until(child, deadline_ns)
    }

    fn settle(&self) -> bool {
        let Some(now_ns) = rawr_import::metrics::monotonic_ns() else {
            return false;
        };
        self.settle_until(self.settlement_deadline(now_ns))
    }

    fn settle_until(&self, deadline_ns: u64) -> bool {
        let state = lock(&self.shared.state);
        if !matches!(state.reader, Reader::Draining) {
            return true;
        }
        let Some(now_ns) = rawr_import::metrics::monotonic_ns() else {
            return false;
        };
        let Some(wait_ns) = deadline_ns.checked_sub(now_ns) else {
            return false;
        };
        let (state, _) = match self.shared.changed.wait_timeout_while(
            state,
            Duration::from_nanos(wait_ns),
            |state| matches!(state.reader, Reader::Draining),
        ) {
            Ok(result) => result,
            Err(poisoned) => poisoned.into_inner(),
        };
        !matches!(state.reader, Reader::Draining)
    }

    fn settlement_deadline(&self, now_ns: u64) -> u64 {
        now_ns
            .checked_add(SETTLE_WAIT.as_nanos() as u64)
            .unwrap_or(self.deadline_ns)
            .min(self.deadline_ns)
    }

    fn failure_context_until(mut self, child: &mut Child, deadline_ns: u64) -> Context {
        let status = loop {
            let Some(before_ns) = rawr_import::metrics::monotonic_ns() else {
                break Status::Running;
            };
            if !within_settlement(before_ns, before_ns, deadline_ns) {
                break Status::Running;
            }
            let observation = child.try_wait();
            let Some(after_ns) = rawr_import::metrics::monotonic_ns() else {
                break Status::Running;
            };
            if !within_settlement(before_ns, after_ns, deadline_ns) {
                break Status::Running;
            }
            match observation {
                Ok(Some(status)) => {
                    let _ = self.settle_until(deadline_ns);
                    break Status::Exited(status.to_string().into_boxed_str());
                }
                Ok(None) => {
                    let remaining_ns = deadline_ns - after_ns;
                    let wait_ns = remaining_ns.min(READ_WAIT.as_nanos() as u64);
                    thread::park_timeout(Duration::from_nanos(wait_ns));
                }
                Err(error) => {
                    break Status::InspectionFailed(error.to_string().into_boxed_str());
                }
            }
        };
        self.join_if_retired();
        self.context(status)
    }

    fn failure_context_without_wait(mut self) -> Context {
        self.join_if_retired();
        self.context(Status::Running)
    }

    fn complete(&self) -> bool {
        !matches!(lock(&self.shared.state).reader, Reader::Draining)
    }

    fn join(&mut self) {
        if let Some(handle) = self.handle.take()
            && handle.join().is_err()
        {
            let mut state = lock(&self.shared.state);
            state.reader = Reader::Panicked;
            self.shared.changed.notify_all();
        }
    }

    fn join_if_retired(&mut self) {
        let worker_finished = self
            .handle
            .as_ref()
            .is_some_and(thread::JoinHandle::is_finished);
        if self.complete() || worker_finished {
            self.join();
        }
    }

    fn context(&self, status: Status) -> Context {
        let state = lock(&self.shared.state);
        Context {
            status,
            tail: state.tail.bytes.clone().into_boxed_slice(),
            observed: state.tail.observed,
            truncated: state.tail.truncated,
            reader: state.reader.clone(),
        }
    }
}

fn within_settlement(before_ns: u64, after_ns: u64, deadline_ns: u64) -> bool {
    before_ns < deadline_ns && after_ns < deadline_ns
}

impl Failure {
    pub(super) fn into_parts(self) -> (Error, Context) {
        (self.source, self.context)
    }
}

impl Tail {
    fn new() -> Self {
        Self {
            bytes: Vec::with_capacity(super::OUTPUT_BYTES),
            observed: 0,
            truncated: false,
        }
    }

    fn push(&mut self, bytes: &[u8]) {
        self.observed = match self.observed.checked_add(bytes.len() as u64) {
            Some(observed) => observed,
            None => {
                self.truncated = true;
                u64::MAX
            }
        };
        if bytes.len() >= super::OUTPUT_BYTES {
            self.bytes.clear();
            self.bytes
                .extend_from_slice(&bytes[bytes.len() - super::OUTPUT_BYTES..]);
            self.truncated |= self.observed > super::OUTPUT_BYTES as u64;
            return;
        }
        let excess = self
            .bytes
            .len()
            .saturating_add(bytes.len())
            .saturating_sub(super::OUTPUT_BYTES);
        if excess > 0 {
            self.bytes.drain(..excess);
            self.truncated = true;
        }
        self.bytes.extend_from_slice(bytes);
    }
}

fn drain(mut reader: UnixStream, deadline_ns: u64, shared: Arc<Shared>) {
    let mut bytes = [0_u8; READ_BYTES];
    loop {
        let observed_ns = match rawr_import::metrics::monotonic_ns() {
            Some(observed_ns) if observed_ns < deadline_ns => observed_ns,
            Some(_) => return complete(&shared, Reader::Deadline),
            None => return complete(&shared, Reader::Clock),
        };
        match reader.read(&mut bytes) {
            Ok(0) => return complete(&shared, Reader::Complete),
            Ok(count) => lock(&shared.state).tail.push(&bytes[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                let remaining_ns = deadline_ns.saturating_sub(observed_ns);
                let wait_ns = remaining_ns.min(READ_WAIT.as_nanos() as u64);
                thread::park_timeout(Duration::from_nanos(wait_ns));
            }
            Err(error) => {
                return complete(&shared, Reader::Failed(error.to_string().into_boxed_str()));
            }
        }
    }
}

fn complete(shared: &Shared, outcome: Reader) {
    lock(&shared.state).reader = outcome;
    shared.changed.notify_all();
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt as _;
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    fn completed(tail: super::Tail) -> super::Capture {
        super::Capture {
            shared: Arc::new(super::Shared {
                state: Mutex::new(super::State {
                    tail,
                    reader: super::Reader::Complete,
                }),
                changed: Condvar::new(),
            }),
            handle: None,
            deadline_ns: u64::MAX,
        }
    }

    fn draining() -> super::Capture {
        super::Capture {
            shared: Arc::new(super::Shared {
                state: Mutex::new(super::State {
                    tail: super::Tail::new(),
                    reader: super::Reader::Draining,
                }),
                changed: Condvar::new(),
            }),
            handle: None,
            deadline_ns: u64::MAX,
        }
    }

    fn spawned(
        script: &str,
        envelope: rawr_import::metrics::evidence::Envelope,
    ) -> (std::process::Child, super::Capture) {
        let (child_stderr, capture) =
            super::Capture::transform(envelope).expect("bounded stderr capture transforms");
        let child = {
            let mut command = std::process::Command::new("/bin/sh");
            command
                .args(["-c", script])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(child_stderr);
            command.spawn().expect("diagnostic child spawns")
        };
        (child, capture)
    }

    #[test]
    fn retained_tail_has_one_exact_bound_and_overflow_fails_closed() {
        let status = std::process::ExitStatus::from_raw(0);
        let mut exact_tail = super::Tail::new();
        let exact = vec![b'a'; super::super::OUTPUT_BYTES];
        exact_tail.push(&exact);
        assert_eq!(exact_tail.bytes.len(), super::super::OUTPUT_BYTES);
        assert_eq!(exact_tail.observed, exact.len() as u64);
        assert!(!exact_tail.truncated);
        completed(exact_tail)
            .finish(&status)
            .expect("the exact retained bound remains admissible");

        let mut overflow_tail = super::Tail::new();
        overflow_tail.push(&exact);
        overflow_tail.push(&[b'z'; 37]);
        assert_eq!(overflow_tail.bytes.len(), super::super::OUTPUT_BYTES);
        assert_eq!(overflow_tail.observed, (exact.len() + 37) as u64);
        assert!(overflow_tail.truncated);
        assert!(overflow_tail.bytes.ends_with(&[b'z'; 37]));
        assert!(matches!(
            completed(overflow_tail).finish(&status),
            Err(super::Failure {
                source: super::Error::Limit { limit },
                ..
            }) if limit == super::super::OUTPUT_BYTES
        ));
    }

    #[test]
    fn delayed_exit_settles_to_terminal_status_and_stderr() {
        let origin_ns = rawr_import::metrics::monotonic_ns().expect("monotonic clock is available");
        let envelope = rawr_import::metrics::evidence::Envelope::transform(origin_ns)
            .expect("absolute evidence envelope seals");
        let (mut child, capture) = spawned(
            "sleep 0.05; printf '%s' 'delayed diagnostic' >&2; exit 29",
            envelope,
        );
        let started = Instant::now();
        let context = capture.failure_context(&mut child);
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(matches!(
            context.status,
            super::Status::Exited(ref status) if status.contains("29")
        ));
        assert_eq!(context.tail.as_ref(), b"delayed diagnostic");
        assert_eq!(context.observed, 18);
        assert!(!context.truncated);
        assert!(matches!(context.reader, super::Reader::Complete));
    }

    #[test]
    fn still_running_settlement_is_truthful_and_envelope_bounded() {
        let origin_ns = rawr_import::metrics::monotonic_ns().expect("monotonic clock is available");
        let envelope = rawr_import::metrics::evidence::Envelope::transform(origin_ns)
            .expect("absolute evidence envelope seals");
        let (mut child, capture) = spawned("sleep 0.10; exit 0", envelope);
        assert_eq!(
            capture.settlement_deadline(origin_ns),
            origin_ns + super::SETTLE_WAIT.as_nanos() as u64
        );
        let now_ns =
            rawr_import::metrics::monotonic_ns().expect("monotonic clock remains available");
        let short_deadline_ns = now_ns + 5_000_000;
        let started = Instant::now();
        let context = capture.failure_context_until(&mut child, short_deadline_ns);
        assert!(started.elapsed() < Duration::from_millis(500));
        assert!(matches!(context.status, super::Status::Running));
        assert!(context.tail.is_empty());
        assert!(matches!(context.reader, super::Reader::Draining));
        let status = child.wait().expect("synthetic child retires naturally");
        assert!(status.success());

        let near_envelope = rawr_import::metrics::evidence::Envelope::transform(1)
            .expect("reference envelope seals");
        let near_now_ns = near_envelope.deadline_ns() - 5_000_000;
        let near_capture = completed(super::Tail::new());
        let near_capture = super::Capture {
            deadline_ns: near_envelope.deadline_ns(),
            ..near_capture
        };
        assert_eq!(
            near_capture.settlement_deadline(near_now_ns),
            near_envelope.deadline_ns()
        );
    }

    #[test]
    fn post_deadline_terminal_status_is_not_admitted() {
        let deadline_ns = rawr_import::metrics::monotonic_ns()
            .expect("monotonic deadline is available immediately before spawn");
        let mut child = std::process::Command::new("/bin/sh")
            .args(["-c", "exit 41"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("post-deadline child spawns");
        let status = child.wait().expect("post-deadline child retires");
        assert_eq!(status.code(), Some(41));
        assert!(
            rawr_import::metrics::monotonic_ns().expect("monotonic clock remains available")
                > deadline_ns
        );

        let context = draining().failure_context_until(&mut child, deadline_ns);
        assert!(matches!(context.status, super::Status::Running));
        assert!(context.tail.is_empty());
        assert!(matches!(context.reader, super::Reader::Draining));
    }

    #[test]
    fn finish_before_does_not_restart_an_expired_retirement_grace() {
        let status = std::process::ExitStatus::from_raw(0);
        let started = Instant::now();
        let failure = draining()
            .finish_before(&status, 1)
            .expect_err("expired grace rejects immediately");
        assert!(matches!(failure.source, super::Error::Deadline));
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(completed(super::Tail::new())
            .finish_before(&status, 1)
            .is_err());
    }

    #[test]
    fn settlement_admission_requires_both_clock_samples_before_deadline() {
        assert!(super::within_settlement(99, 99, 100));
        assert!(!super::within_settlement(99, 100, 100));
        assert!(!super::within_settlement(99, 101, 100));
        assert!(!super::within_settlement(100, 100, 100));
    }

    #[test]
    fn settlement_deadline_overflow_falls_back_to_retained_envelope() {
        let envelope = rawr_import::metrics::evidence::Envelope::transform(1)
            .expect("reference envelope seals");
        let capture = completed(super::Tail::new());
        let capture = super::Capture {
            deadline_ns: envelope.deadline_ns(),
            ..capture
        };
        assert_eq!(
            capture.settlement_deadline(u64::MAX - 1),
            envelope.deadline_ns()
        );
    }
}
