//! Side-effect-free staged transform from built manifest to admitted process.

impl super::Owner {
    /// Allocate one epoch, authenticate its artifact, prepare the exact
    /// snapshot, consume activation authority, and return only after admission.
    pub fn bootstrap<B>(
        &mut self,
        mut manifest: crate::runner::Manifest<crate::runner::Built, B>,
        snapshot: super::snapshot::Bytes,
        process: &crate::process::Owner,
    ) -> Result<crate::process::Candidate<crate::process::Admitted, B>, Error> {
        if !snapshot.matches(manifest.request.snapshot) {
            return Err(Error::Snapshot(super::snapshot::Error::Mismatch));
        }
        manifest
            .bind_startup_snapshot(&snapshot)
            .map_err(Error::Node)?;
        let read_set = manifest.take_prepared_read_set();
        if let Some(read_set) = read_set {
            let mut admitted = read_set
                .with_startup_pin(|| {
                    self.bootstrap_current(manifest, snapshot, process, Some(&read_set))
                })
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
    #[error(transparent)]
    Supervisor(#[from] super::Error),
    #[error(transparent)]
    Process(#[from] crate::process::RunnerError),
    #[error(transparent)]
    Snapshot(#[from] super::snapshot::Error),
}
