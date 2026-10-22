//! Canonical shared states: admitted Vec growth, with no implicit Arc clones.
use super::super::finalize::storage;
use super::{key, Admission, Error, Session, Step};

pub(super) struct States {
    pub(super) values: Vec<key::State>,
    admitted: usize,
}

impl States {
    pub(super) fn new() -> Self {
        Self {
            values: Vec::new(),
            admitted: 0,
        }
    }

    pub(super) fn push<A: Admission>(
        &mut self,
        state: key::State,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        if self.values.len() == self.admitted {
            let Some(capacity) = self.admitted.max(2).checked_mul(2) else {
                return session.fail(Error::Overflow);
            };
            let mut replacement = storage::vector(capacity, session)?;
            session.items(self.values.len())?;
            replacement.append(&mut self.values);
            self.values = replacement;
            self.admitted = capacity;
        }
        self.values.push(state);
        Ok(())
    }

    pub(super) fn clear<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Step<()> {
        session.items(self.values.len())?;
        self.values.clear();
        Ok(())
    }
}
