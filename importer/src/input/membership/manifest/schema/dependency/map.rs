//! Prospective flat map ownership, string order and pinned replacement behavior.
//! Sorting and linear compaction perform no allocation or graph-account event.

use rawr_toml::document;

use super::super::super::{session, storage, value, Step};

struct Entry<'d, K, V> {
    key: K,
    origin: value::Key<'d>,
    value: V,
    ordinal: usize,
}

pub(super) struct Map<'d, K, V> {
    entries: storage::Buffer<Entry<'d, K, V>>,
}

impl<'d, K, V> Map<'d, K, V> {
    pub(super) fn new() -> Self {
        Self {
            entries: storage::Buffer::new(),
        }
    }

    pub(super) fn push<A: document::Admission>(
        &mut self,
        key: K,
        origin: value::Key<'d>,
        value: V,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        let ordinal = self.entries.len();
        self.entries.push(
            Entry {
                key,
                origin,
                value,
                ordinal,
            },
            session,
        )?;
        Ok(())
    }

    pub(super) fn finish<A: document::Admission>(
        &mut self,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        session.check()?;
        storage::sort(self.entries.as_mut_slice(), |a, b| {
            a.origin
                .text
                .as_str()
                .cmp(b.origin.text.as_str())
                .then(a.ordinal.cmp(&b.ordinal))
        });
        let mut retained = 0;
        for index in 0..self.entries.len() {
            if retained != 0
                && self.entries.as_slice()[retained - 1].origin.text.as_str()
                    == self.entries.as_slice()[index].origin.text.as_str()
            {
                // Equal parsed PackageName/String keys compare by exact text.
                // Keep the first key/origin and the last fully converted value.
                let (left, right) = self.entries.as_mut_slice().split_at_mut(index);
                std::mem::swap(&mut left[retained - 1].value, &mut right[0].value);
            } else {
                self.entries.as_mut_slice().swap(retained, index);
                retained += 1;
            }
        }
        self.entries.truncate(retained);
        Ok(())
    }

    pub(super) fn entry(&self, index: usize) -> Option<(&K, &value::Key<'d>, &V)> {
        self.entries
            .as_slice()
            .get(index)
            .map(|entry| (&entry.key, &entry.origin, &entry.value))
    }

    pub(super) fn entries(&self) -> impl ExactSizeIterator<Item = (&K, &value::Key<'d>, &V)> {
        self.entries
            .as_slice()
            .iter()
            .map(|entry| (&entry.key, &entry.origin, &entry.value))
    }
}
