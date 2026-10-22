//! Closed binary StateMap with fixed pinned SipHasher13 initialization.
//! See intern/PROVENANCE.md for the source, equality, work and layout closure.
//! Keys only hash/compare immutable bytes; values and destruction are flat.
//! There is no removal or iteration, so bucket placement cannot reorder
//! determinization. Source len-based size limits do not use bucket ownership.
use std::{
    alloc::Layout,
    collections::HashMap,
    hash::{BuildHasherDefault, DefaultHasher},
};

use rawr_toml::document::Request;

use super::{key, Admission, Error, Session, Step};

#[cfg(test)]
mod tests;

pub(super) struct Interner {
    entries: HashMap<key::State, u32, BuildHasherDefault<DefaultHasher>>,
}

impl Interner {
    pub(super) fn new() -> Self {
        Self {
            entries: HashMap::with_hasher(BuildHasherDefault::default()),
        }
    }

    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(super) fn get<A: Admission>(
        &self,
        bytes: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<Option<u32>> {
        session.items(1)?;
        Ok(self.entries.get(bytes).copied())
    }

    pub(super) fn insert<A: Admission>(
        &mut self,
        state: key::State,
        id: u32,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        // Ordinary std insert reserves before searching. This closed domain
        // never removes keys, and clear is only reached after the final insert.
        if self.entries.len() == self.entries.capacity() {
            let Some(required) = self.entries.len().checked_add(1) else {
                return session.fail(Error::Overflow);
            };
            let Some((buckets, layout)) = growth(required) else {
                return session.fail(Error::Overflow);
            };
            session.admit(Request::Allocation {
                bytes: layout.size(),
                alignment: layout.align(),
            })?;
            if let Err(error) = self.entries.try_reserve(1) {
                return session.fail(Error::Allocation(error));
            }
            if self.entries.capacity() != capacity(buckets) {
                return session.fail(Error::State);
            }
        }
        if self.entries.insert(state, id).is_some() {
            return session.fail(Error::State);
        }
        Ok(())
    }

    pub(super) fn clear<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Step<()> {
        session.items(self.entries.len())?;
        self.entries.clear();
        Ok(())
    }
}

// Copyright the hashbrown contributors. MIT OR Apache-2.0.
// Licenses: importer/src/input/membership/manifest/load/config/merge/table/
// LICENSE-MIT and LICENSE-APACHE. Pinned hashbrown 0.17.1/std Global layout,
// specialized from that adapter to this closed (Arc<[u8]>, StateID) entry.
pub(in super::super) fn growth(required: usize) -> Option<(usize, Layout)> {
    let buckets = if required < 4 {
        4
    } else if required < 8 {
        8
    } else if required < 15 {
        16
    } else {
        (required.checked_mul(8)? / 7).checked_next_power_of_two()?
    };
    let entry = Layout::new::<(key::State, u32)>();
    // State is a fat Arc, so this entry is >= 12 bytes on every supported
    // pointer width and takes none of hashbrown's tiny-item branches.
    let group = if cfg!(all(
        target_feature = "sse2",
        any(target_arch = "x86", target_arch = "x86_64"),
        not(miri)
    )) || cfg!(all(
        target_arch = "loongarch64",
        target_feature = "lsx",
        not(miri)
    )) {
        16
    } else if cfg!(any(
        target_pointer_width = "64",
        target_arch = "aarch64",
        target_arch = "x86_64",
        target_arch = "wasm32"
    )) {
        8
    } else {
        4
    };
    let alignment = entry.align().max(group);
    let offset = entry
        .size()
        .checked_mul(buckets)?
        .checked_add(alignment - 1)?
        & !(alignment - 1);
    let bytes = offset.checked_add(buckets)?.checked_add(group)?;
    Some((buckets, Layout::from_size_align(bytes, alignment).ok()?))
}

pub(in super::super) fn capacity(buckets: usize) -> usize {
    if buckets <= 8 {
        buckets - 1
    } else {
        buckets / 8 * 7
    }
}
