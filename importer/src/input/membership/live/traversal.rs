//! One original-account owner for source-directed directory/loop-handle work.
//! This is kept separate from complete-stream EOF obligations.

use std::ffi::CStr;

use super::{file, native, stream, Stat};
use crate::input::membership::{
    host::stream::Scope,
    observed::{self, traversal},
    Budget, Error, Result,
};

mod storage;

/// An unfinished or refused invocation poisons this owner. Its ordinals cannot
/// be supplied by stored data or reset while retaining the original account.
pub(in super::super) struct Owner<'borrow, 'account> {
    budget: &'borrow mut Budget<'account>,
    scope: Scope,
    next: u64,
    pending: bool,
}

pub(in super::super) struct Capture<'owner, 'borrow, 'account> {
    owner: &'owner mut Owner<'borrow, 'account>,
    root: observed::Path,
    is_root: bool,
    invocation: u64,
    next: u64,
    events: storage::Buffer<traversal::Event>,
    active: storage::Buffer<Active>,
    refused: bool,
}

struct Active {
    id: u64,
    native: Native,
}

enum Native {
    Directory {
        stream: Option<rawr_directory::Stream>,
        error: Option<i32>,
        read: bool,
        eof: bool,
    },
    Handle {
        _descriptor: Descriptor,
        device: u64,
        inode: u64,
    },
}

struct Descriptor(i32);
impl Drop for Descriptor {
    fn drop(&mut self) {
        <native::System as file::System>::close(&mut native::System, self.0);
    }
}

pub(in super::super) struct Directory {
    scope: Scope,
    invocation: u64,
    id: u64,
    error: Option<i32>,
}
impl Directory {
    pub(in super::super) fn opening_error(&self) -> Option<i32> {
        self.error
    }
}

pub(in super::super) struct Identity {
    scope: Scope,
    invocation: u64,
    id: u64,
}

pub(in super::super) enum Read<'entry> {
    Entry {
        name: &'entry [u8],
        kind: std::result::Result<observed::Kind, i32>,
    },
    Eof,
    Error {
        code: i32,
        opening: bool,
    },
}

impl<'borrow, 'account> Owner<'borrow, 'account> {
    pub(in super::super) fn new(budget: &'borrow mut Budget<'account>) -> Result<Self> {
        budget.allocation.check()?;
        Ok(Self {
            budget,
            scope: Scope::new()?,
            next: 0,
            pending: false,
        })
    }

    pub(in super::super) fn begin(
        &mut self,
        root: &[u8],
        is_root: bool,
    ) -> Result<Capture<'_, 'borrow, 'account>> {
        if self.pending {
            return Err(Error::Unconsumed);
        }
        self.pending = true;
        self.budget.allocation.check()?;
        let root = observed::Path::from_bytes(root, self.budget)?;
        let invocation = self.next;
        self.next = self.next.checked_add(1).ok_or(Error::Unconsumed)?;
        Ok(Capture {
            owner: self,
            root,
            is_root,
            invocation,
            next: 0,
            events: storage::Buffer::new(),
            active: storage::Buffer::new(),
            refused: false,
        })
    }
}

impl<'account> Capture<'_, '_, 'account> {
    fn enter(&mut self) -> Result<()> {
        if self.refused {
            return Err(Error::Unconsumed);
        }
        self.refused = true;
        self.owner.budget.allocation.check()?;
        Ok(())
    }

    fn associated(&self, scope: Scope, invocation: u64, id: u64) -> Result<usize> {
        if scope != self.owner.scope || invocation != self.invocation {
            return Err(Error::ObservationType);
        }
        self.active
            .as_slice()
            .iter()
            .position(|active| active.id == id)
            .ok_or(Error::Unconsumed)
    }

    fn ordinal(&mut self) -> Result<u64> {
        let id = self.next;
        self.next = self.next.checked_add(1).ok_or(Error::Unconsumed)?;
        Ok(id)
    }

