// Copyright the Rust Project Developers and hashbrown contributors. MIT OR Apache-2.0.
//! Four concrete owned-map domains, fixed pinned SipHasher13 keys.

use super::{storage, Error, Id, Name, Session, Step};
use rawr_toml::document::Admission;
use std::{
    alloc::Layout,
    borrow::Cow,
    collections::HashMap,
    hash::{BuildHasherDefault, DefaultHasher, Hash},
};

pub(super) trait Key: Eq + Hash {}
impl Key for Id {}
impl Key for Name<'_> {}
impl Key for Cow<'_, [u8]> {}

#[derive(Debug)]
pub(super) struct Map<K, V> {
    entries: HashMap<K, V, BuildHasherDefault<DefaultHasher>>,
    buckets: usize,
}
impl<K: Key, V> Default for Map<K, V> {
    fn default() -> Self {
        Self::new()
    }
}
impl<K: Key, V> Map<K, V> {
    pub(super) fn new() -> Self {
        Self {
            entries: HashMap::with_hasher(BuildHasherDefault::default()),
            buckets: 0,
        }
    }
    // One actual closed map operation is one logical event. The independent
    // finite proof bounds probe sequences by physical buckets, equality by
    // admitted key lengths and rehash by live entries times target buckets;
    // those upper bounds are not fictitious executed byte/item debits.
    pub(super) fn get<A: Admission>(
        &self,
        key: &K,
        session: &mut Session<'_, A>,
    ) -> Step<Option<&V>> {
        session.items(1)?;
        Ok(self.entries.get(key))
    }
    pub(super) fn remove<A: Admission>(
        &mut self,
        key: &K,
        session: &mut Session<'_, A>,
    ) -> Step<Option<V>> {
        session.items(1)?;
        Ok(self.entries.remove(key))
    }
    pub(super) fn insert<A: Admission>(
        &mut self,
        key: K,
        value: V,
        session: &mut Session<'_, A>,
    ) -> Step<Option<V>> {
        // std's insert reserves before find_or_find_insert_slot, even on an
        // occupied key. Keep that distinct from entry's search-first branch.
        self.reserve(1, session)?;
        session.items(1)?;
        Ok(self.entries.insert(key, value))
    }
    pub(super) fn entry<A: Admission>(
        &mut self,
        key: K,
        session: &mut Session<'_, A>,
    ) -> Step<&mut V>
    where
        V: Default,
    {
        session.items(1)?;
        if !self.entries.contains_key(&key) {
            self.reserve(1, session)?;
        }
        session.items(1)?;
        // Actual Entry retains the original equal stored key and drops the
        // supplied owned clone before the caller mutates the occupied value.
        Ok(self.entries.entry(key).or_default())
    }
    pub(super) fn reserve<A: Admission>(
        &mut self,
        additional: usize,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        let Some(required) = self.entries.len().checked_add(additional) else {
            return session.fail(Error::Overflow);
        };
        if required <= self.entries.capacity() {
            return Ok(());
        }
        let full = capacity(self.buckets);
        let next = if required <= full / 2 {
            self.buckets
        } else {
            let Some((buckets, layout)) = growth::<K, V>(required.max(full + 1)) else {
                return session.fail(Error::Overflow);
            };
            storage::allocation(layout, session)?;
            buckets
        };
        if let Err(error) = self.entries.try_reserve(additional) {
            return session.fail(Error::Allocation(error));
        }
        if self.entries.capacity() != capacity(next) {
            return session.fail(Error::State);
        }
        self.buckets = next;
        Ok(())
    }
}

fn group() -> usize {
    if cfg!(all(
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
    }
}
fn capacity(buckets: usize) -> usize {
    if buckets == 0 {
        0
    } else if buckets <= 8 {
        buckets - 1
    } else {
        buckets / 8 * 7
    }
}
fn growth<K, V>(required: usize) -> Option<(usize, Layout)> {
    // Every admitted domain has entries >= 12 bytes; no tiny-entry branch.
    if std::mem::size_of::<(K, V)>() < 12 {
        return None;
    }
    let buckets = if required < 4 {
        4
    } else if required < 8 {
        8
    } else if required < 15 {
        16
    } else {
        (required.checked_mul(8)? / 7).checked_next_power_of_two()?
    };
    let entry = Layout::new::<(K, V)>();
    let align = entry.align().max(group());
    let offset = entry.size().checked_mul(buckets)?.checked_add(align - 1)? & !(align - 1);
    let layout =
        Layout::from_size_align(offset.checked_add(buckets)?.checked_add(group())?, align).ok()?;
    Some((buckets, layout))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::Budget;
    use crate::{cargo, input, node};
    use std::hash::Hasher;

    #[test]
    fn fixed_collisions_occupied_entry_and_removal_keep_table_history() {
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::new(&limits);
        let mut budget = Budget {
            graph: &graph,
            allocation: &mut allocation,
        };
        let mut session = Session::new(&mut budget);
        let mut names = vec![];
        for index in 0..128 {
            let bytes = format!("k{index}").into_bytes();
            let name = Name(Cow::Owned(bytes));
            let mut hash = DefaultHasher::new();
            name.hash(&mut hash);
            if hash.finish() & 3 == 0 {
                names.push(name);
                if names.len() == 3 {
                    break;
                }
            }
        }
        assert_eq!(names.len(), 3);
        let mut table = Map::<Name<'static>, usize>::new();
        for (index, name) in names.iter().enumerate() {
            table
                .insert(
                    Name(Cow::Owned(
                        super::super::own(&name.0, &mut session).unwrap(),
                    )),
                    index,
                    &mut session,
                )
                .unwrap();
        }
        assert_eq!(table.buckets, 4);
        let duplicate = Name(Cow::Owned(
            names[0].0.iter().map(u8::to_ascii_uppercase).collect(),
        ));
        assert_eq!(*table.entry(duplicate, &mut session).unwrap(), 0);
        assert_eq!(table.buckets, 4);
        assert_eq!(table.remove(&names[1], &mut session).unwrap(), Some(1));
        let buckets = table.buckets;
        let key = Name(Cow::Owned(
            super::super::own(&names[1].0, &mut session).unwrap(),
        ));
        table.insert(key, 7, &mut session).unwrap();
        assert!(table.buckets >= buckets);
        assert_eq!(table.get(&names[1], &mut session).unwrap(), Some(&7));
        assert!(session.items(usize::MAX).is_err());
        assert!(table.get(&names[0], &mut session).is_err());
        assert!(session.finish(Ok(())).is_err());
    }
}
