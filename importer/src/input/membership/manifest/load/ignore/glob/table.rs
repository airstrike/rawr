// Copyright the ripgrep contributors. MIT OR Unlicense.
//! Closed globset FNV maps: source entry/insert and actual consuming bucket order.
use std::{
    alloc::Layout,
    collections::{hash_map, HashMap},
};

use rawr_toml::document::{Admission, Request};

use super::regex::nfa;
use crate::input::membership::manifest::{session::Session, Error, Step};

mod fnv;
mod layout;
#[cfg(test)]
mod tests;

/// Literal, basename and extension strategy index lists.
pub(super) struct Indices(Map<usize>);
/// Required-extension builder: source spelling stays owned until compilation.
pub(super) struct Regexes(Map<(usize, String)>);
/// Required-extension destination: each rule owns its distinct compiled Regex.
pub(super) struct Compiled(Map<(usize, nfa::Regex)>);

// These generic details never escape the three concrete facades. Keys are
// always owned Vec<u8>, the hasher is fixed, and no callback is accepted.
struct Map<T> {
    entries: HashMap<Vec<u8>, Rows<T>, fnv::Build>,
}
struct Rows<T> {
    values: Vec<T>,
    admitted: usize,
}

pub(super) struct Groups {
    entries: hash_map::IntoIter<Vec<u8>, Rows<(usize, String)>>,
}
pub(super) struct Texts(Rows<(usize, String)>);
pub(super) struct TextIter {
    entries: std::vec::IntoIter<(usize, String)>,
}

impl Indices {
    pub(super) fn new() -> Self {
        Self(Map::new())
    }

    pub(super) fn is_empty(&self) -> bool {
        self.0.entries.is_empty()
    }

    pub(super) fn add<A: Admission>(
        &mut self,
        index: usize,
        literal: String,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.string(literal.len())?;
        self.0
            .entry(literal.into_bytes(), session)?
            .push(index, session)
    }