    /// Reached source probes share the same account; a failed operation cannot
    /// be caught and converted into a successful traversal transcript.
    pub(in super::super) fn probe<T>(
        &mut self,
        operation: impl FnOnce(&mut Budget<'account>) -> Result<T>,
    ) -> Result<T> {
        self.enter()?;
        let value = operation(self.owner.budget)?;
        self.refused = false;
        Ok(value)
    }

    pub(in super::super) fn open(&mut self, path: &[u8]) -> Result<Directory> {
        self.enter()?;
        let path = observed::Path::from_bytes(path, self.owner.budget)?;
        self.events.prepare(self.owner.budget)?;
        self.active.prepare(self.owner.budget)?;
        let id = self.ordinal()?;
        let (result, native, error) = match stream::acquire(&path, self.owner.budget)? {
            stream::Opened::Stream(stream) => {
                (traversal::Open::Started(stream.mount()), Some(stream), None)
            }
            stream::Opened::Failed(observed::stream::Transcript::OpenFailed(error)) => {
                (traversal::Open::Failed(error), None, Some(error))
            }
            stream::Opened::Failed(observed::stream::Transcript::UnionOpenFailed {
                mount,
                error,
            }) => (
                traversal::Open::UnionFailed { mount, error },
                None,
                Some(error),
            ),
            stream::Opened::Failed(_) => return Err(Error::ObservationType),
        };
        self.active.push(Active {
            id,
            native: Native::Directory {
                stream: native,
                error,
                read: false,
                eof: false,
            },
        })?;
        self.events.push(traversal::Event::Open { path, result })?;
        self.refused = false;
        Ok(Directory {
            scope: self.owner.scope,
            invocation: self.invocation,
            id,
            error,
        })
    }

    /// Every WalkDir native entry is inspected before it becomes an owned
    /// recursive entry. Returned bytes borrow only the retained event, never a
    /// descriptor or refill buffer.
    pub(in super::super) fn next(&mut self, directory: &mut Directory) -> Result<Read<'_>> {
        self.enter()?;
        let index = self.associated(directory.scope, directory.invocation, directory.id)?;
        self.events.prepare(self.owner.budget)?;
        let Native::Directory {
            stream,
            error,
            read,
            eof,
        } = &mut self.active.as_mut_slice()[index].native
        else {
            return Err(Error::ObservationType);
        };
        if *eof {
            return Err(Error::Unconsumed);
        }
        *read = true;
        let result = if let Some(code) = error.take() {
            traversal::Next::OpenError(code)
        } else if let Some(stream) = stream {
            match stream.next(&mut stream::Admission(self.owner.budget)) {
                Ok(Some(entry)) => {
                    let name = self.owner.budget.string(entry.name())?;
                    let raw_kind = entry.raw_kind();
                    let kind = entry
                        .file_type(&mut stream::Admission(self.owner.budget))
                        .map_err(stream::failure)?
                        .map(observed::stream::kind);
                    traversal::Next::Entry(observed::stream::Row::checked(
                        name,
                        raw_kind,
                        Some(kind),
                    )?)
                }
                Ok(None) => {
                    *eof = true;
                    traversal::Next::Eof
                }
                Err(rawr_directory::Error::Io { source, .. }) => {
                    traversal::Next::ReadError(stream::errno(source)?)
                }
                Err(error) => return Err(stream::failure(error)),
            }
        } else {
            *eof = true;
            traversal::Next::Eof
        };
        self.events.push(traversal::Event::Next {
            id: directory.id,
            result,
        })?;
        self.refused = false;
        let Some(traversal::Event::Next { result, .. }) = self.events.as_slice().last() else {
            return Err(Error::Unconsumed);
        };
        Ok(match result {
            traversal::Next::Entry(row) => Read::Entry {
                name: &row.name,
                kind: row.inspected.ok_or(Error::Unconsumed)?,
            },
            traversal::Next::Eof => Read::Eof,
            traversal::Next::ReadError(code) => Read::Error {
                code: *code,
                opening: false,
            },
            traversal::Next::OpenError(code) => Read::Error {
                code: *code,
                opening: true,
            },
        })
    }

    pub(in super::super) fn close(
        &mut self,
        directory: Directory,
        reason: traversal::Close,
    ) -> Result<()> {
        self.enter()?;
        let index = self.associated(directory.scope, directory.invocation, directory.id)?;
        let Native::Directory { read, eof, .. } = &self.active.as_slice()[index].native else {
            return Err(Error::ObservationType);
        };
        let allowed = match reason {
            traversal::Close::Pruned => !read,
            traversal::Close::Drained | traversal::Close::Exhausted => *eof,
        };
        if !allowed {
            return Err(Error::Unconsumed);
        }
        self.events.prepare(self.owner.budget)?;
        // Drop only the source-requested owner; no remainder is enumerated.
        drop(self.active.remove(index));
        self.events.push(traversal::Event::Close {
            id: directory.id,
            reason,
        })?;
        self.refused = false;
        Ok(())
    }

