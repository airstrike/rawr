//! Parent-owned observer artifact, nonblocking readers and finite lifecycle.

use std::io::Read as _;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt as _;
use std::process::{Child, ExitStatus, Stdio};

use rawr_import::metrics::{evidence, report};

#[path = "../../qualification/protocol.rs"]
pub(super) mod protocol;

use protocol::{Command, Error, Event};

#[derive(Clone, Debug, serde::Serialize)]
pub(super) struct Report {
    pub(super) build: report::Build,
    pub(super) artifact: evidence::process::file::Identity,
    pub(super) sdk: super::provenance::Sdk,
    pub(super) visible: protocol::Visible,
    pub(super) costs: protocol::Costs,
    pub(super) spawned_ns: u64,
    pub(super) reaped_ns: u64,
    pub(super) disappeared_ns: u64,
}

pub(super) struct Observer {
    child: Child,
    channel: protocol::Channel,
    stderr: UnixStream,
    diagnostic: Vec<u8>,
    stderr_eof: bool,
    status: Option<ExitStatus>,
    reaped_ns: Option<u64>,
    artifact: evidence::process::file::Identity,
    build: report::Build,
    sdk: super::provenance::Sdk,
    spawned_ns: u64,
    phase: Phase,
    origin_ns: u64,
    deadline_ns: u64,
    title: evidence::title::Identity,
    receipt: Option<evidence::runner::Receipt>,
    visible: Option<protocol::Visible>,
    pressed_ns: Option<u64>,
    disappeared_ns: Option<u64>,
    costs: Option<protocol::Costs>,
    cleanup_stderr_error: Option<String>,
    cleanup_protocol_error: Option<String>,
    cleanup_wait_error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Initializing,
    Ready,
    Starting,
    Started,
    Bound,
    Visible,
    Closing,
    Pressed,
    Disappeared,
    Finishing,
    Terminal,
}

impl Observer {
    pub(super) fn transform(
        launch: [u8; 32],
        executable: evidence::process::file::Identity,
        title: evidence::title::Identity,
    ) -> Result<Self, super::Error> {
        let path = super::provenance::artifact("observer")?;
        let provenance = super::provenance::load(&path, super::provenance::Kind::Observer)?;
        let build = provenance.report()?;
        let sdk = provenance
            .sdk()
            .ok_or_else(|| super::Error::Observer(Error::Invalid("observer SDK receipt absent")))?;
        let artifact = evidence::process::file::Identity::read(&path)?;
        if &artifact != provenance.executable()? {
            return Err(super::Error::Observer(Error::Invalid(
                "observer artifact changed after provenance verification",
            )));
        }
        let result = (|| {
            let (stream, child_stream) = UnixStream::pair()?;
            let (stderr, child_stderr) = UnixStream::pair()?;
            stderr.set_nonblocking(true)?;
            let child_stream: OwnedFd = child_stream.into();
            let child_stderr: OwnedFd = child_stderr.into();
            let channel = protocol::Channel::transform(stream, launch, artifact.digest)?;
            let spawned_ns = protocol::now()?;
            let deadline_ns = spawned_ns
                .checked_add(protocol::GRACE_NS)
                .ok_or(Error::Invalid("observer startup clock"))?;
            let child = std::process::Command::new(&path)
                .arg(evidence::hex(&launch))
                .arg(evidence::hex(&artifact.digest))
                .stdin(Stdio::from(child_stream))
                .stdout(Stdio::null())
                .stderr(Stdio::from(child_stderr))
                .process_group(0)
                .spawn()?;
            let mut observer = Self {
                child,
                channel,
                stderr,
                diagnostic: Vec::new(),
                stderr_eof: false,
                status: None,
                reaped_ns: None,
                artifact: artifact.clone(),
                build,
                sdk,
                spawned_ns,
                phase: Phase::Initializing,
                origin_ns: 0,
                deadline_ns,
                title: title.clone(),
                receipt: None,
                visible: None,
                pressed_ns: None,
                disappeared_ns: None,
                costs: None,
                cleanup_stderr_error: None,
                cleanup_protocol_error: None,
                cleanup_wait_error: None,
            };
            let start: Result<(), Error> = (|| {
                observer.channel.send(Command::Initialize {
                    observer: artifact,
                    executable,
                    title,
                })?;
                while observer.phase != Phase::Ready {
                    observer.tick()?;
                    protocol::before(deadline_ns)?;
                    std::thread::sleep(protocol::POLL);
                }
                Ok(())
            })();
            if let Err(error) = start {
                let signal = observer.cancel();
                let cleanup_deadline = protocol::now()
                    .ok()
                    .and_then(|now| now.checked_add(protocol::GRACE_NS))
                    .unwrap_or(deadline_ns);
                let cleanup = observer.settle(cleanup_deadline);
                return Err(Error::Observation(
                    format!("{error}; observer signal={signal}; cleanup={cleanup}")
                        .into_boxed_str(),
                ));
            }
            Ok(observer)
        })();
        result.map_err(super::Error::Observer)
    }

