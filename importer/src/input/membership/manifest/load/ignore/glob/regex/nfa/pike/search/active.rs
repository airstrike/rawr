// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../../LICENSE-regex-syntax-MIT.
//! Pinned SparseSet ordering and the zero-capture search setup projection.
use super::{cache, Admission, Error, Id, Session, Step};

impl cache::Cache {
    pub(super) fn setup<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Step<()> {
        session.items(self.stack.values.len())?;
        self.stack.values.clear();
        self.curr.clear(session)?;
        self.curr.slots.for_captures = 0;
        self.next.clear(session)?;
        self.next.slots.for_captures = 0;
        Ok(())
    }
}

impl cache::Active {
    pub(super) fn clear<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Step<()> {
        session.items(1)?;
        self.length = 0;
        Ok(())
    }

    pub(super) fn insert<A: Admission>(
        &mut self,
        id: Id,
        session: &mut Session<'_, A>,
    ) -> Step<bool> {
        session.items(1)?;
        let Some(index) = self.sparse.values.get(id.index()).copied() else {
            return session.fail(Error::State);
        };
        if index.index() < self.length {
            let Some(&stored) = self.dense.values.get(index.index()) else {
                return session.fail(Error::State);
            };
            if stored == id {
                return Ok(false);
            }
        }
        if self.length >= self.dense.values.len() {
            return session.fail(Error::State);
        }
        self.dense.values[self.length] = id;
        self.sparse.values[id.index()] = Id(self.length as u32);
        self.length += 1;
        Ok(true)
    }

    pub(super) fn at<A: Admission>(
        &self,
        index: usize,
        session: &mut Session<'_, A>,
    ) -> Step<Option<Id>> {
        session.items(1)?;
        if index >= self.length {
            return Ok(None);
        }
        match self.dense.values.get(index).copied() {
            Some(id) => Ok(Some(id)),
            None => session.fail(Error::State),
        }
    }

    pub(super) fn zero_row<A: Admission>(&self, id: Id, session: &mut Session<'_, A>) -> Step<()> {
        session.items(1)?;
        let Some(start) = id.index().checked_mul(self.slots.per_state) else {
            return session.fail(Error::Overflow);
        };
        if self.slots.for_captures != 0 || start > self.slots.table.values.len() {
            return session.fail(Error::State);
        }
        // Source for_state returns this empty row and copies zero slots.
        Ok(())
    }
}
