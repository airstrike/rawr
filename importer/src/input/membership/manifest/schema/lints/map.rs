//! Charged lint maps: string-key replacement with first-key ownership.

use rawr_toml::document;

use super::super::super::{session, storage, value, Step};

struct Entry<'d, T> {
    key: value::Key<'d>,
    value: T,
    ordinal: usize,
}

pub(super) struct Map<'d, T> {
    entries: storage::Buffer<Entry<'d, T>>,
}

impl<'d, T> Map<'d, T> {
    pub(super) fn new() -> Self {
        Self {
            entries: storage::Buffer::new(),
        }
    }

    pub(super) fn push<A: document::Admission>(
        &mut self,
        key: value::Key<'d>,
        value: T,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        let ordinal = self.entries.len();
        self.entries.push(
            Entry {
                key,
                value,
                ordinal,
            },
            session,
        )?;
        Ok(())
    }

    pub(super) fn finish<A: document::Admission>(
        &mut self,
        insertion_order: bool,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        session.check()?;
        storage::sort(self.entries.as_mut_slice(), |a, b| {
            a.key
                .text
                .as_str()
                .cmp(b.key.text.as_str())
                .then(a.ordinal.cmp(&b.ordinal))
        });
        let mut next = 0;
        let mut retained = 0;
        while next < self.entries.len() {
            let first = next;
            next += 1;
            while next < self.entries.len()
                && self.entries.as_slice()[first].key.text.as_str()
                    == self.entries.as_slice()[next].key.text.as_str()
            {
                next += 1;
            }
            let last = next - 1;
            // Both upstream map implementations retain the first key and
            // replace its value; TOML also retains that key's insertion slot.
            let key = self.entries.as_slice()[first].key;
            let ordinal = self.entries.as_slice()[first].ordinal;
            self.entries.as_mut_slice()[last].key = key;
            self.entries.as_mut_slice()[last].ordinal = ordinal;
            self.entries.as_mut_slice().swap(retained, last);
            retained += 1;
        }
        while self.entries.len() > retained {
            self.entries.pop();
        }
        if insertion_order {
            storage::sort(self.entries.as_mut_slice(), |a, b| {
                a.ordinal.cmp(&b.ordinal)
            });
        }
        Ok(())
    }

    pub(super) fn iter(&self) -> impl ExactSizeIterator<Item = (&value::Key<'d>, &T)> {
        self.entries
            .as_slice()
            .iter()
            .map(|entry| (&entry.key, &entry.value))
    }
}
