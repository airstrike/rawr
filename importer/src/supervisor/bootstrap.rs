//! Side-effect-free staged transform from built manifest to admitted process.

impl super::Owner {
    /// Allocate one epoch, authenticate its artifact, prepare the exact
    /// snapshot, consume activation authority, and return only after admission.
    /// The runner's read set is walked once, just before admission.
    pub fn bootstrap<B>(
        &mut self,
        manifest: crate::runner::Manifest<crate::runner::Built, B>,
        snapshot: super::snapshot::Bytes,
        process: &crate::process::Owner,
    ) -> Result<crate::process::Candidate<crate::process::Admitted, B>, Error> {
        self.bootstrap_checked(manifest, snapshot, process, Check::BeforeAdmission)
    }

    /// [`Self::bootstrap`], whose admitted runner then observes the inputs
    /// `observation` names before its read set is walked, once (#1760): a
    /// change after that walk is the runner's to see, and one before it
    /// is the walk's. The walk before admission proves nothing more, as
    /// nothing runs the program until this returns.
    ///
    /// With [`Current::Watched`], nothing walks it (#1755).
    pub fn bootstrap_observed<B>(
        &mut self,
        manifest: crate::runner::Manifest<crate::runner::Built, B>,
        snapshot: super::snapshot::Bytes,
        process: &crate::process::Owner,
        observation: &[u8],
        current: Current,
    ) -> Result<crate::process::Candidate<crate::process::Admitted, B>, Error> {
        let mut admitted =
            self.bootstrap_checked(manifest, snapshot, process, Check::AfterObservation)?;
        if let Err(failure) = admitted.observe_initial(observation, current) {
            return match admitted.shutdown() {
                Ok(()) => Err(Error::Process(failure)),
                Err(cleanup) => Err(Error::ObserveCleanup {
                    failure: Box::new(failure),
                    cleanup: Box::new(cleanup),
                }),
            };
        }
        Ok(admitted)
    }

    fn bootstrap_checked<B>(
        &mut self,
        mut manifest: crate::runner::Manifest<crate::runner::Built, B>,
        snapshot: super::snapshot::Bytes,
        process: &crate::process::Owner,
        check: Check,
    ) -> Result<crate::process::Candidate<crate::process::Admitted, B>, Error> {
        if !snapshot.matches(manifest.request.snapshot) {
            return Err(Error::Snapshot(super::snapshot::Error::Mismatch));
        }
        manifest
            .bind_startup_snapshot(&snapshot)
            .map_err(Error::Node)?;
        let read_set = manifest.take_prepared_read_set();
        if let Some(read_set) = read_set {
            let checked = match check {
                Check::BeforeAdmission => Some(&read_set),
                Check::AfterObservation => None,
            };
            let mut admitted = read_set
                .with_startup_pin(|| self.bootstrap_current(manifest, snapshot, process, checked))
                .map_err(Error::Node)??;
            if let Err(failure) = admitted.restore_prepared_read_set(read_set) {
                return match admitted.shutdown() {
                    Ok(()) => Err(Error::Node(failure)),
                    Err(cleanup) => Err(Error::ReadSetCleanup { failure, cleanup }),
                };
            }
            return Ok(admitted);
        }
        self.bootstrap_current(manifest, snapshot, process, None)
    }

    fn bootstrap_current<B>(
        &mut self,
        manifest: crate::runner::Manifest<crate::runner::Built, B>,
        snapshot: super::snapshot::Bytes,
        process: &crate::process::Owner,
        read_set: Option<&crate::node::read_set::Prepared>,
    ) -> Result<crate::process::Candidate<crate::process::Admitted, B>, Error> {
        let epoch = self.epoch()?;
        let spawned = manifest.spawn(epoch, process)?;
        let activation = self.activation(spawned.key())?;
        let prepared = spawned.prepare(snapshot)?;
        let activated = prepared.activate(activation)?;
        admit_current(read_set, || activated.admit())
    }
}

pub(crate) fn admit_current<T>(
    read_set: Option<&crate::node::read_set::Prepared>,
    admit: impl FnOnce() -> Result<T, crate::process::RunnerError>,
) -> Result<T, Error> {
    if let Some(read_set) = read_set {
        read_set.revalidate_complete().map_err(Error::Node)?;
    }
    admit().map_err(Error::Process)
}

/// What proves a started runner's read set current once the runner
/// observes its inputs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Current {
    /// A walk of it: the start's one (#1760).
    Walked,
    /// The caller's watcher, which observed every input of it since an
    /// earlier process of the same runner walked it, and saw none change:
    /// a new process of that runner walks it no more (#1755).
    Watched,
}

/// Where a bootstrap walks its runner's read set, once.
#[derive(Clone, Copy)]
enum Check {
    /// Just before admission.
    BeforeAdmission,
    /// Once the admitted runner observes its inputs.
    AfterObservation,
}

/// Failure before a whole-process candidate becomes admissible.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("prepared runner read set rejected before admission")]
    Node(#[source] crate::node::Error),
    #[error("admitted runner lost its read-set owner and candidate cleanup also failed")]
    ReadSetCleanup {
        #[source]
        failure: crate::node::Error,
        cleanup: crate::process::RunnerError,
    },
    #[error("the admitted runner failed to observe its inputs: {failure}; stopping it also failed: {cleanup}")]
    ObserveCleanup {
        #[source]
        failure: Box<crate::process::RunnerError>,
        cleanup: Box<crate::process::RunnerError>,
    },
    #[error(transparent)]
    Supervisor(#[from] super::Error),
    #[error(transparent)]
    Process(#[from] crate::process::RunnerError),
    #[error(transparent)]
    Snapshot(#[from] super::snapshot::Error),
}
