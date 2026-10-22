//! Publication-scoped pin replacement with exact failure restoration.

#[cfg(feature = "import-baseline")]
use crate::metrics::principal::pin;
use std::fs;
use std::path::Path;

use crate::node::{Digest, Error, ReadSetProof};

use super::{lock, Ledger, Role};
use crate::store::lease::Publication;

/// An admission failure and any independent failure to restore its old pins.
#[derive(Debug, thiserror::Error)]
pub(crate) enum Rejected<E> {
    #[error("pin-bound admission failed: {0}")]
    Operation(#[source] E),
    #[error("pin-bound admission failed: {failure}; pin rollback also failed: {cleanup}")]
    Cleanup {
        #[source]
        failure: E,
        cleanup: Error,
    },
}

/// Replacement roots whose durable binding can still restore its exact predecessor.
#[must_use = "a pin transaction must finish its post-bind validation and admission"]
pub(in crate::store) struct Transaction<'publication, 'store, const N: usize> {
    guards: [lock::Guard; N],
    pending: Pending<'publication, 'store>,
}

struct Pending<'publication, 'store> {
    publication: &'publication Publication<'store>,
    root: &'publication Path,
    project: Digest,
    change: Option<Change>,
    #[cfg(feature = "import-baseline")]
    observation: pin::Transaction,
}

struct Change {
    before: Option<Box<[u8]>>,
    records: Vec<lock::Acquisition>,
    acquired: Vec<lock::Guard>,
    _previous: Vec<lock::Guard>,
    ledger_attempted: bool,
}

impl<'publication, 'store> Transaction<'publication, 'store, 1> {
    pub(in crate::store) fn bind_retained(
        publication: &'publication Publication<'store>,
        store: &'publication crate::store::Store,
        retention: &crate::node::read_set::Retention,
    ) -> Result<Self, Error> {
        if publication.root() != store.root() {
            return Err(Error::State(
                "retention publication belongs to another store",
            ));
        }
        retention.validate_storage(store)?;
        Self::bind_parts(
            publication,
            store.root(),
            store.project,
            [Role::InFlight],
            retention.entries(),
            retention.digest(),
        )
    }
}

impl<'publication, 'store, const N: usize> Transaction<'publication, 'store, N> {
    pub(in crate::store) fn bind(
        publication: &'publication Publication<'store>,
        root: &'publication Path,
        project: Digest,
        roles: [Role; N],
        proof: &ReadSetProof,
    ) -> Result<Self, Error> {
        Self::bind_parts(
            publication,
            root,
            project,
            roles,
            proof.entries(),
            proof.digest(),
        )
    }