    pub(super) fn begin(
        &mut self,
        origin_ns: u64,
        window: super::timing::Window,
    ) -> Result<(), Error> {
        if self.phase != Phase::Ready {
            return Err(Error::Invalid("observer start sequence"));
        }
        self.origin_ns = origin_ns;
        self.deadline_ns = window.deadline_ns();
        self.phase = Phase::Starting;
        self.channel.send(Command::Start {
            origin_ns,
            deadline_ns: self.deadline_ns,
        })?;
        while self.phase != Phase::Started {
            self.tick()?;
            protocol::before(self.deadline_ns)?;
            std::thread::sleep(protocol::POLL);
        }
        Ok(())
    }

    pub(super) fn bind(&mut self, receipt: &evidence::runner::Receipt) -> Result<(), Error> {
        if self.phase != Phase::Started {
            return Err(Error::Invalid("observer bind sequence"));
        }
        self.receipt = Some(receipt.clone());
        self.phase = Phase::Bound;
        self.channel.send(Command::Bind(receipt.clone()))
    }

    pub(super) fn poll_visible(&mut self) -> Result<Option<protocol::Visible>, Error> {
        self.tick()?;
        protocol::before(self.deadline_ns)?;
        Ok(if self.phase == Phase::Visible {
            self.visible.clone()
        } else {
            None
        })
    }

    pub(super) fn close(&mut self) -> Result<(), Error> {
        if self.phase != Phase::Visible {
            return Err(Error::Invalid("close before visibility/resource seal"));
        }
        self.deadline_ns = protocol::now()?
            .checked_add(protocol::GRACE_NS)
            .ok_or(Error::Invalid("observer close clock"))?;
        self.phase = Phase::Closing;
        self.channel.send(Command::Close)
    }

    pub(super) fn retirement(&mut self) -> Result<(Option<u64>, bool), Error> {
        self.tick()?;
        protocol::before(self.deadline_ns)?;
        Ok((self.pressed_ns, self.disappeared_ns.is_some()))
    }

    pub(super) fn finish(&mut self) -> Result<(), Error> {
        if self.phase != Phase::Disappeared {
            return Err(Error::Invalid("finish before disappearance"));
        }
        self.phase = Phase::Finishing;
        self.channel.send(Command::Finish)
    }

    pub(super) fn completed(&mut self) -> Result<Option<Report>, Error> {
        self.tick()?;
        protocol::before(self.deadline_ns)?;
        if self.phase != Phase::Terminal
            || !self.channel.eof()
            || !self.stderr_eof
            || self.status.is_none()
        {
            return Ok(None);
        }
        if !self.status.is_some_and(|status| status.success()) || !self.diagnostic.is_empty() {
            return Err(Error::Invalid("observer did not retire normally"));
        }
        Ok(Some(Report {
            build: self.build.clone(),
            artifact: self.artifact.clone(),
            sdk: self.sdk.clone(),
            visible: self
                .visible
                .clone()
                .ok_or(Error::Invalid("visible observation absent"))?,
            costs: self
                .costs
                .clone()
                .ok_or(Error::Invalid("observer costs absent"))?,
            spawned_ns: self.spawned_ns,
            reaped_ns: self
                .reaped_ns
                .ok_or(Error::Invalid("observer reap absent"))?,
            disappeared_ns: self
                .disappeared_ns
                .ok_or(Error::Invalid("window disappearance absent"))?,
        }))
    }

