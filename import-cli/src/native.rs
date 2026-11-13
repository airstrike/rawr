//! Optional one-owner native runner replacement lane.

use std::path::Path;

/// CLI-owned native process slot and its exact cache/process authorities.
pub struct Lane<B> {
    supervisor: rawr_import::supervisor::Owner,
    cache: rawr_import::cache::Writer,
    process: rawr_import::process::Owner,
    slot: rawr_import::supervisor::slot::Slot<B>,
    refresh: bool,
}

/// Result of linearizing one started imported owner against its registered
/// watcher checkpoint.
pub(crate) enum Watched {
    Installed,
    Obsolete,
}

/// When the running program ends as a new runner replaces it.
#[derive(Clone, Copy)]
pub(crate) enum Handover {
    /// It runs until the new one has started, unless it has the terminal,
    /// so a new one that fails to start leaves it running.
    Overlap,
    /// It ends before the new `main` runs, so the terminal, a port or a
    /// lock it held is free (D943).
    Retire,
}

impl<B> Lane<B> {
    pub(crate) fn take_refresh(&mut self) -> Result<bool, Error> {
        let current = self.slot.take_refresh().map_err(Error::Process)?;
        Ok(std::mem::take(&mut self.refresh) | current)
    }

    pub(crate) fn native_watch(
        &self,
    ) -> Result<impl Iterator<Item = rawr_import::input::proof::Watch> + '_, Error> {
        self.slot.native_watch().map_err(Error::Process)
    }

    /// The identity of the running native build's input proof.
    pub(crate) fn native_fingerprint(
        &self,
    ) -> Result<rawr_import::input::fingerprint::Fingerprint, Error> {
        self.slot.native_fingerprint().map_err(Error::Process)
    }

    /// Why `miss` needs another runner than the running one, in one line.
    pub(crate) fn native_change(
        &self,
        miss: &rawr_import::process::hot::Miss<B>,
    ) -> Option<String> {
        self.slot.native_change(miss.request())
    }

    pub(crate) fn prepare_resident(
        &mut self,
        started: rawr_import::process::hot::Started,
        revision: rawr_import::supervisor::Revision,
        snapshot: &rawr_import::supervisor::snapshot::Bytes,
        freshness: rawr_import::supervisor::snapshot::attempt::Freshness,
        observation: &[u8],
    ) -> Result<rawr_import::process::resident::Outcome, Error> {
        self.slot
            .prepare_resident(started, revision, snapshot, freshness, observation)
            .map_err(Error::Process)
    }

    pub(crate) fn commit_resident(
        &mut self,
        prepared: rawr_import::process::resident::Prepared,
    ) -> Result<rawr_import::supervisor::GenerationKey, Error> {
        self.slot.commit_resident(prepared).map_err(Error::Process)
    }

    pub(crate) fn abort_resident(
        &mut self,
        prepared: rawr_import::process::resident::Prepared,
    ) -> Result<(), Error> {
        self.slot.abort_resident(prepared).map_err(Error::Process)
    }

    /// Bind the fixed cache and private process channel beneath one project.
    pub fn new(project: &Path) -> Result<Self, Error> {
        Self::for_lifecycle(project, rawr_import::process::Lifecycle::Check)
    }

    /// Bind a runner lane to one product lifecycle before any child spawns.
    pub fn for_lifecycle(
        project: &Path,
        lifecycle: rawr_import::process::Lifecycle,
    ) -> Result<Self, Error> {
        Self::for_program(project, lifecycle, None)
    }

    /// [`Self::for_lifecycle`], whose runners get `program`'s arguments and
    /// stdin under `rawr run`.
    pub fn for_program(
        project: &Path,
        lifecycle: rawr_import::process::Lifecycle,
        program: Option<rawr_import::process::Arguments>,
    ) -> Result<Self, Error> {
        let mut process = rawr_import::process::Owner::for_lifecycle(project, lifecycle)?;
        if let Some(program) = program {
            process = process.with_program(program);
        }
        Ok(Self {
            supervisor: rawr_import::supervisor::Owner::new(),
            cache: rawr_import::cache::Writer::new(project)?,
            process,
            slot: rawr_import::supervisor::slot::Slot::new(),
            refresh: false,
        })
    }

    /// Allocate the next exact compilation revision from this lane's sole
    /// supervisor authority.
    pub fn revision(&mut self) -> Result<rawr_import::supervisor::Revision, Error> {
        self.supervisor.revision().map_err(Error::Supervisor)
    }

    /// Build or revalidate one typed runner, release the global native Cargo
    /// lane, finish staged admission, then directly replace the process route.
    pub fn replace(
        &mut self,
        generated: rawr_import::runner::Manifest<rawr_import::runner::Generated, B>,
        snapshot: rawr_import::supervisor::snapshot::Bytes,
    ) -> Result<rawr_import::supervisor::GenerationKey, Error> {
        let built = self.build(generated)?;
        let admitted = self.admit_built(built, snapshot)?;
        self.install(admitted)
    }

    /// Build and start one fresh live epoch before swapping it into the slot,
    /// so a startup rejection leaves the current last-good child untouched.
    pub fn replace_started(
        &mut self,
        generated: rawr_import::runner::Manifest<rawr_import::runner::Generated, B>,
        snapshot: rawr_import::supervisor::snapshot::Bytes,
    ) -> Result<rawr_import::supervisor::GenerationKey, Error> {
        let built = self.build(generated)?;
        self.replace_built_started(built, snapshot)
    }

    pub(crate) fn replace_built_started(
        &mut self,
        built: rawr_import::runner::Manifest<rawr_import::runner::Built, B>,
        snapshot: rawr_import::supervisor::snapshot::Bytes,
    ) -> Result<rawr_import::supervisor::GenerationKey, Error> {
        let mut admitted = self.admit_built(built, snapshot)?;
        if let Err(error) = admitted.start() {
            drop(admitted.shutdown());
            return Err(Error::Process(error));
        }
        self.install(admitted)
    }

    pub(crate) fn replace_built(
        &mut self,
        built: rawr_import::runner::Manifest<rawr_import::runner::Built, B>,
        snapshot: rawr_import::supervisor::snapshot::Bytes,
    ) -> Result<rawr_import::supervisor::GenerationKey, Error> {
        let admitted = self.admit_built(built, snapshot)?;
        self.install(admitted)
    }

    pub(crate) fn build(
        &mut self,
        generated: rawr_import::runner::Manifest<rawr_import::runner::Generated, B>,
    ) -> Result<rawr_import::runner::Manifest<rawr_import::runner::Built, B>, Error> {
        let built = generated
            .build_on_demand(&self.cache)
            .map_err(Error::Build)?;
        rawr_import::universe::deliver(&built).map_err(Error::Deliver)?;
        Ok(built)
    }

    fn admit_built(
        &mut self,
        built: rawr_import::runner::Manifest<rawr_import::runner::Built, B>,
        snapshot: rawr_import::supervisor::snapshot::Bytes,
    ) -> Result<rawr_import::process::Candidate<rawr_import::process::Admitted, B>, Error> {
        self.supervisor
            .bootstrap(built, snapshot, &self.process)
            .map_err(Error::Bootstrap)
    }

    fn admit_built_observed(
        &mut self,
        built: rawr_import::runner::Manifest<rawr_import::runner::Built, B>,
        snapshot: rawr_import::supervisor::snapshot::Bytes,
        observation: &[u8],
        current: rawr_import::supervisor::bootstrap::Current,
    ) -> Result<rawr_import::process::Candidate<rawr_import::process::Admitted, B>, Error> {
        self.supervisor
            .bootstrap_observed(built, snapshot, &self.process, observation, current)
            .map_err(|error| match error {
                // The runner's own failures, the walk of its read set as it
                // observes among them, are a running runner's.
                rawr_import::supervisor::bootstrap::Error::Process(failure) => {
                    Error::Process(failure)
                }
                error => Error::Bootstrap(error),
            })
    }

    pub(crate) fn replace_built_started_watched(
        &mut self,
        built: rawr_import::runner::Manifest<rawr_import::runner::Built, B>,
        snapshot: rawr_import::supervisor::snapshot::Bytes,
        adapter: &rawr_watch::Adapter,
        observation: &[u8],
        checkpoint: rawr::reload::scheduler::Checkpoint,
    ) -> Result<Watched, Error> {
        let admitted = self.admit_built_observed(
            built,
            snapshot,
            observation,
            rawr_import::supervisor::bootstrap::Current::Walked,
        )?;
        match adapter.admit_checkpoint(checkpoint, admitted, |candidate| {
            self.slot.install_latest(candidate)
        }) {
            rawr::reload::scheduler::Admission::Current(installation) => self
                .finish_watched_installation(installation, Handover::Overlap)
                .map(|_| Watched::Installed),
            rawr::reload::scheduler::Admission::Obsolete(candidate) => {
                candidate.shutdown()?;
                Ok(Watched::Obsolete)
            }
        }
    }

    pub(crate) fn replace_built_started_request(
        &mut self,
        built: rawr_import::runner::Manifest<rawr_import::runner::Built, B>,
        snapshot: rawr_import::supervisor::snapshot::Bytes,
        adapter: &rawr_watch::Adapter,
        observation: &[u8],
        request: rawr::reload::scheduler::Request,
        handover: Handover,
    ) -> Result<Watched, Error> {
        let admitted = self.admit_built_observed(
            built,
            snapshot,
            observation,
            rawr_import::supervisor::bootstrap::Current::Walked,
        )?;
        match adapter.admit_current(request.complete(admitted), |candidate| {
            self.slot.install_latest(candidate)
        }) {
            rawr::reload::scheduler::Admission::Current(installation) => self
                .finish_watched_installation(installation, handover)
                .map(|_| Watched::Installed),
            rawr::reload::scheduler::Admission::Obsolete(candidate) => {
                candidate.shutdown()?;
                Ok(Watched::Obsolete)
            }
        }
    }

    /// Start the program again in a new process of the running runner's own
    /// executable, from `snapshot`, which `observation` watches: a restart
    /// that needs no native build (D553.6). The running runner ends first,
    /// so its program lets go of the terminal, a port or a lock before the
    /// new `main` runs. Nothing of it carries over: the new program starts
    /// with fresh memory, statics and `main`. Its build, its store pins and
    /// the watcher stay. On an error no runner runs, and the program takes
    /// the full start. With the watcher's word that no native input changed
    /// since the running runner was checked, the new process doesn't walk
    /// its read set again (#1755).
    pub(crate) fn respawn(
        &mut self,
        snapshot: rawr_import::supervisor::snapshot::Bytes,
        observation: &[u8],
        unchanged: Option<rawr_watch::native::Unchanged>,
    ) -> Result<rawr_import::supervisor::GenerationKey, Error> {
        let walk = match unchanged {
            Some(_) => rawr_import::supervisor::bootstrap::Current::Watched,
            None => rawr_import::supervisor::bootstrap::Current::Walked,
        };
        let current = self
            .slot
            .take()
            .ok_or(Error::State("before it restarted"))?;
        let mut built = current.retire_keeping_manifest()?;
        built.rebind_startup(&snapshot).map_err(Error::Node)?;
        let admitted = self.admit_built_observed(built, snapshot, observation, walk)?;
        if let Some(previous) = self.slot.install(admitted) {
            previous.shutdown()?;
        }
        let started = self.slot.start().map_err(Error::Process).and_then(|()| {
            match self.slot.pin_current_live_last_good() {
                // A save as the program started (D531.9): it runs, and the
                // save reaches it as any other does.
                Ok(()) | Err(rawr_import::node::Error::SourceChanged(_)) => Ok(()),
                Err(failure) => Err(Error::Node(failure)),
            }
        });
        if let Err(failure) = started {
            let candidate = self
                .slot
                .take()
                .ok_or(Error::State("before its failed restart was stopped"))?;
            return match candidate.shutdown() {
                Ok(()) => Err(failure),
                Err(cleanup) => Err(Error::InstallCleanup {
                    failure: Box::new(failure),
                    cleanup,
                }),
            };
        }
        self.slot.key().ok_or(Error::State("after it restarted"))
    }

    fn finish_watched_installation(
        &mut self,
        installation: rawr_import::supervisor::slot::Installation<B>,
        handover: Handover,
    ) -> Result<rawr_import::supervisor::GenerationKey, Error> {
        let mut retired = match installation {
            rawr_import::supervisor::slot::Installation::Installed(retired) => retired,
            rawr_import::supervisor::slot::Installation::Obsolete(candidate) => {
                candidate.shutdown()?;
                return Err(Error::Obsolete);
            }
        };
        // Two programs can't share a terminal: one that has it ends before
        // the next starts, which then gets it, and a failed start has no
        // program to go back to. A retiring handover ends it first too.
        if let Some(previous) = retired
            .take_if(|previous| matches!(handover, Handover::Retire) || previous.has_terminal())
        {
            previous.retire(&mut self.refresh)?;
        }
        if let Err(failure) = self.slot.start() {
            return self.rollback_watched(retired, Error::Process(failure));
        }
        match self.slot.pin_current_live_last_good() {
            // A save as the program started (D531.9): it keeps running,
            // and the save swaps in or restarts it as any other does. Its
            // generation stays pinned as prepared, not as live.
            Ok(()) | Err(rawr_import::node::Error::SourceChanged(_)) => {}
            Err(failure) => return self.rollback_watched(retired, Error::Node(failure)),
        }
        let key = self.slot.key().ok_or(Error::State("after it started"))?;
        if let Some(retired) = retired {
            retired.retire(&mut self.refresh)?;
        }
        Ok(key)
    }

    fn rollback_watched(
        &mut self,
        retired: Option<rawr_import::process::Candidate<rawr_import::process::Admitted, B>>,
        failure: Error,
    ) -> Result<rawr_import::supervisor::GenerationKey, Error> {
        let candidate = self
            .slot
            .take()
            .ok_or(Error::State("before its failed start was rolled back"))?;
        if let Some(previous) = retired {
            self.slot.install(previous);
        }
        match candidate.shutdown() {
            Ok(()) => Err(failure),
            Err(cleanup) => Err(Error::InstallCleanup {
                failure: Box::new(failure),
                cleanup,
            }),
        }
    }

    fn install(
        &mut self,
        admitted: rawr_import::process::Candidate<rawr_import::process::Admitted, B>,
    ) -> Result<rawr_import::supervisor::GenerationKey, Error> {
        let installation = self.slot.install_latest(admitted);
        self.finish_installation(installation)
    }

    fn finish_installation(
        &mut self,
        installation: rawr_import::supervisor::slot::Installation<B>,
    ) -> Result<rawr_import::supervisor::GenerationKey, Error> {
        match installation {
            rawr_import::supervisor::slot::Installation::Installed(retired) => {
                let key = self
                    .slot
                    .key()
                    .ok_or(Error::State("after it was installed"))?;
                if let Err(failure) = self.slot.pin_current_live_last_good() {
                    let candidate = self
                        .slot
                        .take()
                        .ok_or(Error::State("before its failed install was rolled back"))?;
                    if let Some(previous) = retired {
                        self.slot.install(previous);
                    }
                    return match candidate.shutdown() {
                        Ok(()) => Err(Error::Node(failure)),
                        Err(cleanup) => Err(Error::PinCleanup { failure, cleanup }),
                    };
                }
                if let Some(retired) = retired {
                    retired.retire(&mut self.refresh)?;
                }
                Ok(key)
            }
            rawr_import::supervisor::slot::Installation::Obsolete(candidate) => {
                candidate.shutdown()?;
                Err(Error::Obsolete)
            }
        }
    }

    /// Hot-admit a compatible request in the current process. A miss returns
    /// the complete generation inputs so the caller can coalesce before Cargo.
    pub fn compile(
        &mut self,
        request: rawr_import::bridge::GenerationRequestKey<B>,
        snapshot: rawr_import::supervisor::snapshot::Bytes,
    ) -> Result<rawr_import::process::hot::Outcome<B>, Error>
    where
        B: rawr_import::surface::Runner,
    {
        self.slot.compile(request, snapshot).map_err(Error::Process)
    }

    /// Capture the child's request before mutable native preparation.
    pub(crate) fn begin_compile(
        &mut self,
    ) -> Result<Option<rawr_import::process::hot::Started>, Error> {
        self.slot.begin_compile().map_err(Error::Process)
    }

    pub(crate) fn cancel_compile(
        &mut self,
        started: Option<rawr_import::process::hot::Started>,
    ) -> Result<(), Error> {
        self.slot.cancel_compile(started).map_err(Error::Process)
    }

    pub(crate) fn prepare_started_compile(
        &mut self,
        started: Option<rawr_import::process::hot::Started>,
        request: rawr_import::bridge::GenerationRequestKey<B>,
        snapshot: rawr_import::supervisor::snapshot::Bytes,
        observation: &[u8],
    ) -> Result<rawr_import::process::hot::Preparation<B>, Error>
    where
        B: rawr_import::surface::Runner,
    {
        self.slot
            .prepare_started_compile(started, request, snapshot, Some(observation))
            .map_err(Error::Process)
    }

    /// Revalidate after watcher registration and abort a rejected preparation.
    pub(crate) fn revalidate_compile(
        &mut self,
        prepared: rawr_import::process::hot::Prepared<B>,
    ) -> Result<rawr_import::process::hot::Prepared<B>, Error> {
        if let Err(failure) = prepared.revalidate_prepared() {
            return match self.slot.abort_compile(prepared) {
                Ok(()) => Err(Error::Node(failure)),
                Err(cleanup) => Err(Error::PinCleanup { failure, cleanup }),
            };
        }
        Ok(prepared)
    }

    /// Commit one watcher-admitted same-process generation.
    pub(crate) fn commit_compile(
        &mut self,
        prepared: rawr_import::process::hot::Prepared<B>,
    ) -> Result<rawr_import::supervisor::GenerationKey, Error> {
        self.slot.commit_compile(prepared).map_err(Error::Process)
    }

    /// Abort one watcher-obsolete same-process generation.
    pub(crate) fn abort_compile(
        &mut self,
        prepared: rawr_import::process::hot::Prepared<B>,
    ) -> Result<(), Error> {
        self.slot.abort_compile(prepared).map_err(Error::Process)
    }

    /// Borrow the complete current native generation identity.
    #[must_use]
    pub fn current_key(&self) -> Option<rawr_import::supervisor::GenerationKey> {
        self.slot.key()
    }

    /// Send one bounded message through the admitted profile route.
    pub fn message<M: rawr_import::profile::message::Message>(
        &mut self,
        message: M,
    ) -> Result<rawr_import::profile::message::Envelope<M>, Error> {
        self.slot.message(message).map_err(Error::Process)
    }

    /// Validate queued profile work against the current exact route.
    pub fn validate_message<M>(
        &self,
        envelope: &rawr_import::profile::message::Envelope<M>,
    ) -> Result<(), Error> {
        self.slot.validate_message(envelope).map_err(Error::Process)
    }

    /// Start the admitted run or preview child.
    pub fn start(&mut self) -> Result<(), Error> {
        self.slot.start().map_err(Error::Process)
    }

    /// Execute the admitted one-shot snapshot child.
    pub fn snapshot(&mut self) -> Result<Box<[u8]>, Error> {
        self.slot.snapshot().map_err(Error::Process)
    }

    /// Forward a bounded rendered failure to the live last-good window.
    pub fn report(&mut self, report: &str) -> Result<(), Error> {
        self.slot.report(report).map_err(Error::Process)
    }

    /// Return whether the live window closed and its child was reaped.
    pub fn reap_if_exited(&mut self) -> Result<bool, Error> {
        self.slot.reap_if_exited().map_err(Error::Process)
    }

    /// Whether the program ended on its own, as its runner was asked
    /// something or between asks: [`Self::reap_if_exited`] says how.
    pub fn exited(&self) -> bool {
        self.slot.exited()
    }

    /// Explicitly stop and reap the current whole-process epoch.
    pub fn shutdown(&mut self) -> Result<(), Error> {
        if let Some(current) = self.slot.take() {
            current.shutdown()?;
        }
        Ok(())
    }
}

