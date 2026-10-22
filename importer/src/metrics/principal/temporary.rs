//! Publication-temporary occurrences linked to their existing source attempt.

use super::{Account, Fault, Observation};
use serde::{Deserialize, Serialize};
use std::cell::Cell;

pub mod join;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    Principal(u64),
    Pin(u64),
    Ledger(u64),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Form {
    Directory,
    Sealed,
    Replacement,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Member {
    Primary,
    Allocation,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Name {
    Payload,
    Envelope,
    Row,
    Root,
    Replacement,
}
impl Name {
    pub(super) fn file(self) -> Option<&'static str> {
        match self {
            Self::Payload => Some("payload"),
            Self::Envelope => Some("envelope"),
            Self::Row => Some("row"),
            Self::Root => Some("root"),
            Self::Replacement => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    CreateDirectory,
    Open(Member),
    Write(Member),
    SyncFile(Member),
    SyncTemporary,
    Rename,
    SyncParent,
    Cleanup,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    Published,
    Existing,
    Failed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "lifecycle", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Event {
    Started {
        attempt: u64,
        source: Source,
        form: Form,
        name: Name,
    },
    Reserved {
        attempt: u64,
        temporary: super::retirement::Temporary,
    },
    Step {
        attempt: u64,
        action: Action,
    },
    Result {
        attempt: u64,
        succeeded: bool,
    },
    Operation {
        attempt: u64,
        outcome: Outcome,
    },
    CleanupChecked {
        attempt: u64,
        exists: bool,
    },
    Finished {
        attempt: u64,
        outcome: Outcome,
    },
}

/// Borrows the original source account; no independent history or scratch owner.
#[derive(Clone, Copy)]
pub(crate) struct Writer<'a> {
    source: Source,
    account: Account<'a>,
}
impl<'a> Writer<'a> {
    pub(crate) fn principal(token: u64, graph: &'a crate::node::GraphBudget) -> Self {
        Self {
            source: Source::Principal(token),
            account: Account::Graph(graph),
        }
    }
    pub(crate) fn pin(token: u64, usage: &'a crate::node::account::evidence::Usage) -> Self {
        Self {
            source: Source::Pin(token),
            account: Account::Retained(usage),
        }
    }
    pub(crate) fn ledger(token: u64, graph: &'a crate::node::GraphBudget) -> Self {
        Self {
            source: Source::Ledger(token),
            account: Account::Graph(graph),
        }
    }
    fn emit(&self, event: Result<Event, Fault>) {
        if crate::metrics::principal_enabled() {
            crate::metrics::record_principal(&Observation {
                event: event.map(|change| super::Event::Temporary { change }),
                budget: self.account,
            });
        }
    }
}
pub(crate) struct Attempt<'a> {
    token: u64,
    writer: Option<Writer<'a>>,
    outcome: Cell<Outcome>,
}
impl<'a> Attempt<'a> {
    pub(crate) fn start(writer: Option<Writer<'a>>, form: Form, name: Option<&str>) -> Self {
        let mut value = Self {
            token: 0,
            writer: None,
            outcome: Cell::new(Outcome::Failed),
        };
        if !crate::metrics::principal_enabled() {
            return value;
        }
        let Some(writer) = writer else {
            return value;
        };
        if matches!(
            writer.source,
            Source::Principal(0) | Source::Pin(0) | Source::Ledger(0)
        ) {
            return value;
        }
        value.token = super::next();
        let name = match name {
            Some("payload") => Some(Name::Payload),
            Some("envelope") => Some(Name::Envelope),
            Some("row") => Some(Name::Row),
            Some("root") => Some(Name::Root),
            None if form == Form::Replacement => Some(Name::Replacement),
            _ => None,
        };
        writer.emit(if value.token == 0 {
            Err(Fault::Exhausted)
        } else {
            name.map(|name| Event::Started {
                attempt: value.token,
                source: writer.source,
                form,
                name,
            })
            .ok_or(Fault::Identity)
        });
        value.writer = Some(writer);
        value
    }
    pub(crate) fn reserved(&self, process: u32, sequence: u64) {
        self.emit(Event::Reserved {
            attempt: self.token,
            temporary: super::retirement::Temporary { process, sequence },
        });
    }
    pub(crate) fn step(&self, action: Action) {
        self.emit(Event::Step {
            attempt: self.token,
            action,
        });
    }
    pub(crate) fn result(&self, succeeded: bool) {
        self.emit(Event::Result {
            attempt: self.token,
            succeeded,
        });
    }
    pub(crate) fn operation(&self, outcome: Outcome) {
        self.emit(Event::Operation {
            attempt: self.token,
            outcome,
        });
    }
    pub(crate) fn cleanup_checked(&self, exists: bool) {
        self.emit(Event::CleanupChecked {
            attempt: self.token,
            exists,
        });
    }
    pub(crate) fn completed(&self, outcome: Outcome) {
        self.outcome.set(outcome);
    }
    fn emit(&self, event: Event) {
        if let Some(writer) = &self.writer {
            writer.emit(Ok(event));
        }
    }
}
impl Drop for Attempt<'_> {
    fn drop(&mut self) {
        self.emit(Event::Finished {
            attempt: self.token,
            outcome: self.outcome.get(),
        });
    }
}
