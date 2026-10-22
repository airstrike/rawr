//! Traversal data. Only the current replay owner can validate its history.

use super::{stream::Row, Path};
use crate::input::membership::{Error, Result};

#[derive(Debug, Eq, PartialEq)]
pub(in crate::input::membership) struct Transcript {
    pub(in crate::input::membership) events: Vec<Event>,
}

/// Open and Handle attempts each consume the next implicit occurrence ordinal,
/// including failed attempts. IDs in later events refer to that shared sequence.
#[derive(Debug, Eq, PartialEq)]
pub(in crate::input::membership) enum Event {
    Open { path: Path, result: Open },
    Next { id: u64, result: Next },
    Close { id: u64, reason: Close },
    Handle { path: Path, result: Handle },
    Release { id: u64 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::input::membership) enum Open {
    Failed(i32),
    UnionFailed {
        mount: rawr_directory::Mount,
        error: i32,
    },
    Started(rawr_directory::Mount),
}

#[derive(Debug, Eq, PartialEq)]
pub(in crate::input::membership) enum Next {
    Entry(Row),
    Eof,
    ReadError(i32),
    OpenError(i32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::input::membership) enum Close {
    Pruned,
    Drained,
    Exhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::input::membership) enum Handle {
    OpenFailed(i32),
    StatFailed(i32),
    Opened { device: u64, inode: u64 },
}

impl Transcript {
    pub(in crate::input::membership) fn validate(&self) -> Result<()> {
        for event in &self.events {
            event.validate()?;
        }
        Ok(())
    }
}

impl Event {
    /// Field shapes only: this never authorizes a recorded prune, ID, release
    /// or terminal state. The replay cursor enforces those reached transitions.
    pub(in crate::input::membership) fn validate(&self) -> Result<()> {
        match self {
            Self::Open { path, result } => {
                native_path(path)?;
                match result {
                    Open::Failed(code) => errno(*code),
                    Open::UnionFailed {
                        mount: rawr_directory::Mount::Known(true),
                        error,
                    } => errno(*error),
                    Open::UnionFailed { .. } => Err(Error::Encoding(
                        "traversal union failure without union mount",
                    )),
                    Open::Started(mount) => match mount {
                        rawr_directory::Mount::Failed(code) => errno(*code),
                        rawr_directory::Mount::Known(_) => Ok(()),
                    },
                }
            }
            Self::Next { result, .. } => match result {
                Next::Entry(row) => {
                    row.validate()?;
                    if row.inspected.is_none() {
                        return Err(Error::Encoding("traversal entry lacks reached type result"));
                    }
                    Ok(())
                }
                Next::Eof => Ok(()),
                Next::ReadError(code) | Next::OpenError(code) => errno(*code),
            },
            Self::Handle { path, result } => {
                native_path(path)?;
                match result {
                    Handle::OpenFailed(code) | Handle::StatFailed(code) => errno(*code),
                    Handle::Opened { .. } => Ok(()),
                }
            }
            Self::Close { .. } | Self::Release { .. } => Ok(()),
        }
    }
}

fn errno(code: i32) -> Result<()> {
    if code <= 0 {
        return Err(Error::Encoding("traversal native error must be positive"));
    }
    Ok(())
}

fn native_path(path: &Path) -> Result<()> {
    if !path.bytes().starts_with(b"/") || path.bytes().contains(&0) {
        return Err(Error::Encoding("invalid traversal native path"));
    }
    Ok(())
}
