//! Finite typed maps keep first keys and last values in semantic key order.

use std::cmp::Ordering;

use rawr_toml::document;

use crate::input::membership::manifest::{session, storage, value, Step};

struct Entry<'d, K> {
    typed: K,
    key: value::Key<'d>,
    node: usize,
    ordinal: usize,
}

pub(super) struct Map<'d, K> {
    entries: storage::Buffer<Entry<'d, K>>,
}

impl<'d, K> Map<'d, K> {
    pub(super) fn new() -> Self {
        Self {
            entries: storage::Buffer::new(),
        }
    }

    pub(super) fn push<A: document::Admission>(
        &mut self,
        typed: K,
        key: value::Key<'d>,
        node: usize,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        let ordinal = self.entries.len();
        self.entries.push(
            Entry {
                typed,
                key,
                node,
                ordinal,
            },
            session,
        )?;
        Ok(())
    }

    pub(super) fn finish<A: document::Admission>(
        &mut self,
        compare: impl Fn(&K, &K) -> Ordering,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        session.check()?;
        storage::sort(self.entries.as_mut_slice(), |a, b| {
            compare(&a.typed, &b.typed).then(a.ordinal.cmp(&b.ordinal))
        });
        let mut next = 0;
        let mut kept = 0;
        while next < self.entries.len() {
            let first = next;
            next += 1;
            while next < self.entries.len()
                && compare(
                    &self.entries.as_slice()[first].typed,
                    &self.entries.as_slice()[next].typed,
                )
                .is_eq()
            {
                next += 1;
            }
            let node = self.entries.as_slice()[next - 1].node;
            self.entries.as_mut_slice()[first].node = node;
            self.entries.as_mut_slice().swap(first, kept);
            kept += 1;
        }
        self.entries.truncate(kept);
        Ok(())
    }

    pub(super) fn entries(&self) -> impl ExactSizeIterator<Item = (&K, &value::Key<'d>, usize)> {
        self.entries
            .as_slice()
            .iter()
            .map(|entry| (&entry.typed, &entry.key, entry.node))
    }
}
