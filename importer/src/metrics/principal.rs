//! Actual principal publications, ledger replacements, pin owners and retirements.
//!
//! These partial producer histories retain their explicit coverage boundaries.
//! It is evidence, never publication authority or a complete durable-write proof.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::node::{self, GraphBudget};

pub mod bundle;
pub mod creation;
pub mod join;
pub mod ledger;
pub mod pin;
pub mod recovery;
pub mod retirement;
pub mod temporary;

static NEXT: AtomicU64 = AtomicU64::new(1);

/// Issue the next principal token; zero once the tokens are exhausted.
pub(crate) fn next() -> u64 {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    if id == u64::MAX {
        NEXT.store(u64::MAX, Ordering::Relaxed);
        return 0;
    }
    id
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Commitment {
    pub bytes: u64,
    pub sha256: [u8; 32],
}

impl Commitment {
    pub(crate) fn produced(bytes: &[u8]) -> Self {
        Self {
            bytes: bytes.len() as u64,
            sha256: Sha256::digest(bytes).into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Lease {
    Action([u8; 32]),
    Execution([u8; 32]),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Target {
    Payload { content: [u8; 32], lease: Lease },
    Envelope { content: [u8; 32], lease: Lease },
    Index { lookup: [u8; 32] },
}

/// Typed identity plus commitments to the actual root and relative target used.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub kind: u8,
    pub schema: u32,
    pub action: [u8; 32],
    pub publisher: [u8; 32],
    pub root: [u8; 32],
    pub relative: [u8; 32],
    pub target: Target,
}

impl Identity {
    pub(crate) fn observe(
        root: &Path,
        target_path: &Path,
        kind: node::Kind,
        schema: u32,
        action: node::ActionKey,
        publisher: node::Digest,
        target: Target,
        budget: &GraphBudget,
    ) -> Option<Self> {
        if !super::principal_enabled() {
            return None;
        }
        let relative = target_path.strip_prefix(root).ok()?;
        budget
            .hash(root.as_os_str().as_encoded_bytes().len())
            .ok()?;
        budget
            .hash(relative.as_os_str().as_encoded_bytes().len())
            .ok()?;
        Some(Self {
            kind: kind as u8,
            schema,
            action: action.as_bytes(),
            publisher: publisher.as_bytes(),
            root: path_digest(root),
            relative: path_digest(relative),
            target,
        })
    }

    /// Uses the same path constructors as the producer, then checks the actual target.
    pub fn relative(&self) -> Option<PathBuf> {
        let kind = node::Kind::from_code(self.kind)?;
        let path = match self.target {
            Target::Payload { content, .. } => {
                crate::store::content_relative(kind, self.schema, node::Digest::from_bytes(content))
            }
            Target::Envelope { .. } => {
                crate::store::action_relative(kind, node::ActionKey::from_bytes(self.action))
            }
            Target::Index { lookup } => crate::store::index_relative(
                kind,
                node::LookupKey::from_bytes(lookup),
                node::ActionKey::from_bytes(self.action),
            ),
        };
        (path_digest(&path) == self.relative).then_some(path)
    }

    pub const fn member(&self) -> &'static str {
        match self.target {
            Target::Payload { .. } => "payload",
            Target::Envelope { .. } => "envelope",
            Target::Index { .. } => "row",
        }
    }
}

pub fn path_digest(path: &Path) -> [u8; 32] {
    Sha256::digest(path.as_os_str().as_encoded_bytes()).into()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Consumed {
    pub kind: u8,
    pub schema: u32,
    pub action: [u8; 32],
    pub recipe: [u8; 32],
    pub content: [u8; 32],
    pub bundled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    Published,
    Existing,
    Failed,
}

/// Vec order is the process's observed order; tokens join interleaved attempts.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "event", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Event {
    Temporary {
        change: temporary::Event,
    },
    Bundle {
        change: bundle::Event,
    },
    Recovery {
        change: recovery::Event,
    },
    Creation {
        change: creation::Event,
    },
    Retirement {
        change: retirement::Event,
    },
    Pin {
        change: pin::Event,
    },
    Ledger {
        change: ledger::Event,
    },
    Started {
        attempt: u64,
        identity: Identity,
    },
    Sealed {
        attempt: u64,
        member: Commitment,
        allocation: Option<Commitment>,
    },
    Renamed {
        attempt: u64,
    },
    Finished {
        attempt: u64,
        outcome: Outcome,
    },
    Admitted {
        admission: u64,
        root: [u8; 32],
        publisher: [u8; 32],
        proof: [u8; 32],
        entries: u64,
    },
    Consumed {
        admission: u64,
        index: u64,
        entry: Consumed,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Fault {
    Unscoped,
    Budget,
    Allocation,
    Frame,
    Identity,
    Exhausted,
    UnsupportedTransport,
    Account,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub events: Events,
    pub fault: Option<Fault>,
    pub cell_transport: bool,
}

/// Borrowed original account: sinks cannot create a replacement evidence budget.
pub struct Observation<'a> {
    pub(crate) event: Result<Event, Fault>,
    pub(crate) budget: Account<'a>,
}

/// Only these two views can charge the already-established original account.
#[derive(Clone, Copy)]
pub(crate) enum Account<'a> {
    Graph(&'a GraphBudget),
    Retained(&'a node::account::evidence::Usage),
}

impl Account<'_> {
    fn shares(self, usage: &node::account::evidence::Usage) -> bool {
        match self {
            Self::Graph(graph) => usage.shares_with(graph),
            Self::Retained(other) => usage.shares_usage(other),
        }
    }

    fn retain(self) -> node::account::evidence::Usage {
        match self {
            Self::Graph(graph) => node::account::evidence::Usage::retain(graph),
            Self::Retained(usage) => usage.clone(),
        }
    }

    fn decode(self, bytes: usize) -> Result<(), node::Error> {
        match self {
            Self::Graph(graph) => graph.decode(bytes),
            Self::Retained(usage) => {
                let (result, observation) = usage.charge(bytes);
                if let Some(observation) = observation {
                    super::record_graph(observation);
                }
                result
            }
        }
    }
}

/// Immutable shared rows: copying a Snapshot does not duplicate event storage.
#[derive(Clone, Default)]
pub struct Events {
    rows: Option<std::sync::Arc<Vec<Event>>>,
    budget: Option<node::account::evidence::Usage>,
    wire_bytes: usize,
}

impl Events {
    pub fn as_slice(&self) -> &[Event] {
        self.rows.as_ref().map_or(&[], |rows| rows.as_slice())
    }
}
impl std::fmt::Debug for Events {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.as_slice().fmt(out)
    }
}
impl PartialEq for Events {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}
impl Eq for Events {}
impl Serialize for Events {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.as_slice().serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for Events {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Events;
            fn expecting(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                out.write_str("bounded principal observations")
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Events, A::Error> {
                let mut rows = Vec::new();
                while let Some(event) = sequence.next_element::<Event>()? {
                    if rows.len() == rows.capacity() {
                        let capacity = rows.len().saturating_mul(2).max(8);
                        if capacity
                            .checked_mul(std::mem::size_of::<Event>())
                            .is_none_or(|bytes| bytes > super::cell::REPORT_BYTES)
                        {
                            return Err(serde::de::Error::custom(
                                "principal observation frame storage exceeded",
                            ));
                        }
                        let mut next = Vec::new();
                        next.try_reserve_exact(capacity)
                            .map_err(serde::de::Error::custom)?;
                        next.append(&mut rows);
                        rows = next;
                    }
                    rows.push(event);
                }
                Ok(Events {
                    rows: Some(std::sync::Arc::new(rows)),
                    budget: None,
                    wire_bytes: 0,
                })
            }
        }
        deserializer.deserialize_seq(Visitor)
    }
}

pub(crate) struct History {
    events: Vec<Event>,
    pub(crate) fault: Option<Fault>,
    wire_bytes: usize,
    budget: Option<node::account::evidence::Usage>,
}

impl Default for History {
    fn default() -> Self {
        Self {
            events: Vec::new(),
            fault: None,
            wire_bytes: 2,
            budget: None,
        }
    }
}

impl History {
    pub(crate) fn record(&mut self, observation: &Observation<'_>) {
        if self.fault.is_some() {
            return;
        }
        // The paired cell observes one original graph. A future second graph
        // needs its own accounting design, not reassignment to the last writer.
        if self
            .budget
            .as_ref()
            .is_some_and(|budget| !observation.budget.shares(budget))
        {
            self.fault = Some(Fault::Account);
            return;
        }
        let event = match observation.event {
            Ok(event) => event,
            Err(fault) => {
                self.fault = Some(fault);
                return;
            }
        };
        let mut count = Count(0);
        if serde_json::to_writer(&mut count, &event).is_err()
            || self
                .wire_bytes
                .checked_add(count.0 + usize::from(!self.events.is_empty()))
                .is_none_or(|bytes| bytes > super::cell::REPORT_BYTES)
        {
            self.fault = Some(Fault::Frame);
            return;
        }
        if self.events.len() == self.events.capacity() {
            let capacity = self.events.len().saturating_mul(2).max(8);
            let Some(bytes) = capacity.checked_mul(std::mem::size_of::<Event>()) else {
                self.fault = Some(Fault::Frame);
                return;
            };
            if bytes > super::cell::REPORT_BYTES {
                self.fault = Some(Fault::Frame);
                return;
            }
            if observation.budget.decode(bytes).is_err() {
                self.fault = Some(Fault::Budget);
                return;
            }
            let mut replacement = Vec::new();
            if replacement.try_reserve_exact(capacity).is_err() {
                self.fault = Some(Fault::Allocation);
                return;
            }
            replacement.append(&mut self.events);
            self.events = replacement;
        }
        self.wire_bytes += count.0 + usize::from(!self.events.is_empty());
        self.events.push(event);
        self.budget
            .get_or_insert_with(|| observation.budget.retain());
    }

    pub(crate) fn snapshot(&self) -> (Evidence, Option<super::accounting::Graph>) {
        let mut fault = self.fault;
        if self.events.is_empty() {
            return (
                Evidence {
                    events: Events::default(),
                    fault,
                    cell_transport: false,
                },
                None,
            );
        }
        let arc = std::alloc::Layout::new::<[std::sync::atomic::AtomicUsize; 2]>()
            .extend(std::alloc::Layout::new::<Vec<Event>>())
            .map(|(layout, _)| layout.pad_to_align().size());
        let bytes = arc
            .ok()
            .and_then(|arc| (self.events.len() * std::mem::size_of::<Event>()).checked_add(arc));
        let (charge, observation) = match (bytes, &self.budget) {
            (Some(bytes), Some(budget)) => budget.charge(bytes),
            _ => (Err(node::Error::Allocation), None),
        };
        let mut rows = Vec::new();
        if charge.is_err() {
            fault.get_or_insert(Fault::Budget);
            return (
                Evidence {
                    events: Events::default(),
                    fault,
                    cell_transport: false,
                },
                observation,
            );
        }
        if rows.try_reserve_exact(self.events.len()).is_err() {
            fault.get_or_insert(Fault::Allocation);
            return (
                Evidence {
                    events: Events::default(),
                    fault,
                    cell_transport: false,
                },
                observation,
            );
        }
        rows.extend_from_slice(&self.events);
        (
            Evidence {
                events: Events {
                    rows: Some(std::sync::Arc::new(rows)),
                    budget: self.budget.clone(),
                    wire_bytes: self.wire_bytes,
                },
                fault,
                cell_transport: false,
            },
            observation,
        )
    }
}

struct Count(usize);
impl std::io::Write for Count {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .ok_or(std::io::ErrorKind::OutOfMemory)?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A typed attempt starts before entry publication and finishes after cleanup.
pub(crate) struct Attempt<'a> {
    token: u64,
    budget: &'a GraphBudget,
    outcome: std::cell::Cell<Outcome>,
}

impl<'a> Attempt<'a> {
    pub(crate) fn temporary(&self) -> temporary::Writer<'_> {
        temporary::Writer::principal(self.token, self.budget)
    }

    pub(crate) fn start(identity: Option<Identity>, budget: &'a GraphBudget) -> Self {
        if !super::principal_enabled() {
            return Self {
                token: 0,
                budget,
                outcome: std::cell::Cell::new(Outcome::Failed),
            };
        }
        let token = next();
        let event = if token == 0 {
            Err(Fault::Exhausted)
        } else {
            identity
                .map(|identity| Event::Started {
                    attempt: token,
                    identity,
                })
                .ok_or(Fault::Identity)
        };
        super::record_principal(&Observation {
            event,
            budget: Account::Graph(budget),
        });
        Self {
            token,
            budget,
            outcome: std::cell::Cell::new(Outcome::Failed),
        }
    }

    pub(crate) fn sealed(&self, member: Commitment, allocation: Option<&[u8]>) {
        if self.token == 0 {
            return;
        }
        let event = if allocation.is_some_and(|bytes| self.budget.hash(bytes.len()).is_err()) {
            Err(Fault::Budget)
        } else {
            Ok(Event::Sealed {
                attempt: self.token,
                member,
                allocation: allocation.map(Commitment::produced),
            })
        };
        super::record_principal(&Observation {
            event,
            budget: Account::Graph(self.budget),
        });
    }

    pub(crate) fn row(&self, bytes: &[u8]) {
        if self.token == 0 {
            return;
        }
        if self.budget.hash(bytes.len()).is_err() {
            super::record_principal(&Observation {
                event: Err(Fault::Budget),
                budget: Account::Graph(self.budget),
            });
        } else {
            self.sealed(Commitment::produced(bytes), None);
        }
    }

    pub(crate) fn renamed(&self) {
        if self.token == 0 {
            return;
        }
        super::record_principal(&Observation {
            event: Ok(Event::Renamed {
                attempt: self.token,
            }),
            budget: Account::Graph(self.budget),
        });
    }

    pub(crate) fn completed(&self, existing: bool) {
        self.outcome.set(if existing {
            Outcome::Existing
        } else {
            Outcome::Published
        });
    }
}

impl Drop for Attempt<'_> {
    fn drop(&mut self) {
        if self.token == 0 {
            return;
        }
        super::record_principal(&Observation {
            event: Ok(Event::Finished {
                attempt: self.token,
                outcome: self.outcome.get(),
            }),
            budget: Account::Graph(self.budget),
        });
    }
}

pub(crate) fn admitted(
    root: &Path,
    publisher: node::Digest,
    proof: &node::ReadSetProof,
    budget: &GraphBudget,
) {
    if !super::principal_enabled() {
        return;
    }
    let token = next();
    if token == 0
        || budget
            .hash(root.as_os_str().as_encoded_bytes().len())
            .is_err()
    {
        super::record_principal(&Observation {
            event: Err(Fault::Budget),
            budget: Account::Graph(budget),
        });
        return;
    }
    super::record_principal(&Observation {
        event: Ok(Event::Admitted {
            admission: token,
            root: path_digest(root),
            publisher: publisher.as_bytes(),
            proof: proof.digest().as_bytes(),
            entries: proof.entries().len() as u64,
        }),
        budget: Account::Graph(budget),
    });
    for (index, entry) in proof.entries().iter().enumerate() {
        super::record_principal(&Observation {
            event: Ok(Event::Consumed {
                admission: token,
                index: index as u64,
                entry: Consumed {
                    kind: entry.kind as u8,
                    schema: entry.schema,
                    action: entry.action.as_bytes(),
                    recipe: entry.recipe.as_bytes(),
                    content: entry.content.as_bytes(),
                    bundled: entry.bundled,
                },
            }),
            budget: Account::Graph(budget),
        });
    }
}

impl Evidence {
    pub(crate) fn prepare_cell_transport(
        &mut self,
        accounting: &mut super::accounting::Evidence,
        limit: u64,
    ) -> Result<(), &'static str> {
        // Session::finish reaches exactly payload-digest encoding then envelope
        // frame encoding. Parent re-encoding has no producer account handle.
        if let Some(budget) = &self.events.budget {
            for encoding in 0..2 {
                let (result, observation) = budget.charge(self.events.wire_bytes);
                if let Some(observation) = observation {
                    accounting.record_graph(observation, limit);
                }
                result.map_err(|_| "principal transport exceeded original account")?;
                if encoding == 0 {
                    // Only the first canonical copy feeds the payload digest.
                    let (result, observation) = budget.hash(self.events.wire_bytes);
                    if let Some(observation) = observation {
                        accounting.record_graph(observation, limit);
                    }
                    result.map_err(|_| "principal transport hash exceeded original account")?;
                }
            }
        }
        self.cell_transport = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