    fn bind_parts(
        publication: &'publication Publication<'store>,
        root: &'publication Path,
        project: Digest,
        roles: [Role; N],
        entries: &[crate::node::read_set::Entry],
        digest: Digest,
    ) -> Result<Self, Error> {
        #[cfg(feature = "import-baseline")]
        crate::metrics::record_scalar(
            crate::metrics::accounting::Scalar::PinRolesRequested,
            N as u64,
        );
        let path = super::ledger_path(root, project);
        let before = match fs::symlink_metadata(&path) {
            Ok(_) => {
                let bytes = crate::store::entry::read_file(&path, super::MAX_LEDGER_BYTES)?;
                #[cfg(feature = "import-baseline")]
                super::observe_read(bytes.len());
                Some(bytes)
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => None,
            Err(source) => {
                return Err(Error::Io {
                    operation: "inspect previous project pin ledger",
                    path,
                    source,
                })
            }
        };
        let previous = match &before {
            Some(bytes) => Ledger::decode(bytes).map_err(|_| Error::Corrupt {
                path: path.clone(),
                reason: "previous project pin ledger decoding failed",
            })?,
            None => Ledger::empty(project),
        };
        if previous.project != project {
            return Err(Error::Corrupt {
                path,
                reason: "previous project pin ledger identity changed",
            });
        }
        for pinned in &previous.roots {
            crate::store::accounting::validate_references(root, &pinned.references)?;
        }
        let mut ledger = previous.clone();
        let references = crate::store::accounting::references(root, entries)?;
        let mut records = Vec::new();
        records
            .try_reserve_exact(N)
            .map_err(|_| Error::Allocation)?;
        for role in roles {
            let pinned = ledger.bind(role, digest, references.clone())?;
            records.push(lock::Acquisition::inspect(root, project, pinned)?);
        }
        let after = ledger.encode()?;
        #[cfg(feature = "import-baseline")]
        lock::admit_guard_observations(publication, previous.roots.len());
        let mut retained = Vec::new();
        retained
            .try_reserve_exact(previous.roots.len())
            .map_err(|_| Error::Allocation)?;
        for pinned in previous.roots {
            retained.push(
                lock::Acquisition::inspect(root, project, pinned)?.retain(publication, root)?,
            );
        }
        #[cfg(feature = "import-baseline")]
        lock::admit_guard_observations(publication, N);
        let mut acquired = Vec::new();
        acquired
            .try_reserve_exact(N)
            .map_err(|_| Error::Allocation)?;
        let mut pending = Pending {
            publication,
            root,
            project,
            #[cfg(feature = "import-baseline")]
            observation: pin::Transaction::start(
                publication,
                project.as_bytes(),
                digest.as_bytes(),
                N,
                retained.len(),
                before.as_deref(),
                &after,
            ),
            change: Some(Change {
                before,
                records,
                acquired,
                _previous: retained,
                ledger_attempted: false,
            }),
        };
        #[cfg(feature = "import-baseline")]
        {
            for (index, role) in roles.into_iter().enumerate() {
                pending.observation.role(index, role.observed());
            }
            if let Some(change) = &pending.change {
                for guard in &change._previous {
                    pending.observation.step(pin::TransactionStep::Previous {
                        owner: guard.observed(),
                    });
                }
            }
        }
        let stage = (|| {
            let change = pending
                .change
                .as_mut()
                .ok_or(Error::State("pin transaction lost its captured state"))?;
            for record in &change.records {
                change.acquired.push(record.acquire(publication, root)?);
                #[cfg(feature = "import-baseline")]
                if let Some(guard) = change.acquired.last() {
                    pending.observation.step(pin::TransactionStep::Acquired {
                        index: (change.acquired.len() - 1) as u64,
                        owner: guard.observed(),
                    });
                }
                checkpoint(Stage::Acquired(change.acquired.len()))?;
            }
            if change.before.as_deref() != Some(after.as_ref()) {
                change.ledger_attempted = true;
                #[cfg(feature = "import-baseline")]
                pending
                    .observation
                    .step(pin::TransactionStep::LedgerAttempted);
                super::save_bytes(
                    publication,
                    root,
                    project,
                    &after,
                    #[cfg(feature = "import-baseline")]
                    crate::metrics::publication::Replacement::PinBind,
                    #[cfg(feature = "import-baseline")]
                    crate::metrics::principal::ledger::Cause::PinBind,
                    #[cfg(feature = "import-baseline")]
                    change.before.as_deref().map_or(
                        crate::metrics::principal::ledger::Before::Absent,
                        crate::metrics::principal::ledger::Before::Bytes,
                    ),
                )?;
            }
            #[cfg(feature = "import-baseline")]
            pending.observation.step(pin::TransactionStep::Ledger {
                replaced: change.ledger_attempted,
            });
            checkpoint(Stage::Ledger)?;
            Ok(())
        })();
        if let Err(failure) = stage {
            return Err(pending.reject(failure));
        }
        let guards = std::mem::take(
            &mut pending
                .change
                .as_mut()
                .ok_or(Error::State("pin transaction lost its acquired guards"))?
                .acquired,
        );
        let guards: [lock::Guard; N] = match guards.try_into() {
            Ok(guards) => guards,
            Err(guards) => {
                drop(guards);
                return Err(
                    pending.reject(Error::State("pin transaction guard cardinality changed"))
                );
            }
        };
        Ok(Self { guards, pending })
    }

    pub(in crate::store) fn finish<T, E>(
        mut self,
        validate: impl FnOnce() -> Result<(), Error>,
        operation: impl FnOnce() -> Result<T, E>,
    ) -> Result<Result<(T, [lock::Guard; N]), Rejected<E>>, Error> {
        if let Err(failure) = checkpoint(Stage::PostBind).and_then(|()| validate()) {
            let Self {
                guards,
                mut pending,
            } = self;
            drop(guards);
            return Err(pending.reject(failure));
        }
        #[cfg(feature = "import-baseline")]
        self.pending
            .observation
            .step(pin::TransactionStep::Validated);
        match operation() {
            Ok(value) => {
                #[cfg(feature = "import-baseline")]
                self.pending
                    .observation
                    .step(pin::TransactionStep::Callback { succeeded: true });
                #[cfg(feature = "import-baseline")]
                crate::metrics::record_scalar(
                    crate::metrics::accounting::Scalar::PinRolesCommitted,
                    N as u64,
                );
                self.pending.change = None;
                #[cfg(feature = "import-baseline")]
                self.pending
                    .observation
                    .step(pin::TransactionStep::Committed);
                Ok(Ok((value, self.guards)))
            }
            Err(failure) => {
                #[cfg(feature = "import-baseline")]
                self.pending
                    .observation
                    .step(pin::TransactionStep::Callback { succeeded: false });
                let Self {
                    guards,
                    mut pending,
                } = self;
                drop(guards);
                Ok(Err(match pending.restore() {
                    Ok(()) => Rejected::Operation(failure),
                    Err(cleanup) => Rejected::Cleanup { failure, cleanup },
                }))
            }
        }
    }
}

impl Pending<'_, '_> {
    fn reject(&mut self, failure: Error) -> Error {
        match self.restore() {
            Ok(()) => failure,
            Err(cleanup) => Error::Cleanup {
                failure: Box::new(failure),
                cleanup: Box::new(cleanup),
            },
        }
    }

