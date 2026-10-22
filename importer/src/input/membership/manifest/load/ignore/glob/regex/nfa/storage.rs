//! Admitted retained cache arrays; allocator bonus capacity grants no growth.
use crate::input::membership::manifest::{session::Session, Error, Step};
use rawr_toml::document::{Admission, Request};
use std::{alloc::Layout, sync::Arc};

pub(super) struct Array<T> {
    pub(super) values: Vec<T>,
    pub(super) admitted: usize,
}

impl<T: Copy> Array<T> {
    pub(super) fn new() -> Self {
        Self {
            values: Vec::new(),
            admitted: 0,
        }
    }

    pub(super) fn reserve<A: Admission>(
        &mut self,
        incoming: usize,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        let Some(required) = self.values.len().checked_add(incoming) else {
            return session.fail(Error::Overflow);
        };
        if required <= self.admitted {
            return Ok(());
        }
        let Some(doubled) = self.admitted.max(2).checked_mul(2) else {
            return session.fail(Error::Overflow);
        };
        let admitted = doubled.max(required);
        let Ok(layout) = Layout::array::<T>(admitted) else {
            return session.fail(Error::Overflow);
        };
        session.admit(Request::Allocation {
            bytes: layout.size(),
            alignment: layout.align(),
        })?;
        let mut replacement = Vec::new();
        if let Err(error) = replacement.try_reserve_exact(admitted) {
            return session.fail(Error::Allocation(error));
        }
        session.items(self.values.len())?;
        replacement.extend_from_slice(&self.values);
        self.values = replacement;
        self.admitted = admitted;
        Ok(())
    }

    pub(super) fn push<A: Admission>(
        &mut self,
        value: T,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        self.reserve(1, session)?;
        session.items(1)?;
        self.values.push(value);
        Ok(())
    }

    pub(super) fn pop<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Step<Option<T>> {
        session.items(1)?;
        Ok(self.values.pop())
    }

    pub(super) fn resize<A: Admission>(
        &mut self,
        length: usize,
        value: T,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        if length <= self.values.len() {
            session.items(self.values.len() - length)?;
            self.values.truncate(length);
            return Ok(());
        }
        if length > self.admitted {
            let Some(doubled) = self.admitted.max(2).checked_mul(2) else {
                return session.fail(Error::Overflow);
            };
            let admitted = doubled.max(length);
            let Ok(layout) = Layout::array::<T>(admitted) else {
                return session.fail(Error::Overflow);
            };
            session.admit(Request::Allocation {
                bytes: layout.size(),
                alignment: layout.align(),
            })?;
            let mut replacement = Vec::new();
            if let Err(error) = replacement.try_reserve_exact(admitted) {
                return session.fail(Error::Allocation(error));
            }
            // Both extents remain live until every copy and new slot has been
            // admitted. A refusal leaves the original array intact.
            session.items(self.values.len())?;
            replacement.extend_from_slice(&self.values);
            session.items(length - self.values.len())?;
            replacement.resize(length, value);
            self.values = replacement;
            self.admitted = admitted;
        } else {
            session.items(length - self.values.len())?;
            self.values.resize(length, value);
        }
        Ok(())
    }
}

pub(super) fn shared<T, A: Admission>(value: T, session: &mut Session<'_, A>) -> Step<Arc<T>> {
    let Ok((layout, _)) = Layout::new::<[usize; 2]>().extend(Layout::new::<T>()) else {
        return session.fail(Error::Overflow);
    };
    let layout = layout.pad_to_align();
    session.admit(Request::Allocation {
        bytes: layout.size(),
        alignment: layout.align(),
    })?;
    Ok(Arc::new(value))
}
