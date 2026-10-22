//! Closed replacing StateMap, retaining buckets across clear epochs.
use super::{dfa, Admission, Error, Session, Step};
use dfa::key;
use rawr_toml::document::Request;
use std::{
    collections::HashMap,
    hash::{BuildHasherDefault, DefaultHasher},
};

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
        // Pinned raw::find_or_find_insert_index reserves BEFORE equality.
        // An equal-key replacement at exhausted capacity must grow as well.
        if self.entries.len() == self.entries.capacity() {
            let Some(required) = self.entries.len().checked_add(1) else {
                return session.fail(Error::Overflow);
            };
            let Some((buckets, layout)) = dfa::intern::growth(required) else {
                return session.fail(Error::Overflow);
            };
            session.admit(Request::Allocation {
                bytes: layout.size(),
                alignment: layout.align(),
            })?;
            if let Err(error) = self.entries.try_reserve(1) {
                return session.fail(Error::Allocation(error));
            }
            if self.entries.capacity() != dfa::intern::capacity(buckets) {
                return session.fail(Error::State);
            }
        }
        // std retains the stored key, drops the incoming equal key and only
        // replaces its inert ID. No preliminary get changes reserve ordering.
        self.entries.insert(state, id);
        Ok(())
    }

    pub(super) fn clear<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Step<()> {
        session.items(self.entries.len())?;
        self.entries.clear();
        Ok(())
    }
}