    fn tick(&mut self) -> Result<(), Error> {
        self.drain()?;
        if let Some(event) = self.channel.poll::<Event>()? {
            match (self.phase, event) {
                (Phase::Initializing, Event::Ready) => self.phase = Phase::Ready,
                (Phase::Starting, Event::Started) => self.phase = Phase::Started,
                (Phase::Bound, Event::Visible(visible)) => {
                    let receipt = self
                        .receipt
                        .as_ref()
                        .ok_or(Error::Invalid("runner receipt absent"))?;
                    if visible.cli.pid != receipt.cli.pid
                        || visible.runner.pid != receipt.runner.pid
                        || visible.runner.parent != receipt.cli.pid
                        || visible.runner.group != receipt.runner.pid
                        || visible.window.pid != i64::from(receipt.runner.pid)
                        || visible.window.title.as_bytes() != self.title.bytes()
                        || visible.observed_ns < self.origin_ns
                        || visible.observed_ns > self.deadline_ns
                        || visible.executable.path != visible.runner.path
                        || visible.executable.digest != receipt.executable_digest
                        || visible.executable.size != receipt.executable_bytes
                        || visible.resources.samples == 0
                    {
                        return Err(Error::Invalid("observer visibility association"));
                    }
                    self.visible = Some(visible);
                    self.phase = Phase::Visible;
                }
                (Phase::Closing, Event::Pressed { observed_ns }) => {
                    if observed_ns
                        < self
                            .visible
                            .as_ref()
                            .ok_or(Error::Invalid("visible observation absent"))?
                            .observed_ns
                        || observed_ns > protocol::now()?
                    {
                        return Err(Error::Invalid("observer press clock"));
                    }
                    self.deadline_ns = observed_ns
                        .checked_add(protocol::GRACE_NS)
                        .ok_or(Error::Invalid("observer retirement clock"))?;
                    self.pressed_ns = Some(observed_ns);
                    self.phase = Phase::Pressed;
                }
                (Phase::Pressed, Event::Disappeared { observed_ns }) => {
                    if observed_ns < self.pressed_ns.ok_or(Error::Invalid("press absent"))?
                        || observed_ns > self.deadline_ns
                    {
                        return Err(Error::Invalid("disappearance clock"));
                    }
                    self.disappeared_ns = Some(observed_ns);
                    self.phase = Phase::Disappeared;
                }
                (Phase::Finishing, Event::Finished { costs }) => {
                    if costs.sampled.samples == 0
                        || costs.sampler_children == 0
                        || costs.first_sample_ns < self.spawned_ns
                        || costs.last_sample_ns < costs.first_sample_ns
                        || costs.last_sample_ns > self.deadline_ns
                    {
                        return Err(Error::Invalid(
                            "observer cost observation absent or invalid",
                        ));
                    }
                    self.costs = Some(costs);
                    self.phase = Phase::Terminal;
                }
                (_, Event::Failed { reason }) => return Err(Error::Observation(reason)),
                _ => return Err(Error::Invalid("observer phase or duplicate event")),
            }
        }
        if self.channel.eof() && self.phase != Phase::Terminal {
            return Err(Error::Invalid("observer EOF before terminal protocol"));
        }
        if self.status.is_none()
            && let Some(status) = self.child.try_wait()?
        {
            self.status = Some(status);
            self.reaped_ns = Some(protocol::now()?);
            if !status.success() {
                return Err(Error::Observation(
                    format!(
                        "observer exit {status}: {}",
                        String::from_utf8_lossy(&self.diagnostic)
                    )
                    .into_boxed_str(),
                ));
            }
        }
        Ok(())
    }

