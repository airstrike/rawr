//! Closed pin-record, owner, transaction and retirement observations.

use std::cell::Cell;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub mod join;

use super::{Account, Commitment, Fault, Observation};
use crate::metrics::publication::{Replacement, Role};
use crate::node::account::evidence::Usage;
use crate::store::Publication;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub root: [u8; 32],
    pub relative: [u8; 32],
    pub project: [u8; 32],
    pub role: Role,
    pub owner: [u8; 32],
    pub proof: [u8; 32],
    pub record: Commitment,
    pub references: u64,
}
impl Identity {
    pub fn relative(&self) -> Option<PathBuf> {
        let role = match self.role {
            Role::Live => "live",
            Role::LastGood => "last-good",
            Role::Prepared => "prepared",
            Role::InFlight => "in-flight",
        };
        let path = PathBuf::from("pins")
            .join(role)
            .join(crate::node::Digest::from_bytes(self.owner).directory());
        (super::path_digest(&path) == self.relative).then_some(path)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub class: u8,
    pub kind: u8,
    pub schema: u32,
    pub key: [u8; 32],
    pub bytes: u64,
    pub allocations: u64,
    pub members: u64,
    pub record: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    Shared,
    Exclusive,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Operation {
    Acquire,
    Retain,
    Release,
    Stale,
    Project {
        project: [u8; 32],
        purpose: Replacement,
        cause: super::ledger::Cause,
    },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Step {
    Existing,
    Renamed,
    RemovalStarted,
    Removed,
    RemovalFailed,
    Absent,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransactionStep {
    Previous { owner: u64 },
    Acquired { index: u64, owner: u64 },
    LedgerAttempted,
    Ledger { replaced: bool },
    Validated,
    Callback { succeeded: bool },
    Restore { attempt: u64 },
    RestoreApplied { attempt: u64 },
    Restored { attempt: u64, succeeded: bool },
    Committed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "pin", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Event {
    Record {
        source: u64,
        publication: u64,
        identity: Identity,
    },
    Reference {
        source: u64,
        index: u64,
        reference: Reference,
    },
    Started {
        attempt: u64,
        publication: u64,
        source: Option<u64>,
        root: [u8; 32],
        relative: [u8; 32],
        operation: Operation,
    },
    Step {
        attempt: u64,
        step: Step,
    },
    Relocated {
        attempt: u64,
        temporary: [u8; 32],
    },
    Finished {
        attempt: u64,
        succeeded: bool,
    },
    Owner {
        owner: u64,
        source: u64,
        mode: Mode,
    },
    Retired {
        owner: u64,
    },
    Transaction {
        transaction: u64,
        publication: u64,
        root: [u8; 32],
        project: [u8; 32],
        proof: [u8; 32],
        roles: u64,
        previous: u64,
        before: super::ledger::Prior,
        after: Commitment,
    },
    Role {
        transaction: u64,
        index: u64,
        role: Role,
    },
    TransactionStep {
        transaction: u64,
        step: TransactionStep,
    },
    TransactionFinished {
        transaction: u64,
        pending: bool,
    },
}

/// A source record retains only the same original usage state and its token.
#[derive(Clone, Default)]
pub(crate) struct Source {
    token: u64,
    usage: Option<Usage>,
}
impl Source {
    pub(crate) fn observe(
        publication: &Publication<'_>,
        target: &Path,
        project: [u8; 32],
        role: Role,
        owner: [u8; 32],
        proof: [u8; 32],
        bytes: &[u8],
        references: usize,
    ) -> Self {
        if !crate::metrics::principal_enabled() {
            return Self::default();
        }
        let source = Self {
            token: super::next(),
            usage: Some(Usage::retain(publication.graph())),
        };
        let event = (|| {
            if source.token == 0 {
                return Err(Fault::Exhausted);
            }
            let relative = target
                .strip_prefix(publication.root())
                .map_err(|_| Fault::Identity)?;
            for size in [
                publication.root().as_os_str().as_encoded_bytes().len(),
                relative.as_os_str().as_encoded_bytes().len(),
                bytes.len(),
            ] {
                publication.graph().hash(size).map_err(|_| Fault::Budget)?;
            }
            Ok(Event::Record {
                source: source.token,
                publication: publication.observation(),
                identity: Identity {
                    root: super::path_digest(publication.root()),
                    relative: super::path_digest(relative),
                    project,
                    role,
                    owner,
                    proof,
                    record: Commitment::produced(bytes),
                    references: references as u64,
                },
            })
        })();
        source.emit(event);
        source
    }
    pub(crate) fn reference(&self, index: usize, reference: Reference) {
        self.emit(Ok(Event::Reference {
            source: self.token,
            index: index as u64,
            reference,
        }));
    }
    pub(crate) fn owner(&self, mode: Mode) -> Owner {
        let Some(usage) = &self.usage else {
            return Owner::default();
        };
        let token = super::next();
        self.emit(if token == 0 {
            Err(Fault::Exhausted)
        } else {
            Ok(Event::Owner {
                owner: token,
                source: self.token,
                mode,
            })
        });
        Owner {
            token,
            usage: Some(usage.clone()),
        }
    }
    fn emit(&self, event: Result<Event, Fault>) {
        if let Some(usage) = &self.usage {
            record(usage, event);
        }
    }
}

/// This field follows the native File field, so retirement follows File drop.
/// It reports owner destruction, never an inferred successful native close.
#[derive(Default)]
pub(crate) struct Owner {
    token: u64,
    usage: Option<Usage>,
}
impl Owner {
    pub(crate) fn token(&self) -> u64 {
        self.token
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        if let Some(usage) = &self.usage {
            record(usage, Ok(Event::Retired { owner: self.token }));
        }
    }
}

pub(crate) struct Attempt {
    token: u64,
    usage: Option<Usage>,
    succeeded: Cell<bool>,
}
impl Attempt {
    pub(crate) fn temporary(&self) -> Option<super::temporary::Writer<'_>> {
        self.usage
            .as_ref()
            .map(|usage| super::temporary::Writer::pin(self.token, usage))
    }

    pub(crate) fn start(
        publication: &Publication<'_>,
        target: &Path,
        source: Option<&Source>,
        operation: Operation,
    ) -> Self {
        let mut value = Self {
            token: 0,
            usage: None,
            succeeded: Cell::new(false),
        };
        if !crate::metrics::principal_enabled() {
            return value;
        }
        value.token = super::next();
        value.usage = Some(Usage::retain(publication.graph()));
        let event = (|| {
            if value.token == 0 || source.is_some_and(|source| source.token == 0) {
                return Err(Fault::Exhausted);
            }
            let relative = target
                .strip_prefix(publication.root())
                .map_err(|_| Fault::Identity)?;
            for size in [
                publication.root().as_os_str().as_encoded_bytes().len(),
                relative.as_os_str().as_encoded_bytes().len(),
            ] {
                publication.graph().hash(size).map_err(|_| Fault::Budget)?;
            }
            Ok(Event::Started {
                attempt: value.token,
                publication: publication.observation(),
                source: source.map(|source| source.token),
                root: super::path_digest(publication.root()),
                relative: super::path_digest(relative),
                operation,
            })
        })();
        value.emit(event);
        value
    }
    pub(crate) fn step(&self, step: Step) {
        self.emit(Ok(Event::Step {
            attempt: self.token,
            step,
        }));
    }
    pub(crate) fn relocated(&self, publication: &Publication<'_>, temporary: &Path) {
        if self.usage.is_none() || !crate::metrics::principal_enabled() {
            return;
        }
        let event = (|| {
            let relative = temporary
                .strip_prefix(publication.root())
                .map_err(|_| Fault::Identity)?;
            publication
                .graph()
                .hash(relative.as_os_str().as_encoded_bytes().len())
                .map_err(|_| Fault::Budget)?;
            Ok(Event::Relocated {
                attempt: self.token,
                temporary: super::path_digest(relative),
            })
        })();
        self.emit(event);
    }
    pub(crate) fn completed(&self, succeeded: bool) {
        self.succeeded.set(succeeded);
    }
    fn emit(&self, event: Result<Event, Fault>) {
        if let Some(usage) = &self.usage {
            record(usage, event);
        }
    }
}
impl Drop for Attempt {
    fn drop(&mut self) {
        self.emit(Ok(Event::Finished {
            attempt: self.token,
            succeeded: self.succeeded.get(),
        }));
    }
}

pub(crate) struct Transaction {
    token: u64,
    usage: Option<Usage>,
}
impl Transaction {
    pub(crate) fn start(
        publication: &Publication<'_>,
        project: [u8; 32],
        proof: [u8; 32],
        roles: usize,
        previous: usize,
        before: Option<&[u8]>,
        after: &[u8],
    ) -> Self {
        let mut value = Self {
            token: 0,
            usage: None,
        };
        if !crate::metrics::principal_enabled() {
            return value;
        }
        value.token = super::next();
        value.usage = Some(Usage::retain(publication.graph()));
        let event = (|| {
            if value.token == 0 {
                return Err(Fault::Exhausted);
            }
            for size in [
                publication.root().as_os_str().as_encoded_bytes().len(),
                before.map_or(0, <[u8]>::len),
                after.len(),
            ] {
                publication.graph().hash(size).map_err(|_| Fault::Budget)?;
            }
            Ok(Event::Transaction {
                transaction: value.token,
                publication: publication.observation(),
                root: super::path_digest(publication.root()),
                project,
                proof,
                roles: roles as u64,
                previous: previous as u64,
                before: before.map_or(super::ledger::Prior::Absent, |bytes| {
                    super::ledger::Prior::Bytes(Commitment::produced(bytes))
                }),
                after: Commitment::produced(after),
            })
        })();
        value.emit(event);
        value
    }
    pub(crate) fn role(&self, index: usize, role: Role) {
        self.emit(Ok(Event::Role {
            transaction: self.token,
            index: index as u64,
            role,
        }));
    }
    pub(crate) fn step(&self, step: TransactionStep) {
        self.emit(Ok(Event::TransactionStep {
            transaction: self.token,
            step,
        }));
    }
    pub(crate) fn finish(&self, pending: bool) {
        self.emit(Ok(Event::TransactionFinished {
            transaction: self.token,
            pending,
        }));
    }
    fn emit(&self, event: Result<Event, Fault>) {
        if let Some(usage) = &self.usage {
            record(usage, event);
        }
    }
}

/// Covers an actual reached owned-array allocation or resize request.
pub(crate) fn storage(publication: &Publication<'_>, bytes: usize) {
    if bytes != 0
        && crate::metrics::principal_enabled()
        && publication.graph().decode(bytes).is_err()
    {
        crate::metrics::record_principal(&Observation {
            event: Err(Fault::Budget),
            budget: Account::Graph(publication.graph()),
        });
    }
}

fn record(usage: &Usage, event: Result<Event, Fault>) {
    if !crate::metrics::principal_enabled() {
        return;
    }
    crate::metrics::record_principal(&Observation {
        event: event.map(|change| super::Event::Pin { change }),
        budget: Account::Retained(usage),
    });
}
