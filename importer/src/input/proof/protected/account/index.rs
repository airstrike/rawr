use std::collections::BTreeMap;

use super::{Account, Current, Legacy};
use crate::input::dependency::observed::index;
use crate::input::proof::ExecutionJoin;
use crate::input::stored::observe::slots::Slots;

pub(in crate::input::proof::protected) trait Index<K: Ord, V: Copy, A: Account>:
    Sized
{
    fn new() -> Self;
    fn len(&self) -> usize;
    fn get(&self, key: &K) -> Option<ExecutionJoin<V>>;
    fn insert(&mut self, key: K, value: V, account: &mut A) -> Result<(), A::Error>;
}

pub(in crate::input::proof::protected) struct LegacyIndex<K, V>(BTreeMap<K, ExecutionJoin<V>>);

impl<K: Ord, V: Copy> Index<K, V, Legacy<'_>> for LegacyIndex<K, V> {
    fn new() -> Self {
        Self(BTreeMap::new())
    }
    fn len(&self) -> usize {
        self.0.len()
    }
    fn get(&self, key: &K) -> Option<ExecutionJoin<V>> {
        self.0.get(key).copied()
    }
    fn insert(&mut self, key: K, value: V, _account: &mut Legacy<'_>) -> crate::Result<()> {
        match self.0.entry(key) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(ExecutionJoin::One(value));
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                entry.insert(ExecutionJoin::Ambiguous);
            }
        }
        Ok(())
    }
}

pub(in crate::input::proof::protected) struct CurrentIndex<K, V> {
    values: Slots<(K, ExecutionJoin<V>)>,
    index: index::Index,
}

impl<K: Ord, V: Copy> Index<K, V, Current<'_, '_>> for CurrentIndex<K, V> {
    fn new() -> Self {
        Self {
            values: Slots::new(),
            index: index::Index::new(),
        }
    }
    fn len(&self) -> usize {
        self.values.as_slice().len()
    }
    fn get(&self, key: &K) -> Option<ExecutionJoin<V>> {
        match self
            .index
            .locate(self.values.as_slice(), |(candidate, _)| candidate.cmp(key))
        {
            Err(index) => Some(self.values.as_slice()[index].1),
            Ok(_) => None,
        }
    }
    fn insert(
        &mut self,
        key: K,
        value: V,
        account: &mut Current<'_, '_>,
    ) -> crate::input::stored::Result<()> {
        account.0.check()?;
        match self
            .index
            .locate(self.values.as_slice(), |(candidate, _)| candidate.cmp(&key))
        {
            Err(index) => self.values.as_mut_slice()[index].1 = ExecutionJoin::Ambiguous,
            Ok(vacant) => {
                self.values
                    .push((key, ExecutionJoin::One(value)), account.0)?;
                self.index.insert(vacant, account.0)?;
            }
        }
        Ok(())
    }
}
