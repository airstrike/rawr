//! Reached temporary recovery and thaw requests, with no inferred old producer.

use std::cell::Cell;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{Account, Commitment, Fault, Observation};
use crate::node::GraphBudget;
use crate::store::Publication;

pub mod join;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Origin {
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "recovery", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Event {
    Batch {
        batch: u64,
        publication: u64,
        root: [u8; 32],
    },
    Path {
        path: u64,
        commitment: Commitment,
    },
    Bytes {
        path: u64,
        index: u64,
        used: u8,
        bytes: [u8; 32],
    },
    Measured {
        batch: u64,
        index: u64,
        path: u64,
        directory: bool,
        bytes: u64,
        members: u64,
        origin: Origin,
    },
    Ready {
        batch: u64,
        count: u64,
    },
    Started {
        batch: u64,
        attempt: u64,
        index: u64,
    },
    Thaw {
        attempt: u64,
    },
    Mode {
        attempt: u64,
        change: u64,
        directory: bool,
        before: u32,
        requested: u32,
    },
    ModeResult {
        change: u64,
        succeeded: bool,
    },
    Thawed {
        attempt: u64,
        succeeded: bool,
    },
    RemovalStarted {
        attempt: u64,
    },
    Removed {
        attempt: u64,
        succeeded: bool,
    },
    Finished {
        attempt: u64,
        succeeded: bool,
    },
    Synced {
        batch: u64,
        succeeded: bool,
    },
    BatchFinished {
        batch: u64,
        recovered: Option<bool>,
    },
}

pub(crate) struct Batch<'a> {
    token: u64,
    root: &'a Path,
    graph: &'a GraphBudget,
    recovered: Cell<Option<bool>>,
}
impl<'a> Batch<'a> {
    pub(crate) fn start(publication: &'a Publication<'_>, root: &'a Path) -> Self {
        let mut value = Self {
            token: 0,
            root,
            graph: publication.graph(),
            recovered: Cell::new(None),
        };
        if !crate::metrics::principal_enabled() {
            return value;
        }
        value.token = super::next();
        let event = (|| {
            if value.token == 0 {
                return Err(Fault::Exhausted);
            }
            if publication.root() != root || publication.observation() == 0 {
                return Err(Fault::Identity);
            }
            value
                .graph
                .hash(root.as_os_str().as_encoded_bytes().len())
                .map_err(|_| Fault::Budget)?;
            Ok(Event::Batch {
                batch: value.token,
                publication: publication.observation(),
                root: super::path_digest(root),
            })
        })();
        emit(value.graph, event);
        value
    }
    pub(crate) fn measured(
        &self,
        index: usize,
        path: &Path,
        directory: bool,
        bytes: u64,
        members: u64,
    ) {
        if self.token == 0 {
            return;
        }
        let path = path_rows(self.graph, self.root, path);
        emit(
            self.graph,
            Ok(Event::Measured {
                batch: self.token,
                index: index as u64,
                path,
                directory,
                bytes,
                members,
                origin: Origin::Unknown,
            }),
        );
    }
    pub(crate) fn ready(&self, count: usize) {
        if self.token != 0 {
            emit(
                self.graph,
                Ok(Event::Ready {
                    batch: self.token,
                    count: count as u64,
                }),
            );
        }
    }
    pub(crate) fn temporary(&self, index: usize) -> Attempt<'_> {
        let token = if self.token == 0 { 0 } else { super::next() };
        if self.token != 0 {
            emit(
                self.graph,
                if token == 0 {
                    Err(Fault::Exhausted)
                } else {
                    Ok(Event::Started {
                        batch: self.token,
                        attempt: token,
                        index: index as u64,
                    })
                },
            );
        }
        Attempt {
            token,
            graph: self.graph,
            root: self.root,
            succeeded: Cell::new(false),
        }
    }
    pub(crate) fn synced(&self, succeeded: bool) {
        if self.token != 0 {
            emit(
                self.graph,
                Ok(Event::Synced {
                    batch: self.token,
                    succeeded,
                }),
            );
        }
    }
    pub(crate) fn completed(&self, recovered: bool) {
        self.recovered.set(Some(recovered));
    }
}
impl Drop for Batch<'_> {
    fn drop(&mut self) {
        if self.token != 0 {
            emit(
                self.graph,
                Ok(Event::BatchFinished {
                    batch: self.token,
                    recovered: self.recovered.get(),
                }),
            );
        }
    }
}

pub(crate) struct Attempt<'a> {
    token: u64,
    graph: &'a GraphBudget,
    root: &'a Path,
    succeeded: Cell<bool>,
}
impl Attempt<'_> {
    pub(crate) fn thaw(&self) {
        self.event(Event::Thaw {
            attempt: self.token,
        });
    }
    pub(crate) fn mode(&self, path: &Path, directory: bool, before: u32, requested: u32) -> u64 {
        if self.token == 0 {
            return 0;
        }
        let change = path_rows(self.graph, self.root, path);
        self.event(Event::Mode {
            attempt: self.token,
            change,
            directory,
            before,
            requested,
        });
        change
    }
    pub(crate) fn mode_result(&self, change: u64, succeeded: bool) {
        self.event(Event::ModeResult { change, succeeded });
    }
    pub(crate) fn thawed(&self, succeeded: bool) {
        self.event(Event::Thawed {
            attempt: self.token,
            succeeded,
        });
    }
    pub(crate) fn removal_started(&self) {
        self.event(Event::RemovalStarted {
            attempt: self.token,
        });
    }
    pub(crate) fn removed(&self, succeeded: bool) {
        self.event(Event::Removed {
            attempt: self.token,
            succeeded,
        });
    }
    pub(crate) fn completed(&self) {
        self.succeeded.set(true);
    }
    fn event(&self, event: Event) {
        if self.token != 0 {
            emit(self.graph, Ok(event));
        }
    }
}
impl Drop for Attempt<'_> {
    fn drop(&mut self) {
        self.event(Event::Finished {
            attempt: self.token,
            succeeded: self.succeeded.get(),
        });
    }
}

// The original path remains borrowed. Only fixed Copy rows enter the existing
// admitted Event storage; there is no unaccounted retained path allocation.
fn path_rows(graph: &GraphBudget, root: &Path, path: &Path) -> u64 {
    let result = (|| {
        let relative = path.strip_prefix(root).map_err(|_| Fault::Identity)?;
        let bytes = relative.as_os_str().as_encoded_bytes();
        graph.hash(bytes.len()).map_err(|_| Fault::Budget)?;
        let token = super::next();
        if token == 0 {
            return Err(Fault::Exhausted);
        }
        emit(
            graph,
            Ok(Event::Path {
                path: token,
                commitment: Commitment {
                    bytes: bytes.len() as u64,
                    sha256: super::path_digest(relative),
                },
            }),
        );
        for (index, chunk) in bytes.chunks(32).enumerate() {
            let mut row = [0; 32];
            row[..chunk.len()].copy_from_slice(chunk);
            emit(
                graph,
                Ok(Event::Bytes {
                    path: token,
                    index: index as u64,
                    used: chunk.len() as u8,
                    bytes: row,
                }),
            );
        }
        Ok(token)
    })();
    match result {
        Ok(token) => token,
        Err(fault) => {
            emit(graph, Err(fault));
            0
        }
    }
}
fn emit(graph: &GraphBudget, event: Result<Event, Fault>) {
    crate::metrics::record_principal(&Observation {
        event: event.map(|change| super::Event::Recovery { change }),
        budget: Account::Graph(graph),
    });
}
