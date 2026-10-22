// Copyright (c) 2015 Andrew Gallant. MIT.
// See walk/PROVENANCE.md and walk/LICENSE-MIT.
//! WalkDir 2.5.0's Unix continuation for Cargo's unsorted, link-following walk.
use std::{
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use super::super::path;
use crate::input::membership::{
    host::{
        self,
        traversal::{Cursor, Directory, Read},
    },
    manifest::{session::Session, storage::Buffer, Error, Step},
    observed::{self, traversal::Close},
};

/// A materialized WalkDir entry; no native row or account borrow survives.
pub(super) struct Entry {
    pub(super) path: PathBuf,
    pub(super) kind: observed::Kind,
    pub(super) depth: usize,
    pub(super) followed: bool,
}

/// Native source errors remain distinct from an original Session refusal.
pub(super) enum Failure {
    Io { path: Option<PathBuf>, errno: i32 },
    Loop { ancestor: PathBuf, child: PathBuf },
}

/// IntoIter's two stacks retain their separate source allocation/drop order.
pub(super) struct Walk<'image> {
    start: Option<PathBuf>,
    lists: Buffer<List>,
    ancestors: Buffer<PathBuf>,
    oldest: usize,
    depth: usize,
    traversal: Cursor<'image>,
}

enum List {
    Opened {
        depth: usize,
        directory: Option<Directory>,
        failure: Option<Failure>,
        ended: bool,
    },
    Closed(std::vec::IntoIter<Result<Entry, Failure>>),
}

