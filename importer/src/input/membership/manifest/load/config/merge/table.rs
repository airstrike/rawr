//! Closed pinned standard-map operations; no current configuration authority.

use std::collections::{hash_map, HashMap};
use std::{hash::Hash, os::unix::ffi::OsStrExt, path::Path};

use rawr_toml::document;

use super::super::super::super::{session::Session, Error, Step};
use super::fx;

mod layout;

/// Borrowed keys and inert flat-node indices prevent recursive destruction.
/// Physical bucket extent survives removals; capacity() includes growth_left.
pub(in super::super::super) type Table<'d> = Map<'d, str>;
pub(in super::super::super) type PathTable<'d> = Map<'d, Path>;
mod sealed {
    pub trait Key {}
    impl Key for str {}
    impl Key for std::path::Path {}
}
pub(in super::super::super) trait Key: sealed::Key + Eq + Hash {
    fn bytes(&self) -> usize;
}
impl Key for str {
    fn bytes(&self) -> usize {
        self.len()
    }
}
impl Key for Path {
    fn bytes(&self) -> usize {
        self.as_os_str().as_bytes().len()
    }
}
// Both permitted borrowed-key entries use the already audited physical layout.
const _: () = {
    assert!(std::mem::size_of::<(&Path, usize)>() == std::mem::size_of::<(&str, usize)>());
    assert!(std::mem::align_of::<(&Path, usize)>() == std::mem::align_of::<(&str, usize)>());
};
pub(in super::super::super) struct Map<'d, K: Key + ?Sized> {
    entries: HashMap<&'d K, usize, fx::Build>,
    buckets: usize,
}

impl<'d, K: Key + ?Sized> Map<'d, K> {
    pub(in super::super::super) fn new() -> Self {
        Self {
            entries: HashMap::with_hasher(fx::Build),
            buckets: 0,
        }
    }

