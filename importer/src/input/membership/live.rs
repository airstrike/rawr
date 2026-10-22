//! Live acquisition data from one associated stream, without source authority.
//!
//! The full selector must still bind complete before/actual-cold/after evidence
//! and independently reacquire current observations at each admission gate.

#![forbid(unsafe_code)]

use super::{observed, Budget, Result};

mod canonical;
mod discovery;
mod file;
pub(super) mod git_read;
mod native;
pub(super) mod stream;
pub(super) mod traversal;

#[cfg(test)]
mod tests;

/// File data together with the optional identity observations made around it.
/// Neither successful acquisition nor matched metadata supplies freshness.
#[derive(Debug)]
pub(super) struct Acquired {
    record: observed::Record,
    identity: Identity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Stat {
    Metadata(Metadata),
    Error(i32),
}

/// Lossless optional live facts. Signed native nanoseconds are retained even
/// when they cannot form the separate stored Metadata observation schema.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Metadata {
    kind: observed::Kind,
    device: u64,
    inode: u64,
    owner: u32,
    mode: u32,
    length: u64,
    modified_seconds: i64,
    modified_nanos: i64,
    changed_seconds: i64,
    changed_nanos: i64,
}

impl Metadata {
    pub(super) fn kind(&self) -> observed::Kind {
        self.kind
    }
    pub(super) fn device(&self) -> u64 {
        self.device
    }
    pub(super) fn inode(&self) -> u64 {
        self.inode
    }
    pub(super) fn owner(&self) -> u32 {
        self.owner
    }
    pub(super) fn mode(&self) -> u32 {
        self.mode
    }
    pub(super) fn length(&self) -> u64 {
        self.length
    }
    pub(super) fn modified_seconds(&self) -> i64 {
        self.modified_seconds
    }
    pub(super) fn modified_nanos(&self) -> i64 {
        self.modified_nanos
    }
    pub(super) fn changed_seconds(&self) -> i64 {
        self.changed_seconds
    }
    pub(super) fn changed_nanos(&self) -> i64 {
        self.changed_nanos
    }
}

/// Descriptor observations are absent when opening did not produce a handle.
#[derive(Debug)]
pub(super) struct Identity {
    before_path: Stat,
    opened: Option<Stat>,
    after_opened: Option<Stat>,
    after_path: Stat,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Comparison {
    Matched,
    Changed,
    Unavailable,
}

impl Acquired {
    pub(super) fn record(&self) -> &observed::Record {
        &self.record
    }

    pub(super) fn identity(&self) -> &Identity {
        &self.identity
    }
}

impl Identity {
    pub(super) fn before_path(&self) -> &Stat {
        &self.before_path
    }

    pub(super) fn opened(&self) -> Option<&Stat> {
        self.opened.as_ref()
    }

    pub(super) fn after_opened(&self) -> Option<&Stat> {
        self.after_opened.as_ref()
    }

    pub(super) fn after_path(&self) -> &Stat {
        &self.after_path
    }

    pub(super) fn comparison(&self) -> Comparison {
        let mut first = None;
        let mut complete = true;
        for observation in [
            Some(self.before_path),
            self.opened,
            self.after_opened,
            Some(self.after_path),
        ] {
            match observation {
                Some(Stat::Metadata(metadata)) => {
                    if first.is_some_and(|first| first != metadata) {
                        return Comparison::Changed;
                    }
                    first = Some(metadata);
                }
                Some(Stat::Error(_)) | None => complete = false,
            }
        }
        if complete {
            Comparison::Matched
        } else {
            Comparison::Unavailable
        }
    }
}

/// Acquire the exact native manifest query. A manifest is a manifest-class
/// read bound to the physical `process::TAIL_BYTES`, never to the
/// configurable manifest target. The result has no decoder/constructor path
/// from a serialized EOF assertion.
pub(super) fn manifest(path: &[u8], budget: &mut Budget<'_>) -> Result<Acquired> {
    file::acquire(
        path,
        crate::process::TAIL_BYTES,
        budget,
        &mut native::System,
    )
}

/// Acquire only the pinned physical discovery projection under original limits.
/// Actual cold Cargo warning success and complete current association are separate.
pub(super) fn discovery(
    directory: &[u8],
    filename: observed::discovery::Filename,
    warn: bool,
    budget: &mut Budget<'_>,
) -> Result<observed::Record> {
    discovery::acquire(directory, filename, warn, budget, &mut native::System)
}

/// Unconnected acquisition data; native qualification and complete association
/// remain prerequisites before this can feed any current admission gate.
pub(super) fn canonical(path: &[u8], budget: &mut Budget<'_>) -> Result<observed::Record> {
    canonical::acquire(path, budget, &mut native::System)
}
