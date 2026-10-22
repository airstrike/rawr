//! Process-local ledger replacements. These are observations, not write authority.

pub mod join;
#[cfg(test)]
mod tests;

use std::cell::Cell;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{Commitment, Fault, Lease, Observation};
use crate::metrics::publication::Replacement;
use crate::node::GraphBudget;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Target {
    Installation,
    Project([u8; 32]),
}

impl Target {
    pub fn relative(self) -> PathBuf {
        match self {
            Self::Installation => PathBuf::from("ledgers/installation"),
            Self::Project(project) => PathBuf::from("ledgers/projects")
                .join(crate::node::Digest::from_bytes(project).directory()),
        }
    }

    fn matches(self, path: &Path) -> bool {
        let bytes = path.as_os_str().as_encoded_bytes();
        match self {
            Self::Installation => bytes == b"ledgers/installation",
            Self::Project(project) => {
                const PREFIX: &[u8] = b"ledgers/projects/";
                let Some(name) = bytes.strip_prefix(PREFIX) else {
                    return false;
                };
                let mut hex = [0; 64];
                for (pair, value) in hex.chunks_exact_mut(2).zip(project) {
                    pair[0] = b"0123456789abcdef"[(value >> 4) as usize];
                    pair[1] = b"0123456789abcdef"[(value & 15) as usize];
                }
                name == hex
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Cause {
    Bootstrap,
    Migration,
    Reservation(Lease),
    Cancel(Lease),
    Commit(Lease),
    Planned(Lease),
    Recovery(Option<Lease>),
    Eviction,
    Repair,
    PinBind,
    PinRestore,
    InFlightRelease,
    RunnerRetirement,
}

/// Only a source buffer already held by the caller can establish a predecessor.
/// Unknown is distinct from an observed absent file.
#[derive(Clone, Copy)]
pub(crate) enum Before<'a> {
    Unknown,
    Absent,
    Bytes(&'a [u8]),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Prior {
    Unknown,
    Absent,
    Bytes(Commitment),
}

pub(crate) struct Write<'a> {
    pub(crate) target: Target,
    pub(crate) cause: Cause,
    pub(crate) before: Before<'a>,
}

impl<'a> Write<'a> {
    pub(crate) fn installation(cause: Cause) -> Self {
        Self {
            target: Target::Installation,
            cause,
            before: Before::Unknown,
        }
    }

    pub(crate) fn project(project: crate::node::Digest, cause: Cause) -> Self {
        Self {
            target: Target::Project(project.as_bytes()),
            cause,
            before: Before::Unknown,
        }
    }

    pub(crate) fn with_before(mut self, before: Before<'a>) -> Self {
        self.before = before;
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "change", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Event {
    Started {
        attempt: u64,
        publication: u64,
        root: [u8; 32],
        relative: [u8; 32],
        target: Target,
        schema: u16,
        purpose: Replacement,
        cause: Cause,
        before: Prior,
        after: Commitment,
    },
    Renamed {
        attempt: u64,
    },
    Cleanup {
        attempt: u64,
        succeeded: bool,
    },
    Finished {
        attempt: u64,
        succeeded: bool,
    },
}

pub(crate) struct Attempt<'a> {
    token: u64,
    budget: &'a GraphBudget,
    succeeded: Cell<bool>,
}

impl<'a> Attempt<'a> {
    pub(crate) fn temporary(&self) -> super::temporary::Writer<'_> {
        super::temporary::Writer::ledger(self.token, self.budget)
    }

    pub(crate) fn replace(
        publication: &crate::store::Publication<'a>,
        root: &Path,
        target: &Path,
        bytes: &[u8],
        purpose: Replacement,
        write: Write<'_>,
    ) -> Self {
        let budget = publication.graph();
        let mut result = Self {
            token: 0,
            budget,
            succeeded: Cell::new(false),
        };
        if !crate::metrics::principal_enabled() {
            return result;
        }
        result.token = super::next();
        let event = (|| {
            if result.token == 0 || publication.observation() == 0 {
                return Err(Fault::Exhausted);
            }
            if publication.root() != root {
                return Err(Fault::Identity);
            }
            let relative = target.strip_prefix(root).map_err(|_| Fault::Identity)?;
            if !write.target.matches(relative) {
                return Err(Fault::Identity);
            }
            let schema = bytes.get(8..10).ok_or(Fault::Identity)?;
            if !compatible(
                write.target,
                u16::from_le_bytes([schema[0], schema[1]]),
                purpose,
                write.cause,
            ) {
                return Err(Fault::Identity);
            }
            for length in [
                root.as_os_str().as_encoded_bytes().len(),
                relative.as_os_str().as_encoded_bytes().len(),
                bytes.len(),
            ] {
                budget.hash(length).map_err(|_| Fault::Budget)?;
            }
            let before = match write.before {
                Before::Unknown => Prior::Unknown,
                Before::Absent => Prior::Absent,
                Before::Bytes(bytes) => {
                    budget.hash(bytes.len()).map_err(|_| Fault::Budget)?;
                    Prior::Bytes(Commitment::produced(bytes))
                }
            };
            Ok(Event::Started {
                attempt: result.token,
                publication: publication.observation(),
                root: super::path_digest(root),
                relative: super::path_digest(relative),
                target: write.target,
                schema: u16::from_le_bytes([schema[0], schema[1]]),
                purpose,
                cause: write.cause,
                before,
                after: Commitment::produced(bytes),
            })
        })();
        record(budget, event);
        result
    }

    pub(crate) fn renamed(&self) {
        self.emit(Event::Renamed {
            attempt: self.token,
        });
    }

    pub(crate) fn cleanup(&self, succeeded: bool) {
        self.emit(Event::Cleanup {
            attempt: self.token,
            succeeded,
        });
    }

    pub(crate) fn completed(&self, succeeded: bool) {
        self.succeeded.set(succeeded);
    }

    fn emit(&self, event: Event) {
        if self.token != 0 {
            record(self.budget, Ok(event));
        }
    }
}

impl Drop for Attempt<'_> {
    fn drop(&mut self) {
        self.emit(Event::Finished {
            attempt: self.token,
            succeeded: self.succeeded.get(),
        });
    }
}

fn record(budget: &GraphBudget, event: Result<Event, Fault>) {
    crate::metrics::record_principal(&Observation {
        event: event.map(|change| super::Event::Ledger { change }),
        budget: super::Account::Graph(budget),
    });
}

pub(super) fn compatible(target: Target, schema: u16, purpose: Replacement, cause: Cause) -> bool {
    match target {
        Target::Installation if schema == 5 => matches!(
            (purpose, cause),
            (Replacement::InstallationBootstrap, Cause::Bootstrap)
                | (Replacement::InstallationMigration, Cause::Migration)
                | (Replacement::InstallationReservation, Cause::Reservation(_))
                | (Replacement::InstallationRepair, Cause::Repair)
                | (Replacement::InstallationPlanned, Cause::Planned(_))
                | (
                    Replacement::InstallationSave,
                    Cause::Reservation(_)
                        | Cause::Cancel(_)
                        | Cause::Commit(_)
                        | Cause::Recovery(_)
                        | Cause::Eviction
                )
        ),
        Target::Project(_) if schema == 1 => matches!(
            (purpose, cause),
            (Replacement::ProjectReservation, Cause::Reservation(_))
                | (Replacement::ProjectCancel, Cause::Cancel(_))
                | (Replacement::InFlightRelease, Cause::InFlightRelease)
                | (Replacement::ProjectRecovery, Cause::Recovery(_))
                | (
                    Replacement::ProjectRunnerRetirement,
                    Cause::RunnerRetirement
                )
                | (Replacement::PinBind, Cause::PinBind)
                | (Replacement::PinRestore, Cause::PinRestore)
        ),
        _ => false,
    }
}