/// Optional native-lane setup, build, admission, or retirement failure.
#[derive(Debug)]
pub enum Error {
    Import(rawr_import::Error),
    Cache(rawr_import::cache::entry::Error),
    Build(rawr_import::runner::BuildError),
    Deliver(rawr_import::universe::Error),
    Process(rawr_import::process::RunnerError),
    Bootstrap(rawr_import::supervisor::bootstrap::Error),
    Supervisor(rawr_import::supervisor::Error),
    Node(rawr_import::node::Error),
    PinCleanup {
        failure: rawr_import::node::Error,
        cleanup: rawr_import::process::RunnerError,
    },
    InstallCleanup {
        failure: Box<Error>,
        cleanup: rawr_import::process::RunnerError,
    },
    State(&'static str),
    Obsolete,
}

impl Error {
    /// A save after the program's files were read: a newer preparation
    /// supersedes this one, so it is no failure to report.
    pub(crate) const fn source_changed(&self) -> bool {
        matches!(
            self,
            Self::Node(rawr_import::node::Error::SourceChanged(_))
                | Self::Process(rawr_import::process::RunnerError::Node(
                    rawr_import::node::Error::SourceChanged(_)
                ))
                | Self::PinCleanup {
                    failure: rawr_import::node::Error::SourceChanged(_),
                    ..
                }
        )
    }