    pub(super) fn contains<A: Admission>(
        &self,
        bytes: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<bool> {
        session.items(1)?;
        Ok(self.0.entries.contains_key(bytes))
    }

    pub(super) fn get<A: Admission>(
        &self,
        bytes: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<Option<&[usize]>> {
        session.items(1)?;
        Ok(self.0.entries.get(bytes).map(|rows| rows.values.as_slice()))
    }
}

impl Regexes {
    pub(super) fn new() -> Self {
        Self(Map::new())
    }

    pub(super) fn is_empty(&self) -> bool {
        self.0.entries.is_empty()
    }

    pub(super) fn add<A: Admission>(
        &mut self,
        index: usize,
        extension: String,
        regex: String,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.string(extension.len())?;
        session.string(regex.len())?;
        self.0
            .entry(extension.into_bytes(), session)?
            .push((index, regex), session)
    }

    pub(super) fn into_iter<A: Admission>(self, session: &mut Session<'_, A>) -> Step<Groups> {
        session.items(1)?;
        Ok(Groups {
            entries: self.0.entries.into_iter(),
        })
    }
}

impl Compiled {
    pub(super) fn new() -> Self {
        Self(Map::new())
    }

    /// Source build clones the extension and publishes an empty destination
    /// list before compiling any regex in that bucket's group.
    pub(super) fn insert_empty<A: Admission>(
        &mut self,
        extension: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        let key = copy(extension, session)?;
        self.0.insert_empty(key, session)
    }

    pub(super) fn push<A: Admission>(
        &mut self,
        extension: &[u8],
        index: usize,
        regex: nfa::Regex,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        let Some(rows) = self.0.entries.get_mut(extension) else {
            return session.fail(Error::State);
        };
        rows.push((index, regex), session)
    }

    /// Gives the compiled Regex's retained cache an exclusive borrow.
    pub(super) fn get_mut<A: Admission>(
        &mut self,
        extension: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<Option<&mut [(usize, nfa::Regex)]>> {
        session.items(1)?;
        Ok(self
            .0
            .entries
            .get_mut(extension)
            .map(|rows| rows.values.as_mut_slice()))
    }
}

impl Groups {
    pub(super) fn next<A: Admission>(
        &mut self,
        session: &mut Session<'_, A>,
    ) -> Step<Option<(Vec<u8>, Texts)>> {
        session.items(1)?;
        Ok(self.entries.next().map(|(key, rows)| (key, Texts(rows))))
    }
}

impl Texts {
    /// Called after the corresponding destination empty list is inserted.
    pub(super) fn into_iter<A: Admission>(self, session: &mut Session<'_, A>) -> Step<TextIter> {
        session.items(1)?;
        Ok(TextIter {
            entries: self.0.values.into_iter(),
        })
    }
}

impl TextIter {
    pub(super) fn next<A: Admission>(
        &mut self,
        session: &mut Session<'_, A>,
    ) -> Step<Option<(usize, String)>> {
        session.items(1)?;
        Ok(self.entries.next())
    }
}

impl<T> Map<T> {
    fn new() -> Self {
        Self {
            entries: HashMap::with_hasher(fnv::Build::default()),
        }
    }

    fn entry<A: Admission>(
        &mut self,
        key: Vec<u8>,
        session: &mut Session<'_, A>,
    ) -> Step<&mut Rows<T>> {
        session.items(1)?;
        // std uses rustc_entry: hash/search BEFORE reserving. The preliminary
        // closed lookup supplies a fallible reserve boundary only on vacancy.
        if !self.entries.contains_key(key.as_slice()) {
            self.reserve_one(session)?;
        }
        // This actual entry call drops an incoming equal key before returning
        // its occupied value. The caller's later append therefore cannot keep
        // that discarded key alive through a vector allocation/refusal.
        Ok(self.entries.entry(key).or_insert(Rows::new()))
    }

    fn insert_empty<A: Admission>(
        &mut self,
        key: Vec<u8>,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        // Ordinary insert differs from Entry: reserve before equality even
        // when replacing a key at exhausted capacity.
        self.reserve_one(session)?;
        self.entries.insert(key, Rows::new());
        Ok(())
    }

    fn reserve_one<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Step<()> {
        session.check()?;
        if self.entries.len() < self.entries.capacity() {
            return Ok(());
        }
        let Some(required) = self.entries.len().checked_add(1) else {
            return session.fail(Error::Overflow);
        };
        let Some((buckets, layout)) = layout::growth::<T>(required) else {
            return session.fail(Error::Overflow);
        };
        session.admit(Request::Allocation {
            bytes: layout.size(),
            alignment: layout.align(),
        })?;
        if let Err(error) = self.entries.try_reserve(1) {
            return session.fail(Error::Allocation(error));
        }
        if self.entries.capacity() != layout::capacity(buckets) {
            return session.fail(Error::State);
        }
        Ok(())
    }
}

impl<T> Rows<T> {
    fn new() -> Self {
        Self {
            values: Vec::new(),
            admitted: 0,
        }
    }

    fn push<A: Admission>(&mut self, value: T, session: &mut Session<'_, A>) -> Step<()> {
        session.items(1)?;
        if self.values.len() == self.admitted {
            let minimum = match std::mem::size_of::<T>() {
                0 => return session.fail(Error::State),
                1 => 8,
                2..=1024 => 4,
                _ => 1,
            };
            let Some(capacity) = self.admitted.checked_mul(2).map(|n| n.max(minimum)) else {
                return session.fail(Error::Overflow);
            };
            let Ok(layout) = Layout::array::<T>(capacity) else {
                return session.fail(Error::Overflow);
            };
            session.admit(Request::Allocation {
                bytes: layout.size(),
                alignment: layout.align(),
            })?;
            let mut replacement = Vec::new();
            if let Err(error) = replacement.try_reserve_exact(capacity) {
                return session.fail(Error::Allocation(error));
            }
            session.items(self.values.len())?;
            replacement.append(&mut self.values);
            self.values = replacement;
            self.admitted = capacity;
        }
        self.values.push(value);
        Ok(())
    }
}

fn copy<A: Admission>(bytes: &[u8], session: &mut Session<'_, A>) -> Step<Vec<u8>> {
    session.items(1)?;
    let mut output = Vec::new();
    if !bytes.is_empty() {
        let Ok(layout) = Layout::array::<u8>(bytes.len()) else {
            return session.fail(Error::Overflow);
        };
        session.admit(Request::Allocation {
            bytes: layout.size(),
            alignment: layout.align(),
        })?;
        if let Err(error) = output.try_reserve_exact(bytes.len()) {
            return session.fail(Error::Allocation(error));
        }
    }
    session.items(bytes.len())?;
    output.extend_from_slice(bytes);
    Ok(output)
}