    /// Preserve physical placement, deleted controls and remaining growth.
    /// The pinned same-bucket clone_from cannot allocate for these inert types.
    pub(super) fn try_copy<A: document::Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.items(1)?;
        let mut copy = Self::new();
        if self.buckets != 0 {
            let full = layout::capacity(self.buckets);
            let Some((buckets, allocation)) = layout::growth(full) else {
                return session.fail(Error::Overflow);
            };
            if buckets != self.buckets {
                return session.fail(Error::State);
            }
            session.admit(document::Request::Allocation {
                bytes: allocation.size(),
                alignment: allocation.align(),
            })?;
            if let Err(error) = copy.entries.try_reserve(full) {
                return session.fail(Error::Allocation(error));
            }
            // A fresh map has no deleted controls: its usable capacity is its
            // full physical capacity, which uniquely identifies bucket extent.
            if copy.entries.capacity() != full {
                return session.fail(Error::State);
            }
        }
        // Do not skip allocated-empty sources: their controls/growth survive.
        // A zero-bucket source takes the pinned empty-singleton branch instead.
        copy.entries.clone_from(&self.entries);
        copy.buckets = self.buckets;
        Ok(copy)
    }

    /// Result collection has a zero lower size hint. Each conversion insert
    /// reserves before searching, even when it will replace an existing key.
    pub(in super::super::super) fn insert<A: document::Admission>(
        &mut self,
        key: &'d K,
        value: usize,
        session: &mut Session<'_, A>,
    ) -> Step<Option<usize>> {
        session.items(1)?;
        session.string(key.bytes())?;
        self.reserve_one(session)?;
        Ok(self.entries.insert(key, value))
    }

    /// std's rustc_entry searches before reserving. Preserve an occupied node
    /// for the caller's recursive merge; only vacant entries retain the new node.
    pub(in super::super::super) fn merge<A: document::Admission>(
        &mut self,
        key: &'d K,
        value: usize,
        session: &mut Session<'_, A>,
    ) -> Step<Option<usize>> {
        session.items(1)?;
        session.string(key.bytes())?;
        if let Some(value) = self.entries.get(key) {
            return Ok(Some(*value));
        }
        self.reserve_one(session)?;
        // The extra closed lookup avoids std Entry's infallible reservation.
        // After reserve_one there is guaranteed space, including tombstones.
        self.entries.entry(key).or_insert(value);
        Ok(None)
    }

    pub(in super::super::super) fn get<A: document::Admission>(
        &self,
        key: &K,
        session: &mut Session<'_, A>,
    ) -> Step<Option<usize>> {
        session.items(1)?;
        session.string(key.bytes())?;
        Ok(self.entries.get(key).copied())
    }

    pub(super) fn remove<A: document::Admission>(
        &mut self,
        key: &K,
        session: &mut Session<'_, A>,
    ) -> Step<Option<usize>> {
        session.items(1)?;
        session.string(key.bytes())?;
        Ok(self.entries.remove(key))
    }

    /// Replace an existing inert index without altering keys or placement.
    pub(in super::super::super) fn replace<A: document::Admission>(
        &mut self,
        key: &K,
        value: usize,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        session.string(key.bytes())?;
        let Some(index) = self.entries.get_mut(key) else {
            return session.fail(Error::State);
        };
        *index = value;
        Ok(())
    }

    pub(in super::super::super) fn iter<A: document::Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<Iter<'_, 'd, K>> {
        session.check()?;
        Ok(Iter {
            entries: self.entries.iter(),
        })
    }

    /// Joining flat value arenas changes only inert indices, never keys or
    /// table placement. No hashing, reservation, clone or recursive drop occurs.
    pub(super) fn rebase<A: document::Admission>(
        &mut self,
        offset: usize,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.check()?;
        for index in self.entries.values_mut() {
            session.items(1)?;
            let Some(next) = index.checked_add(offset) else {
                return session.fail(Error::Overflow);
            };
            *index = next;
        }
        Ok(())
    }

    pub(in super::super::super) fn into_iter<A: document::Admission>(
        self,
        session: &mut Session<'_, A>,
    ) -> Step<IntoIter<'d, K>> {
        session.check()?;
        Ok(IntoIter {
            entries: self.entries.into_iter(),
        })
    }

    fn reserve_one<A: document::Admission>(&mut self, session: &mut Session<'_, A>) -> Step<()> {
        session.check()?;
        if self.entries.len() < self.entries.capacity() {
            return Ok(());
        }
        let Some(required) = self.entries.len().checked_add(1) else {
            return session.fail(Error::Overflow);
        };
        let full = layout::capacity(self.buckets);
        let next = if required <= full / 2 {
            // The pinned implementation rehashes tombstones in place.
            self.buckets
        } else {
            let Some((buckets, allocation)) = layout::growth(required.max(full + 1)) else {
                return session.fail(Error::Overflow);
            };
            session.admit(document::Request::Allocation {
                bytes: allocation.size(),
                alignment: allocation.align(),
            })?;
            buckets
        };
        if let Err(error) = self.entries.try_reserve(1) {
            return session.fail(Error::Allocation(error));
        }
        self.buckets = next;
        if self.entries.capacity() != layout::capacity(next) {
            return session.fail(Error::State);
        }
        Ok(())
    }
}

pub(in super::super::super) struct IntoIter<'d, K: Key + ?Sized = str> {
    entries: hash_map::IntoIter<&'d K, usize>,
}

impl<'d, K: Key + ?Sized> IntoIter<'d, K> {
    pub(in super::super::super) fn next<A: document::Admission>(
        &mut self,
        session: &mut Session<'_, A>,
    ) -> Step<Option<(&'d K, usize)>> {
        session.items(1)?;
        Ok(self.entries.next())
    }
}

pub(in super::super::super) struct Iter<'t, 'd, K: Key + ?Sized = str> {
    entries: hash_map::Iter<'t, &'d K, usize>,
}

impl<'d, K: Key + ?Sized> Iter<'_, 'd, K> {
    pub(in super::super::super) fn next<A: document::Admission>(
        &mut self,
        session: &mut Session<'_, A>,
    ) -> Step<Option<(&'d K, usize)>> {
        session.items(1)?;
        Ok(self.entries.next().map(|(&key, &value)| (key, value)))
    }
}

#[cfg(test)]
mod tests;