    pub(in super::super) fn handle(
        &mut self,
        path: &[u8],
    ) -> Result<std::result::Result<Identity, i32>> {
        self.enter()?;
        let path = observed::Path::from_bytes(path, self.owner.budget)?;
        self.events.prepare(self.owner.budget)?;
        self.active.prepare(self.owner.budget)?;
        let id = self.ordinal()?;
        let terminated = terminated(path.bytes(), self.owner.budget)?;
        let name = CStr::from_bytes_with_nul(&terminated)
            .map_err(|_| Error::Encoding("invalid loop handle path"))?;
        // std::fs::OpenOptions uses cvt_r: reserve one eventual close, then
        // prospectively charge every open attempt, including EINTR retries.
        self.owner.budget.graph.paths(1)?;
        let opened = loop {
            self.owner.budget.graph.paths(1)?;
            match <native::System as file::System>::open(&mut native::System, name) {
                Err(code) if code == nix::libc::EINTR => continue,
                result => break result,
            }
        };
        let (result, opened) = match opened {
            Err(code) => (traversal::Handle::OpenFailed(code), Err(code)),
            Ok(descriptor) => {
                let descriptor = Descriptor(descriptor);
                self.owner.budget.graph.paths(1)?;
                match <native::System as file::System>::fstat(&mut native::System, descriptor.0)? {
                    Stat::Error(code) => (traversal::Handle::StatFailed(code), Err(code)),
                    Stat::Metadata(metadata) => {
                        let device = metadata.device();
                        let inode = metadata.inode();
                        self.active.push(Active {
                            id,
                            native: Native::Handle {
                                _descriptor: descriptor,
                                device,
                                inode,
                            },
                        })?;
                        (
                            traversal::Handle::Opened { device, inode },
                            Ok(Identity {
                                scope: self.owner.scope,
                                invocation: self.invocation,
                                id,
                            }),
                        )
                    }
                }
            }
        };
        self.events
            .push(traversal::Event::Handle { path, result })?;
        self.refused = false;
        Ok(opened)
    }

    pub(in super::super) fn same(&mut self, left: &Identity, right: &Identity) -> Result<bool> {
        self.enter()?;
        let left = self.associated(left.scope, left.invocation, left.id)?;
        let right = self.associated(right.scope, right.invocation, right.id)?;
        self.owner.budget.allocation.items(1)?;
        let Native::Handle {
            device: a,
            inode: b,
            ..
        } = self.active.as_slice()[left].native
        else {
            return Err(Error::ObservationType);
        };
        let Native::Handle {
            device: c,
            inode: d,
            ..
        } = self.active.as_slice()[right].native
        else {
            return Err(Error::ObservationType);
        };
        self.refused = false;
        Ok((a, b) == (c, d))
    }

    pub(in super::super) fn release(&mut self, identity: Identity) -> Result<()> {
        self.enter()?;
        let index = self.associated(identity.scope, identity.invocation, identity.id)?;
        if !matches!(self.active.as_slice()[index].native, Native::Handle { .. }) {
            return Err(Error::ObservationType);
        }
        self.events.prepare(self.owner.budget)?;
        drop(self.active.remove(index));
        self.events
            .push(traversal::Event::Release { id: identity.id })?;
        self.refused = false;
        Ok(())
    }

    pub(in super::super) fn finish(mut self) -> Result<observed::Record> {
        self.enter()?;
        if !self.active.as_slice().is_empty() {
            return Err(Error::Unconsumed);
        }
        let record = observed::Record {
            query: observed::Query::Traversal {
                root: self.root,
                is_root: self.is_root,
                invocation: self.invocation,
            },
            value: observed::Value::Traversal(traversal::Transcript {
                events: self.events.into_values(),
            }),
        }
        .checked()?;
        self.owner.pending = false;
        Ok(record)
    }
}

fn terminated(path: &[u8], budget: &mut Budget<'_>) -> Result<Vec<u8>> {
    let length = path.len().checked_add(1).ok_or(Error::Unconsumed)?;
    budget.allocation.items(1)?;
    budget.allocation.bytes(length)?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length)?;
    bytes.extend_from_slice(path);
    bytes.push(0);
    Ok(bytes)
}