impl<'image> Walk<'image> {
    pub(super) fn new(
        root: &Path,
        is_root: bool,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Self> {
        let start = Some(path::copy(root, session)?);
        let traversal = session.walk_begin(root.as_os_str().as_bytes(), is_root)?;
        Ok(Self {
            start,
            lists: Buffer::new(),
            ancestors: Buffer::new(),
            oldest: 0,
            depth: 0,
            traversal,
        })
    }

    /// Source IntoIter::next, before FilterEntry applies Cargo's predicate.
    pub(super) fn next(
        &mut self,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Option<Result<Entry, Failure>>> {
        session.check()?;
        if let Some(start) = self.start.take() {
            return match from_path(0, start, false, session)? {
                Ok(entry) => self.handle_entry(entry, session).map(Some),
                Err(failure) => Ok(Some(Err(failure))),
            };
        }
        while self.lists.len() != 0 {
            session.items(1)?;
            self.depth = self.lists.len();
            let index = self.lists.len() - 1;
            let next = list_next(
                &mut self.lists.as_mut_slice()[index],
                &self.ancestors.as_slice()[index],
                &mut self.traversal,
                session,
            )?;
            match next {
                None => self.pop(Close::Exhausted, session)?,
                Some(Err(failure)) => return Ok(Some(Err(failure))),
                Some(Ok(entry)) => return self.handle_entry(entry, session).map(Some),
            }
        }
        Ok(None)
    }

    /// Only the containing source predicate may request pruning.
    pub(super) fn skip_current_dir(
        &mut self,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<()> {
        session.check()?;
        if self.lists.len() != 0 {
            self.pop(Close::Pruned, session)?;
        }
        Ok(())
    }

    pub(super) fn finish(self, session: &mut Session<'_, host::Host<'image, '_>>) -> Step<()> {
        session.check()?;
        if self.start.is_some() || self.lists.len() != 0 || self.ancestors.len() != 0 {
            return session.fail(Error::State);
        }
        session.walk_finish(self.traversal)
    }

    fn handle_entry(
        &mut self,
        mut entry: Entry,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Result<Entry, Failure>> {
        session.items(1)?;
        if entry.kind == observed::Kind::Symlink {
            entry = match from_path(self.depth, path::copy(&entry.path, session)?, true, session)? {
                Ok(entry) => entry,
                Err(failure) => return Ok(Err(failure)),
            };
            if entry.kind == observed::Kind::Directory
                && let Err(failure) = self.check_loop(&entry.path, session)?
            {
                return Ok(Err(failure));
            }
        }
        if entry.kind == observed::Kind::Directory {
            self.push(&entry, session)?;
        } else if entry.depth == 0 && entry.kind == observed::Kind::Symlink {
            // Source follow_root_links branch. A normal followed Unix stat
            // does not report a symlink, but retain its exact continuation.
            match session.metadata(entry.path.as_os_str().as_bytes(), true)? {
                Ok(metadata) if metadata.kind == observed::Kind::Directory => {
                    self.push(&entry, session)?
                }
                Ok(_) => {}
                Err(errno) => {
                    return Ok(Err(Failure::Io {
                        path: Some(path::copy(&entry.path, session)?),
                        errno,
                    }))
                }
            }
        }
        Ok(Ok(entry))
    }

    fn push(
        &mut self,
        entry: &Entry,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<()> {
        let Some(free) = self.lists.len().checked_sub(self.oldest) else {
            return session.fail(Error::State);
        };
        if free == 10 {
            self.drain(self.oldest, session)?;
        }
        let directory =
            session.walk_open(&mut self.traversal, entry.path.as_os_str().as_bytes())?;
        // ReadDir's native error is owned now, although its delivery occurs
        // later and filter_entry may discard it without ever requesting it.
        let failure = match directory.opening_error() {
            Some(errno) => Some(Failure::Io {
                path: Some(path::copy(&entry.path, session)?),
                errno,
            }),
            None => None,
        };
        let list = List::Opened {
            depth: self.depth,
            directory: Some(directory),
            failure,
            ended: false,
        };
        // Unix Ancestor::new copies the path; it opens no native handle.
        let ancestor = path::copy(&entry.path, session)?;
        self.ancestors.push(ancestor, session)?;
        self.lists.push(list, session)?;
        if free == 10 {
            self.oldest += 1;
        }
        Ok(())
    }

    fn drain(
        &mut self,
        index: usize,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<()> {
        if matches!(&self.lists.as_slice()[index], List::Closed(_)) {
            return Ok(());
        }
        let mut entries = Buffer::new();
        while let Some(entry) = list_next(
            &mut self.lists.as_mut_slice()[index],
            &self.ancestors.as_slice()[index],
            &mut self.traversal,
            session,
        )? {
            entries.push(entry, session)?;
        }
        let List::Opened { directory, .. } = &mut self.lists.as_mut_slice()[index] else {
            return session.fail(Error::State);
        };
        let Some(directory) = directory.take() else {
            return session.fail(Error::State);
        };
        session.walk_close(&mut self.traversal, directory, Close::Drained)?;
        self.lists.as_mut_slice()[index] = List::Closed(entries.into_iter());
        Ok(())
    }

    fn pop(&mut self, close: Close, session: &mut Session<'_, host::Host<'image, '_>>) -> Step<()> {
        session.items(1)?;
        let Some(mut list) = self.lists.pop() else {
            return session.fail(Error::State);
        };
        if let List::Opened { directory, .. } = &mut list {
            let Some(directory) = directory.take() else {
                return session.fail(Error::State);
            };
            session.walk_close(&mut self.traversal, directory, close)?;
        }
        // Source drops the DirList (including unread buffered entries) before
        // dropping the ancestor path and adjusting oldest_opened.
        drop(list);
        self.ancestors.pop();
        self.oldest = self.oldest.min(self.lists.len());
        Ok(())
    }

    fn check_loop(
        &mut self,
        child: &Path,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Result<(), Failure>> {
        let child_handle =
            match session.walk_handle(&mut self.traversal, child.as_os_str().as_bytes())? {
                Ok(handle) => handle,
                Err(errno) => return Ok(Err(Failure::Io { path: None, errno })),
            };
        for ancestor in self.ancestors.as_slice().iter().rev() {
            session.items(1)?;
            let ancestor_handle =
                match session.walk_handle(&mut self.traversal, ancestor.as_os_str().as_bytes())? {
                    Ok(handle) => handle,
                    Err(errno) => {
                        session.walk_release(&mut self.traversal, child_handle)?;
                        return Ok(Err(Failure::Io { path: None, errno }));
                    }
                };
            let same = session.walk_same(&mut self.traversal, &child_handle, &ancestor_handle)?;
            session.walk_release(&mut self.traversal, ancestor_handle)?;
            if same {
                let failure = Failure::Loop {
                    ancestor: path::copy(ancestor, session)?,
                    child: path::copy(child, session)?,
                };
                session.walk_release(&mut self.traversal, child_handle)?;
                return Ok(Err(failure));
            }
        }
        session.walk_release(&mut self.traversal, child_handle)?;
        Ok(Ok(()))
    }
}

fn from_path(
    depth: usize,
    owned: PathBuf,
    followed: bool,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<Result<Entry, Failure>> {
    match session.metadata(owned.as_os_str().as_bytes(), followed)? {
        Ok(metadata) => Ok(Ok(Entry {
            path: owned,
            kind: metadata.kind,
            depth,
            followed,
        })),
        Err(errno) => Ok(Err(Failure::Io {
            path: Some(path::copy(&owned, session)?),
            errno,
        })),
    }
}

fn list_next<'image>(
    list: &mut List,
    parent: &Path,
    traversal: &mut Cursor<'image>,
    session: &mut Session<'_, host::Host<'image, '_>>,
) -> Step<Option<Result<Entry, Failure>>> {
    session.items(1)?;
    let List::Opened {
        depth,
        directory,
        failure,
        ended,
    } = list
    else {
        let List::Closed(entries) = list else {
            return session.fail(Error::State);
        };
        return Ok(entries.next());
    };
    if *ended {
        return Ok(None);
    }
    let Some(directory) = directory.as_mut() else {
        return session.fail(Error::State);
    };
    match session.walk_next(traversal, directory)? {
        Read::Entry { name, kind } => {
            if failure.is_some() {
                return session.fail(Error::State);
            }
            let entry_path = path::join(
                parent,
                Path::new(std::ffi::OsStr::from_bytes(name)),
                session,
            )?;
            Ok(Some(match kind {
                Ok(kind) => Ok(Entry {
                    path: entry_path,
                    kind,
                    depth: *depth + 1,
                    followed: false,
                }),
                Err(errno) => Err(Failure::Io {
                    path: Some(entry_path),
                    errno,
                }),
            }))
        }
        Read::Eof => {
            if failure.is_some() {
                return session.fail(Error::State);
            }
            *ended = true;
            Ok(None)
        }
        Read::Error { code, opening } => {
            // The next logical DirList::next consumes the transport's pure
            // post-error EOF; it does not issue another native read.
            if opening {
                match failure.take() {
                    Some(failure @ Failure::Io { errno, .. }) if errno == code => {
                        Ok(Some(Err(failure)))
                    }
                    _ => session.fail(Error::State),
                }
            } else if failure.is_none() {
                Ok(Some(Err(Failure::Io {
                    path: None,
                    errno: code,
                })))
            } else {
                session.fail(Error::State)
            }
        }
    }
}