    fn restore(&mut self) -> Result<(), Error> {
        let Some(change) = self.change.as_mut() else {
            return Ok(());
        };
        #[cfg(feature = "import-baseline")]
        let attempt = crate::metrics::principal::next();
        #[cfg(feature = "import-baseline")]
        self.observation
            .step(pin::TransactionStep::Restore { attempt });
        let result = (|| {
            #[cfg(feature = "import-baseline")]
            crate::metrics::record_scalar(
                crate::metrics::accounting::Scalar::PinRolesRollbackRequested,
                change.records.len() as u64,
            );
            change.acquired.clear();
            checkpoint(Stage::Rollback)?;
            if change.ledger_attempted {
                match &change.before {
                    Some(bytes) => super::save_bytes(
                        self.publication,
                        self.root,
                        self.project,
                        bytes,
                        #[cfg(feature = "import-baseline")]
                        crate::metrics::publication::Replacement::PinRestore,
                        #[cfg(feature = "import-baseline")]
                        crate::metrics::principal::ledger::Cause::PinRestore,
                        #[cfg(feature = "import-baseline")]
                        crate::metrics::principal::ledger::Before::Unknown,
                    )?,
                    None => super::save_or_retire(
                        self.publication,
                        self.root,
                        &Ledger::empty(self.project),
                        #[cfg(feature = "import-baseline")]
                        crate::metrics::publication::Replacement::PinRestore,
                        #[cfg(feature = "import-baseline")]
                        crate::metrics::principal::ledger::Cause::PinRestore,
                    )?,
                }
            }
            let mut failure = None;
            for record in change.records.iter().rev() {
                if let Err(cleanup) = record.rollback(self.publication, self.root) {
                    failure = Some(match failure {
                        Some(failure) => Error::Cleanup {
                            failure: Box::new(failure),
                            cleanup: Box::new(cleanup),
                        },
                        None => cleanup,
                    });
                }
            }
            if let Some(failure) = failure {
                return Err(failure);
            }
            Ok(())
        })();
        if result.is_ok() {
            #[cfg(feature = "import-baseline")]
            self.observation
                .step(pin::TransactionStep::RestoreApplied { attempt });
            self.change = None;
        }
        #[cfg(feature = "import-baseline")]
        self.observation.step(pin::TransactionStep::Restored {
            attempt,
            succeeded: result.is_ok(),
        });
        result
    }
}

impl Drop for Pending<'_, '_> {
    fn drop(&mut self) {
        // Normal failures explicitly preserve cleanup errors; unwinding still
        // attempts restoration while the publication lease remains held.
        drop(self.restore());
        #[cfg(feature = "import-baseline")]
        self.observation.finish(self.change.is_some());
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Stage {
    Published(Role),
    Acquired(usize),
    Ledger,
    PostBind,
    Rollback,
}

pub(super) fn checkpoint(stage: Stage) -> Result<(), Error> {
    #[cfg(test)]
    if FAILURE.with(|failure| failure.get() == Some(stage)) {
        FAILURE.with(|failure| failure.set(None));
        return Err(Error::State("injected pin transaction failure"));
    }
    #[cfg(not(test))]
    let _ = stage;
    Ok(())
}

#[cfg(test)]
thread_local! {
    static FAILURE: std::cell::Cell<Option<Stage>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn with_post_bind_failure<T>(operation: impl FnOnce() -> T) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            FAILURE.with(|failure| failure.set(None));
        }
    }
    FAILURE.with(|failure| assert!(failure.replace(Some(Stage::PostBind)).is_none()));
    let _reset = Reset;
    operation()
}

#[cfg(test)]
mod tests;
