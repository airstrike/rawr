//! Actual bundle-owned controls, mode requests and cleanup; never Cargo writes.

use super::{Account, Commitment, Fault, Observation};
use crate::node::account::evidence::Usage;
use crate::store::Publication;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub mod join;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Target {
    Bundle {
        execution: [u8; 32],
        bundle: [u8; 32],
    },
    Temporary(super::retirement::Temporary),
}
impl Target {
    pub fn relative(self) -> PathBuf {
        match self {
            Self::Bundle { execution, bundle } => PathBuf::from("executions")
                .join(crate::node::Digest::from_bytes(execution).directory())
                .join(crate::node::Digest::from_bytes(bundle).directory()),
            Self::Temporary(value) => value.relative(),
        }
    }
    fn parse(path: &Path) -> Option<Self> {
        if let Some(super::retirement::Target::Bundle { execution, bundle }) =
            super::retirement::Target::parse(path)
        {
            Some(Self::Bundle { execution, bundle })
        } else {
            let name = path
                .as_os_str()
                .as_encoded_bytes()
                .strip_prefix(b"temporaries/")?;
            let name = std::str::from_utf8(name).ok()?;
            let (process, sequence) = name.split_once('-')?;
            Some(Self::Temporary(super::retirement::Temporary {
                process: process.parse().ok()?,
                sequence: sequence.parse().ok()?,
            }))
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Origin {
    Allocation { kind: u8 },
    Repair,
    Eviction,
    Planned,
    Orphan,
    Dead,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stage {
    Initialized,
    Sealed,
    Published,
    Retained,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Control {
    Lease,
    Seal,
    Allocation,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Cleanup {
    Initialization,
    Drop,
    Checked,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    Directory,
    Control {
        control: Control,
        bytes: Commitment,
    },
    RenameAllocation,
    Sync,
    Freeze {
        directory: bool,
        before: u32,
        requested: u32,
    },
    Thaw {
        directory: bool,
        before: u32,
        requested: u32,
    },
    ThawTree,
    Remove {
        cleanup: Cleanup,
    },
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Progress {
    Opened,
    Written,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    Succeeded,
    Failed,
    Collision,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "bundle", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Event {
    Owner {
        owner: u64,
        root: [u8; 32],
        target: Target,
        origin: Origin,
        publication: Option<u64>,
    },
    Record {
        owner: u64,
        publication: u64,
        allocation: super::pin::Reference,
        lease: super::Lease,
        publisher: [u8; 32],
        /// Sealed geometry committed by a schema-5 bundle record.
        #[serde(default)]
        geometry: Option<[u8; 32]>,
    },
    Stage {
        owner: u64,
        stage: Stage,
    },
    End {
        owner: u64,
    },
    Started {
        owner: u64,
        operation: u64,
        publication: Option<u64>,
        target: Target,
        path: Commitment,
        action: Action,
    },
    Progress {
        operation: u64,
        progress: Progress,
    },
    Finished {
        operation: u64,
        outcome: Outcome,
    },
}

/// Inline sidecar retains the original usage only, not graph scratch or a sink.
#[derive(Default)]
pub(crate) struct Owner {
    token: u64,
    usage: Option<Usage>,
}
impl Owner {
    pub(crate) fn begin(
        graph: &crate::node::GraphBudget,
        store: &Path,
        root: &Path,
        origin: Origin,
        publication: Option<&Publication<'_>>,
    ) -> Self {
        if !crate::metrics::principal_enabled() {
            return Self::default();
        }
        let value = Self {
            token: super::next(),
            usage: Some(Usage::retain(graph)),
        };
        let event = (|| {
            let relative = root.strip_prefix(store).map_err(|_| Fault::Identity)?;
            let target = Target::parse(relative).ok_or(Fault::Identity)?;
            let root = value
                .commitment(store.as_os_str().as_encoded_bytes())?
                .sha256;
            if publication.is_some_and(|publication| publication.root() != store) {
                return Err(Fault::Identity);
            }
            let publication = value.publication(publication)?;
            if value.token == 0 {
                return Err(Fault::Exhausted);
            }
            Ok(Event::Owner {
                owner: value.token,
                root,
                target,
                origin,
                publication,
            })
        })();
        value.emit(event);
        value
    }
    pub(crate) fn context<'a>(
        &'a self,
        store: &'a Path,
        root: &'a Path,
        publication: Option<&'a Publication<'a>>,
    ) -> Context<'a> {
        Context {
            owner: self,
            store,
            root,
            publication,
        }
    }
    pub(crate) fn stage(&self, stage: Stage) {
        self.emit(Ok(Event::Stage {
            owner: self.token,
            stage,
        }));
    }
    fn publication(&self, publication: Option<&Publication<'_>>) -> Result<Option<u64>, Fault> {
        match publication {
            Some(publication)
                if self
                    .usage
                    .as_ref()
                    .is_some_and(|usage| usage.shares_with(publication.graph()))
                    && publication.observation() != 0 =>
            {
                Ok(Some(publication.observation()))
            }
            Some(_) => Err(Fault::Account),
            None => Ok(None),
        }
    }
    fn commitment(&self, bytes: &[u8]) -> Result<Commitment, Fault> {
        let usage = self.usage.as_ref().ok_or(Fault::Unscoped)?;
        let (result, observation) = usage.hash(bytes.len());
        if let Some(observation) = observation {
            crate::metrics::record_graph(observation);
        }
        result.map_err(|_| Fault::Budget)?;
        Ok(Commitment::produced(bytes))
    }
    fn emit(&self, event: Result<Event, Fault>) {
        if let Some(usage) = &self.usage {
            if crate::metrics::principal_enabled() {
                crate::metrics::record_principal(&Observation {
                    event: event.map(|change| super::Event::Bundle { change }),
                    budget: Account::Retained(usage),
                });
            }
        }
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.emit(Ok(Event::End { owner: self.token }));
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Context<'a> {
    owner: &'a Owner,
    store: &'a Path,
    root: &'a Path,
    publication: Option<&'a Publication<'a>>,
}
impl Context<'_> {
    pub(crate) fn record_encoded(
        &self,
        mut allocation: super::pin::Reference,
        lease: super::Lease,
        publisher: [u8; 32],
        geometry: Option<[u8; 32]>,
        bytes: &[u8],
    ) -> Option<Commitment> {
        if self.owner.token == 0 || !crate::metrics::principal_enabled() {
            return None;
        }
        match self.owner.commitment(bytes) {
            Ok(commitment) => {
                allocation.record = commitment.sha256;
                self.record(allocation, lease, publisher, geometry);
                Some(commitment)
            }
            Err(fault) => {
                self.owner.emit(Err(fault));
                None
            }
        }
    }

    pub(crate) fn record(
        &self,
        allocation: super::pin::Reference,
        lease: super::Lease,
        publisher: [u8; 32],
        geometry: Option<[u8; 32]>,
    ) {
        if self.owner.token == 0 || !crate::metrics::principal_enabled() {
            return;
        }
        let event = (|| {
            let publication = self
                .owner
                .publication(self.publication)?
                .ok_or(Fault::Unscoped)?;
            if self
                .publication
                .is_some_and(|publication| publication.root() != self.store)
            {
                return Err(Fault::Identity);
            }
            Ok(Event::Record {
                owner: self.owner.token,
                publication,
                allocation,
                lease,
                publisher,
                geometry,
            })
        })();
        self.owner.emit(event);
    }
    pub(crate) fn start(&self, path: &Path, action: Action) -> Operation<'_> {
        self.start_with(path, || Ok(action))
    }
    pub(crate) fn control(&self, path: &Path, control: Control, bytes: &[u8]) -> Operation<'_> {
        self.start_with(path, || {
            Ok(Action::Control {
                control,
                bytes: self.owner.commitment(bytes)?,
            })
        })
    }
    pub(crate) fn committed_control(
        &self,
        path: &Path,
        control: Control,
        bytes: Commitment,
    ) -> Operation<'_> {
        self.start(path, Action::Control { control, bytes })
    }
    fn start_with(
        &self,
        path: &Path,
        action: impl FnOnce() -> Result<Action, Fault>,
    ) -> Operation<'_> {
        let mut operation = Operation {
            owner: self.owner,
            token: 0,
            outcome: Outcome::Failed,
        };
        if self.owner.token == 0 || !crate::metrics::principal_enabled() {
            return operation;
        }
        operation.token = super::next();
        let event = (|| {
            if operation.token == 0 {
                return Err(Fault::Exhausted);
            }
            let action = action()?;
            if self
                .publication
                .is_some_and(|publication| publication.root() != self.store)
            {
                return Err(Fault::Identity);
            }
            if !path.starts_with(self.root)
                && !(action == Action::Sync && self.root.parent() == Some(path))
            {
                return Err(Fault::Identity);
            }
            let relative = path.strip_prefix(self.store).map_err(|_| Fault::Identity)?;
            let target = Target::parse(
                self.root
                    .strip_prefix(self.store)
                    .map_err(|_| Fault::Identity)?,
            )
            .ok_or(Fault::Identity)?;
            Ok(Event::Started {
                owner: self.owner.token,
                operation: operation.token,
                publication: self.owner.publication(self.publication)?,
                target,
                path: self
                    .owner
                    .commitment(relative.as_os_str().as_encoded_bytes())?,
                action,
            })
        })();
        self.owner.emit(event);
        operation
    }
}
pub(crate) struct Operation<'a> {
    owner: &'a Owner,
    token: u64,
    outcome: Outcome,
}
impl Operation<'_> {
    pub(crate) fn progress(&self, progress: Progress) {
        if self.token != 0 {
            self.owner.emit(Ok(Event::Progress {
                operation: self.token,
                progress,
            }));
        }
    }
    pub(crate) fn result(&mut self, succeeded: bool) {
        self.outcome = if succeeded {
            Outcome::Succeeded
        } else {
            Outcome::Failed
        };
    }
    pub(crate) fn collision(&mut self) {
        self.outcome = Outcome::Collision;
    }
}
impl Drop for Operation<'_> {
    fn drop(&mut self) {
        if self.token != 0 {
            self.owner.emit(Ok(Event::Finished {
                operation: self.token,
                outcome: self.outcome,
            }));
        }
    }
}
