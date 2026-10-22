// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../LICENSE-regex-syntax-MIT.
//! regex-automata 0.4.14 util::search::PatternSet, with admitted ownership/work.
use super::super::finalize::storage;
use super::{Admission, Error, Session, Step};

pub(in super::super::super::super) struct Set {
    length: usize,
    which: Vec<bool>,
}

#[derive(Debug, PartialEq, Eq)]
pub(in super::super) struct Full {
    pub(in super::super) attempted: u32,
    pub(in super::super) capacity: usize,
}

impl Set {
    pub(in super::super::super::super) fn new<A: Admission>(
        capacity: usize,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.items(1)?;
        if capacity > i32::MAX as usize {
            return session.fail(Error::State);
        }
        Ok(Self {
            length: 0,
            which: storage::repeat(false, capacity, session)?,
        })
    }

    pub(in super::super::super::super) fn clear<A: Admission>(
        &mut self,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        self.length = 0;
        let mut index = 0;
        loop {
            session.items(1)?;
            let Some(entry) = self.which.get_mut(index) else {
                break;
            };
            *entry = false;
            index += 1;
        }
        Ok(())
    }

    pub(in super::super) fn insert<A: Admission>(
        &mut self,
        pattern: u32,
        session: &mut Session<'_, A>,
    ) -> Step<Result<bool, Full>> {
        session.items(1)?;
        if pattern as usize >= self.which.len() {
            return Ok(Err(Full {
                attempted: pattern,
                capacity: self.which.len(),
            }));
        }
        if self.which[pattern as usize] {
            return Ok(Ok(false));
        }
        self.length += 1;
        self.which[pattern as usize] = true;
        Ok(Ok(true))
    }

    pub(in super::super) fn is_empty(&self) -> bool {
        self.length == 0
    }
    pub(in super::super) fn is_full(&self) -> bool {
        self.length == self.which.len()
    }

    /// Pinned ascending-ID iteration, without an unadmitted result collection.
    pub(in super::super::super::super) fn next<A: Admission>(
        &self,
        cursor: &mut usize,
        session: &mut Session<'_, A>,
    ) -> Step<Option<u32>> {
        loop {
            session.items(1)?;
            let Some(&yes) = self.which.get(*cursor) else {
                return Ok(None);
            };
            let index = *cursor;
            *cursor += 1;
            if yes {
                return Ok(Some(index as u32));
            }
        }
    }
}
