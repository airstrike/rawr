//! Source-directed traversal replay; retirement is distinct from complete EOF.
//!
//! walkdir 2.5.0 lib.rs:901,1007,1072: deferred opening errors,
//! source-requested draining and prune-before-read remain separate transitions.
//! This transport checks their requested order; the contained caller decides
//! which operation to request. Consumption grants no current-source authority.

use super::{observed, stored, stream, Host};
use crate::input::membership::{Error, Result};
use observed::traversal;

pub(in super::super) use observed::traversal::Close;

/// One invocation borrows immutable events and is bound to its issuing Host.
/// Linear tokens carry active state; dropping one leaves `owners` outstanding.
pub(in super::super) struct Cursor<'image> {
    image: &'image stored::Stored,
    scope: stream::Scope,
    invocation: u64,
    record: usize,
    events: &'image [traversal::Event],
    position: usize,
    next_open: u64,
    owners: usize,
    refused: bool,
}

/// Private construction and consuming retirement prevent occurrence reuse.
pub(in super::super) struct Directory {
    scope: stream::Scope,
    invocation: u64,
    id: u64,
    opening: Option<i32>,
    union: bool,
    state: State,
}

#[derive(Clone, Copy)]
enum State {
    Unread,
    Reading,
    Error,
    Eof,
}

/// Descriptor identity stays owned until the source's explicit release.
pub(in super::super) struct Identity {
    scope: stream::Scope,
    invocation: u64,
    id: u64,
    device: u64,
    inode: u64,
}

/// The native entry/type operation is already materialized in this image row.
pub(in super::super) enum Read<'image> {
    Entry {
        name: &'image [u8],
        kind: std::result::Result<observed::Kind, i32>,
    },
    Eof,
    Error {
        code: i32,
        opening: bool,
    },
}

impl Directory {
    /// WalkDir owns an opening error's path before later delivery or pruning.
    pub(in super::super) fn opening_error(&self) -> Option<i32> {
        self.opening
    }
}

impl<'image> Host<'image, '_> {
    pub(in super::super) fn traversal(
        &mut self,
        root: &[u8],
        is_root: bool,
    ) -> Result<Cursor<'image>> {
        self.budget.allocation.check()?;
        let invocation = self.next_traversal;
        let next = invocation
            .checked_add(1)
            .ok_or(Error::Encoding("traversal invocation overflow"))?;
        let root = observed::Path::from_bytes(root, &mut self.budget)?;
        let record = self.lookup(&observed::Query::Traversal {
            root,
            is_root,
            invocation,
        })?;
        let observed::Value::Traversal(transcript) = &self.image.records()[record].value else {
            return Err(Error::ObservationType);
        };
        self.budget.allocation.items(1)?;
        let streams = self
            .streams
            .checked_add(1)
            .ok_or(Error::Encoding("source cursor count overflow"))?;
        self.next_traversal = next;
        self.streams = streams;
        Ok(Cursor {
            image: self.image,
            scope: self.scope,
            invocation,
            record,
            events: &transcript.events,
            position: 0,
            next_open: 0,
            owners: 0,
            refused: false,
        })
    }

    /// Preserve recorded metadata errno as source data; missing evidence fails.
    pub(in super::super) fn traversal_metadata(
        &mut self,
        path: &[u8],
        follow: bool,
    ) -> Result<std::result::Result<observed::Metadata, i32>> {
        self.budget.allocation.check()?;
        let path = observed::Path::from_bytes(path, &mut self.budget)?;
        match self.observe(&observed::Query::Metadata { path, follow })? {
            observed::Value::Metadata(value) => Ok(Ok(*value)),
            observed::Value::Error(code) if *code > 0 => Ok(Err(*code)),
            observed::Value::Error(_) => Err(Error::Encoding("invalid metadata errno")),
            // Absence alone does not retain the exact native errno requested
            // by this source operation. It cannot manufacture ENOENT here.
            observed::Value::Absent => Err(Error::Unobserved),
            _ => Err(Error::ObservationType),
        }
    }
}