    /// An input of the runner's build changed since its read set was
    /// sealed, as its start's one walk of it found (#1760).
    pub(crate) const fn read_set_stale(&self) -> bool {
        matches!(
            self,
            Self::Node(rawr_import::node::Error::StaleReadSet { .. })
                | Self::Process(rawr_import::process::RunnerError::Node(
                    rawr_import::node::Error::StaleReadSet { .. }
                ))
        )
    }
}

impl crate::Stale for Error {
    fn is_stale(&self) -> bool {
        match self {
            Self::Import(error) => error.is_stale(),
            Self::Build(error) => error.is_stale(),
            Self::Node(error)
            | Self::Process(rawr_import::process::RunnerError::Node(error))
            | Self::PinCleanup { failure: error, .. } => error.is_stale(),
            _ => false,
        }
    }
}

impl From<rawr_import::Error> for Error {
    fn from(error: rawr_import::Error) -> Self {
        Self::Import(error)
    }
}

impl From<rawr_import::cache::entry::Error> for Error {
    fn from(error: rawr_import::cache::entry::Error) -> Self {
        Self::Cache(error)
    }
}

impl From<rawr_import::runner::BuildError> for Error {
    fn from(error: rawr_import::runner::BuildError) -> Self {
        Self::Build(error)
    }
}

impl From<rawr_import::process::RunnerError> for Error {
    fn from(error: rawr_import::process::RunnerError) -> Self {
        Self::Process(error)
    }
}

impl From<rawr_import::supervisor::bootstrap::Error> for Error {
    fn from(error: rawr_import::supervisor::bootstrap::Error) -> Self {
        Self::Bootstrap(error)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Import(error) => error.fmt(formatter),
            Self::Cache(error) => error.fmt(formatter),
            Self::Build(error) => error.fmt(formatter),
            Self::Deliver(error) => error.fmt(formatter),
            Self::Process(error) => error.fmt(formatter),
            Self::Bootstrap(error) => error.fmt(formatter),
            Self::Supervisor(error) => error.fmt(formatter),
            Self::Node(error) => error.fmt(formatter),
            Self::PinCleanup { failure, cleanup } => write!(
                formatter,
                "cannot keep the new runner: {failure}; stopping it also failed: {cleanup}"
            ),
            Self::InstallCleanup { failure, cleanup } => write!(
                formatter,
                "cannot start the new runner: {failure}; stopping it also failed: {cleanup}"
            ),
            Self::State(reason) => write!(formatter, "the runner went missing {reason}"),
            Self::Obsolete => formatter.write_str("a newer save replaced this runner build"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Import(error) => Some(error),
            Self::Cache(error) => Some(error),
            Self::Build(error) => Some(error),
            Self::Deliver(error) => Some(error),
            Self::Process(error) => Some(error),
            Self::Bootstrap(error) => Some(error),
            Self::Supervisor(error) => Some(error),
            Self::Node(error) => Some(error),
            Self::PinCleanup { failure, .. } => Some(failure),
            Self::InstallCleanup { failure, .. } => Some(failure.as_ref()),
            Self::State(_) | Self::Obsolete => None,
        }
    }
}