    fn drain(&mut self) -> Result<(), Error> {
        if self.stderr_eof {
            return Ok(());
        }
        let mut buffer = [0; 4096];
        match self.stderr.read(&mut buffer) {
            Ok(0) => self.stderr_eof = true,
            Ok(count) => {
                if self.diagnostic.len().saturating_add(count) > protocol::OUTPUT_BYTES {
                    return Err(Error::Invalid("observer stderr bound"));
                }
                self.diagnostic
                    .try_reserve_exact(count)
                    .map_err(|_| Error::Invalid("observer diagnostic allocation"))?;
                self.diagnostic.extend_from_slice(&buffer[..count]);
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }

    /// Signal only; callers signal workload owners before any wait or drain.
    pub(super) fn cancel(&mut self) -> String {
        if self.status.is_some() {
            return "already reaped; no PID-based signal sent".into();
        }
        match self.child.try_wait() {
            Ok(Some(status)) => {
                self.status = Some(status);
                self.reaped_ns = protocol::now().ok();
                return format!("already exited and reaped ({status}); no PID-based signal sent");
            }
            Ok(None) => {}
            Err(error) => {
                self.cleanup_wait_error = Some(error.to_string());
                return format!("observer ownership uncertain after wait failure ({error}); no PID-based signal sent");
            }
        }
        let group = i32::try_from(self.child.id()).map(nix::unistd::Pid::from_raw);
        match group {
            Ok(group) => format!(
                "{:?}",
                nix::sys::signal::killpg(group, nix::sys::signal::Signal::SIGKILL)
            ),
            Err(error) => error.to_string(),
        }
    }

    pub(super) fn cleanup_poll(&mut self) -> bool {
        if let Err(error) = self.drain() {
            if self.cleanup_stderr_error.is_none() {
                self.cleanup_stderr_error = Some(error.to_string());
            }
        }
        if let Err(error) = self.channel.abandon() {
            if self.cleanup_protocol_error.is_none() {
                self.cleanup_protocol_error = Some(error.to_string());
            }
        }
        if self.status.is_none() {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    self.status = Some(status);
                    self.reaped_ns = protocol::now().ok();
                }
                Ok(None) => {}
                Err(error) => {
                    if self.cleanup_wait_error.is_none() {
                        self.cleanup_wait_error = Some(error.to_string());
                    }
                }
            }
        }
        self.status.is_some() && self.stderr_eof && self.channel.eof()
    }

    pub(super) fn cleanup_facts(&self) -> String {
        let reached = match super::release::encode(&(
            self.artifact.clone(),
            &self.visible,
            &self.costs,
            self.spawned_ns,
            self.pressed_ns,
            self.disappeared_ns,
        )) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(error) => format!("reached observer evidence unavailable: {error}"),
        };
        format!("reached={reached}; observer cleanup_stderr_error={:?}, cleanup_protocol_error={:?}, cleanup_wait_error={:?}; observer status={:?}, reaped_ns={:?}, protocol_eof={}, stderr_eof={}, phase={:?}, stderr={}", self.cleanup_stderr_error, self.cleanup_protocol_error, self.cleanup_wait_error, self.status, self.reaped_ns, self.channel.eof(), self.stderr_eof, self.phase, String::from_utf8_lossy(&self.diagnostic))
    }

    fn settle(&mut self, deadline: u64) -> String {
        while !self.cleanup_poll() && protocol::before(deadline).is_ok() {
            std::thread::sleep(protocol::POLL);
        }
        self.cleanup_facts()
    }
}

impl Drop for Observer {
    fn drop(&mut self) {
        // Defensive signal only. Success requires completed(); failures retain
        // actual reap/reader outcomes through the explicit combined cleanup.
        if self.status.is_none() {
            let _ = self.cancel();
        }
    }
}
