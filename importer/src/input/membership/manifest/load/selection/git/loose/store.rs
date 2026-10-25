// Copyright the gitoxide contributors. MIT OR Apache-2.0.
//! Default loose-only Store ownership and the fixed find_loose("HEAD") search.

use std::{
    alloc::Layout,
    cell::UnsafeCell,
    path,
    rc::Rc,
    sync::{atomic::AtomicUsize, Arc},
};

use rawr_toml::document;

use super::{name, read, Candidate, Name, Reference};
use crate::input::membership::{
    host,
    manifest::{load, session::Session, Error, Step},
};

/// The exact constructor-only packed state: an unlocked word and empty snapshot.
/// No locking, packed-file lookup, shared clone or inner allocation is reachable.
struct Empty {
    _state: AtomicUsize,
    _snapshot: UnsafeCell<Option<Arc<()>>>,
}

/// This private owner exists only after its path and actual Arc were admitted.
pub(in super::super) struct Store {
    directory: path::PathBuf,
    // gix's Arc, never shared across threads here; an Rc has the same layout.
    _packed: Rc<Empty>,
}

#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub(in super::super) enum Failure {
    #[error("loose reference search failed: {0}")]
    Find(read::Failure),
    #[error("reference {name:?} was not found")]
    NotFound { name: path::PathBuf },
}

impl Store {
    pub(in super::super) fn at<A: document::Admission>(
        directory: &path::Path,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        // gix-discover copies dot_git before Store::at evaluates the packed Arc.
        let directory = load::path::copy(directory, session)?;
        let empty = Empty {
            _state: AtomicUsize::new(0),
            _snapshot: UnsafeCell::new(None),
        };
        let Ok((layout, _)) = Layout::new::<[AtomicUsize; 2]>().extend(Layout::new::<Empty>())
        else {
            return session.fail(Error::Overflow);
        };
        let layout = layout.pad_to_align();
        session.admit(document::Request::Allocation {
            bytes: layout.size(),
            alignment: layout.align(),
        })?;
        let packed = Rc::new(empty);
        session.items(1)?;
        Ok(Self {
            directory,
            _packed: packed,
        })
    }

    pub(in super::super) fn head(
        &self,
        session: &mut Session<'_, host::Host<'_, '_>>,
    ) -> Step<Result<Reference, Failure>> {
        // find_existing_inner validates its fixed partial name before creating
        // the reusable BString in find_one_with_verified_input.
        if name::partial(b"HEAD", session)?.is_err() {
            return session.fail(Error::State);
        }
        match self.find(session)? {
            Ok(Some(reference)) => Ok(Ok(reference)),
            Err(error) => {
                session.items(1)?;
                Ok(Err(Failure::Find(error)))
            }
            Ok(None) => {
                // The candidate BString has already retired with find().
                let name = load::path::copy(path::Path::new("HEAD"), session)?;
                session.items(1)?;
                Ok(Err(Failure::NotFound { name }))
            }
        }
    }

    fn find(
        &self,
        session: &mut Session<'_, host::Host<'_, '_>>,
    ) -> Step<Result<Option<Reference>, read::Failure>> {
        let mut names = Buffer {
            bytes: Vec::new(),
            admitted: 0,
        };
        session.items(1)?;
        // The true pseudo-ref pass tries HEAD only; the false pass tries the
        // four refs/ names. HEAD suppresses the extra remotes/<name>/HEAD tail.
        for candidate in Candidate::ALL {
            let name = names.select(candidate, session)?;
            match read::one(&self.directory, name, session)? {
                Ok(None) => {}
                outcome => return Ok(outcome),
            }
        }
        Ok(Ok(None))
    }
}

/// BString is a Vec<u8>; clear retains capacity across all five attempts.
struct Buffer {
    bytes: Vec<u8>,
    admitted: usize,
}
impl Buffer {
    fn select<'buffer, A: document::Admission>(
        &'buffer mut self,
        candidate: Candidate,
        session: &mut Session<'_, A>,
    ) -> Step<Name<'buffer>> {
        session.items(1)?;
        self.bytes.clear();
        if candidate != Candidate::Head {
            self.extend(b"refs/", session)?;
        }
        let middle = match candidate {
            Candidate::Head | Candidate::Ref => b"".as_slice(),
            Candidate::Tag => b"tags",
            Candidate::Branch => b"heads",
            Candidate::Remote => b"remotes",
        };
        if !middle.is_empty() {
            self.extend(middle, session)?;
            self.extend(b"/", session)?;
        }
        self.extend(b"HEAD", session)?;
        Ok(Name {
            candidate,
            bytes: &self.bytes,
        })
    }

    fn extend<A: document::Admission>(
        &mut self,
        bytes: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        let Some(required) = self.bytes.len().checked_add(bytes.len()) else {
            return session.fail(Error::Overflow);
        };
        if required > self.admitted {
            let Some(doubled) = self.admitted.checked_mul(2) else {
                return session.fail(Error::Overflow);
            };
            let capacity = required.max(doubled).max(8);
            let Ok(layout) = Layout::array::<u8>(capacity) else {
                return session.fail(Error::Overflow);
            };
            session.admit(document::Request::Allocation {
                bytes: layout.size(),
                alignment: layout.align(),
            })?;
            let mut replacement = Vec::new();
            if let Err(error) = replacement.try_reserve_exact(capacity) {
                return session.fail(Error::Allocation(error));
            }
            replacement.extend_from_slice(&self.bytes);
            self.bytes = replacement;
            self.admitted = capacity;
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