impl<'image> Cursor<'image> {
    fn begin(&mut self, host: &mut Host<'image, '_>) -> Result<()> {
        if self.refused {
            return Err(Error::Unconsumed);
        }
        self.refused = true;
        if self.scope != host.scope || !std::ptr::eq(self.image, host.image) {
            return Err(Error::ObservationType);
        }
        host.budget.allocation.check()?;
        host.budget.allocation.items(1)?;
        Ok(())
    }

    fn event(&self) -> Result<&'image traversal::Event> {
        self.events.get(self.position).ok_or(Error::Unobserved)
    }

    fn consumed(&mut self) {
        // Every caller first borrowed a real event at this index.
        self.position += 1;
        self.refused = false;
    }

    fn token(&self, scope: stream::Scope, invocation: u64, id: u64) -> Result<()> {
        if scope != self.scope || invocation != self.invocation || id >= self.next_open {
            return Err(Error::ObservationType);
        }
        Ok(())
    }

    fn occurrence(&self) -> Result<(u64, u64)> {
        let next = self
            .next_open
            .checked_add(1)
            .ok_or(Error::Encoding("traversal open ordinal overflow"))?;
        Ok((self.next_open, next))
    }

    pub(in super::super) fn open(
        &mut self,
        host: &mut Host<'image, '_>,
        path: &[u8],
    ) -> Result<Directory> {
        self.begin(host)?;
        host.budget.graph.paths(1)?;
        let traversal::Event::Open {
            path: expected,
            result,
        } = self.event()?
        else {
            return Err(Error::ObservationType);
        };
        path_matches(host, path, expected)?;
        let (id, next) = self.occurrence()?;
        let owners = self.owners.checked_add(1).ok_or(Error::Unconsumed)?;
        let opening = match result {
            traversal::Open::Failed(code) | traversal::Open::UnionFailed { error: code, .. } => {
                Some(*code)
            }
            traversal::Open::Started(_) => None,
        };
        let union = matches!(
            result,
            traversal::Open::Started(rawr_directory::Mount::Known(true))
        );
        self.next_open = next;
        self.owners = owners;
        self.consumed();
        Ok(Directory {
            scope: self.scope,
            invocation: self.invocation,
            id,
            opening,
            union,
            state: State::Unread,
        })
    }

    pub(in super::super) fn next(
        &mut self,
        host: &mut Host<'image, '_>,
        directory: &mut Directory,
    ) -> Result<Read<'image>> {
        self.begin(host)?;
        self.token(directory.scope, directory.invocation, directory.id)?;
        let traversal::Event::Next { id, result } = self.event()? else {
            return Err(Error::ObservationType);
        };
        if *id != directory.id || matches!(directory.state, State::Eof) {
            return Err(Error::ObservationType);
        }
        let (state, read) = match result {
            traversal::Next::Entry(row)
                if directory.opening.is_none()
                    && matches!(directory.state, State::Unread | State::Reading) =>
            {
                let kind = row.inspected.ok_or(Error::Unobserved)?;
                if observed::stream::hint(row.raw_kind).is_none() {
                    host.budget.graph.paths(1)?;
                }
                (
                    State::Reading,
                    Read::Entry {
                        name: &row.name,
                        kind,
                    },
                )
            }
            traversal::Next::ReadError(code)
                if directory.opening.is_none()
                    && !directory.union
                    && matches!(directory.state, State::Unread | State::Reading) =>
            {
                (
                    State::Error,
                    Read::Error {
                        code: *code,
                        opening: false,
                    },
                )
            }
            traversal::Next::OpenError(code)
                if directory.opening == Some(*code) && matches!(directory.state, State::Unread) =>
            {
                (
                    State::Error,
                    Read::Error {
                        code: *code,
                        opening: true,
                    },
                )
            }
            traversal::Next::Eof
                if directory.opening.is_none() || matches!(directory.state, State::Error) =>
            {
                (State::Eof, Read::Eof)
            }
            _ => return Err(Error::ObservationType),
        };
        directory.state = state;
        self.consumed();
        Ok(read)
    }

    pub(in super::super) fn close(
        &mut self,
        host: &mut Host<'image, '_>,
        directory: Directory,
        reason: Close,
    ) -> Result<()> {
        self.begin(host)?;
        self.token(directory.scope, directory.invocation, directory.id)?;
        let traversal::Event::Close {
            id,
            reason: expected,
        } = self.event()?
        else {
            return Err(Error::ObservationType);
        };
        if *id != directory.id || *expected != reason {
            return Err(Error::ObservationType);
        }
        match reason {
            Close::Pruned if matches!(directory.state, State::Unread) => {}
            Close::Drained | Close::Exhausted if matches!(directory.state, State::Eof) => {}
            _ => return Err(Error::Unconsumed),
        }
        self.owners = self.owners.checked_sub(1).ok_or(Error::Unconsumed)?;
        self.consumed();
        Ok(())
    }

    pub(in super::super) fn handle(
        &mut self,
        host: &mut Host<'image, '_>,
        path: &[u8],
    ) -> Result<std::result::Result<Identity, i32>> {
        self.begin(host)?;
        host.budget.graph.paths(1)?;
        let traversal::Event::Handle {
            path: expected,
            result,
        } = self.event()?
        else {
            return Err(Error::ObservationType);
        };
        path_matches(host, path, expected)?;
        let (id, next) = self.occurrence()?;
        let result = match result {
            traversal::Handle::OpenFailed(code) | traversal::Handle::StatFailed(code) => Err(*code),
            traversal::Handle::Opened { device, inode } => {
                self.owners = self.owners.checked_add(1).ok_or(Error::Unconsumed)?;
                Ok(Identity {
                    scope: self.scope,
                    invocation: self.invocation,
                    id,
                    device: *device,
                    inode: *inode,
                })
            }
        };
        self.next_open = next;
        self.consumed();
        Ok(result)
    }

    pub(in super::super) fn same(
        &mut self,
        host: &mut Host<'image, '_>,
        left: &Identity,
        right: &Identity,
    ) -> Result<bool> {
        self.begin(host)?;
        self.token(left.scope, left.invocation, left.id)?;
        self.token(right.scope, right.invocation, right.id)?;
        let same = left.device == right.device && left.inode == right.inode;
        // Equality is pure source work, not a native or serialized event.
        self.refused = false;
        Ok(same)
    }

    pub(in super::super) fn release(
        &mut self,
        host: &mut Host<'image, '_>,
        identity: Identity,
    ) -> Result<()> {
        self.begin(host)?;
        self.token(identity.scope, identity.invocation, identity.id)?;
        let traversal::Event::Release { id } = self.event()? else {
            return Err(Error::ObservationType);
        };
        if *id != identity.id {
            return Err(Error::ObservationType);
        }
        self.owners = self.owners.checked_sub(1).ok_or(Error::Unconsumed)?;
        self.consumed();
        Ok(())
    }

    pub(in super::super) fn finish(mut self, host: &mut Host<'image, '_>) -> Result<()> {
        self.begin(host)?;
        if self.position != self.events.len() || self.owners != 0 {
            return Err(Error::Unconsumed);
        }
        host.streams = host.streams.checked_sub(1).ok_or(Error::Unconsumed)?;
        host.used[self.record] = true;
        Ok(())
    }
}

fn path_matches(host: &mut Host<'_, '_>, path: &[u8], expected: &observed::Path) -> Result<()> {
    host.budget
        .allocation
        .string_exceeded("membership native path bytes", path.len());
    host.budget.allocation.items(path.len())?;
    if !path.starts_with(b"/") || path.contains(&0) {
        return Err(Error::Encoding("expected absolute native path"));
    }
    host.budget.allocation.items(path.len())?;
    if path != expected.bytes() {
        return Err(Error::ObservationType);
    }
    Ok(())
}
