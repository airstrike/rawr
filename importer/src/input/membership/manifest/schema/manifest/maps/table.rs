//! Admitted ordinary string-key order and first-key/last-value replacement.

use rawr_toml::document;

use crate::input::membership::manifest::{session, storage, value, Span, Step};

/// The first converted key/origin and last complete value/origin of an equal key.
pub(in crate::input::membership::manifest) struct Entry<'d, K, V> {
    key: K,
    origin: value::Key<'d>,
    value: V,
    span: Option<Span>,
    ordinal: usize,
}

pub(super) struct Table<'d, K, V> {
    entries: storage::Buffer<Entry<'d, K, V>>,
}

impl<'d, K, V> Table<'d, K, V> {
    pub(super) const fn new() -> Self {
        Self {
            entries: storage::Buffer::new(),
        }
    }

    pub(super) fn push<A: document::Admission>(
        &mut self,
        key: K,
        origin: value::Key<'d>,
        value: V,
        span: Option<Span>,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        let ordinal = self.entries.len();
        self.entries.push(
            Entry {
                key,
                origin,
                value,
                span,
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
        storage::sort(self.entries.as_mut_slice(), |left, right| {
            left.origin
                .text
                .as_str()
                .cmp(right.origin.text.as_str())
                .then(left.ordinal.cmp(&right.ordinal))
        });
        let mut retained = 0;
        for index in 0..self.entries.len() {
            if retained != 0
                && self.entries.as_slice()[retained - 1].origin.text.as_str()
                    == self.entries.as_slice()[index].origin.text.as_str()
            {
                let (left, right) = self.entries.as_mut_slice().split_at_mut(index);
                std::mem::swap(&mut left[retained - 1].value, &mut right[0].value);
                left[retained - 1].span = right[0].span;
            } else {
                self.entries.as_mut_slice().swap(retained, index);
                retained += 1;
            }
        }
        self.entries.truncate(retained);
        Ok(())
    }

    pub(super) fn entry(&self, index: usize) -> Option<&Entry<'d, K, V>> {
        self.entries.as_slice().get(index)
    }

    pub(super) fn entries(&self) -> impl ExactSizeIterator<Item = &Entry<'d, K, V>> {
        self.entries.as_slice().iter()
    }
}

impl<'d, K, V> Entry<'d, K, V> {
    pub(in crate::input::membership::manifest) fn key(&self) -> &K {
        &self.key
    }
    pub(in crate::input::membership::manifest) fn origin(&self) -> &value::Key<'d> {
        &self.origin
    }
    pub(in crate::input::membership::manifest) fn value(&self) -> &V {
        &self.value
    }
    pub(in crate::input::membership::manifest) fn span(&self) -> Option<Span> {
        self.span
    }
}
