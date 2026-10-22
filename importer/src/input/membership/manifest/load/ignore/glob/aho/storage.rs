//! Actual fallible Vec/VecDeque storage for the closed Aho construction types.
use std::{alloc::Layout, collections::VecDeque};

use rawr_toml::document::{Admission, Request};

use crate::input::membership::manifest::{session::Session, Error, Step};

pub(super) struct Buffer<T> {
    values: Vec<T>,
    admitted: usize,
}

impl<T> Buffer<T> {
    pub(super) fn new() -> Self {
        Self {
            values: Vec::new(),
            admitted: 0,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.values.len()
    }

    pub(super) fn slice(&self) -> &[T] {
        &self.values
    }

    pub(super) fn slice_mut(&mut self) -> &mut [T] {
        &mut self.values
    }

    pub(super) fn push<A: Admission>(
        &mut self,
        value: T,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        self.reserve(1, session)?;
        self.values.push(value);
        Ok(())
    }

    pub(super) fn reserve<A: Admission>(
        &mut self,
        extra: usize,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.check()?;
        let Some(required) = self.len().checked_add(extra) else {
            return session.fail(Error::Overflow);
        };
        if required <= self.admitted {
            return Ok(());
        }
        let minimum = match std::mem::size_of::<T>() {
            0 => return session.fail(Error::State),
            1 => 8,
            2..=1024 => 4,
            _ => 1,
        };
        let Some(capacity) = self
            .admitted
            .checked_mul(2)
            .map(|n| n.max(required).max(minimum))
        else {
            return session.fail(Error::Overflow);
        };
        self.replace(capacity, session)
    }

    fn replace<A: Admission>(&mut self, capacity: usize, session: &mut Session<'_, A>) -> Step<()> {
        admit::<T, A>(capacity, session)?;
        let mut values = Vec::new();
        if let Err(error) = values.try_reserve_exact(capacity) {
            return session.fail(Error::Allocation(error));
        }
        session.items(self.len())?;
        values.append(&mut self.values);
        self.values = values;
        self.admitted = capacity;
        Ok(())
    }

    pub(super) fn compact<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Step<()> {
        session.check()?;
        if self.admitted != self.len() {
            self.replace(self.len(), session)?;
        }
        Ok(())
    }

    pub(super) fn filled<A: Admission>(
        length: usize,
        value: T,
        session: &mut Session<'_, A>,
    ) -> Step<Self>
    where
        T: Copy,
    {
        let mut buffer = Self::new();
        if length != 0 {
            buffer.replace(length, session)?;
        }
        session.items(length)?;
        buffer.values.resize(length, value);
        Ok(buffer)
    }

    pub(super) fn copy<A: Admission>(values: &[T], session: &mut Session<'_, A>) -> Step<Self>
    where
        T: Copy,
    {
        let mut buffer = Self::new();
        if !values.is_empty() {
            buffer.replace(values.len(), session)?;
        }
        session.items(values.len())?;
        buffer.values.extend_from_slice(values);
        Ok(buffer)
    }
}

impl Buffer<u32> {
    pub(super) fn indices<A: Admission>(length: usize, session: &mut Session<'_, A>) -> Step<Self> {
        let mut buffer = Self::new();
        if length != 0 {
            buffer.replace(length, session)?;
        }
        for index in 0..length {
            session.items(1)?;
            buffer.values.push(index as u32);
        }
        Ok(buffer)
    }
}

pub(super) struct Queue {
    values: VecDeque<u32>,
    admitted: usize,
}
impl Queue {
    pub(super) fn new() -> Self {
        Self {
            values: VecDeque::new(),
            admitted: 0,
        }
    }

    pub(super) fn push<A: Admission>(
        &mut self,
        value: u32,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        if self.values.len() == self.admitted {
            let Some(capacity) = self.admitted.checked_mul(2).map(|n| n.max(4)) else {
                return session.fail(Error::Overflow);
            };
            admit::<u32, A>(capacity, session)?;
            let mut values = VecDeque::new();
            if let Err(error) = values.try_reserve_exact(capacity) {
                return session.fail(Error::Allocation(error));
            }
            session.items(self.values.len())?;
            values.append(&mut self.values);
            self.values = values;
            self.admitted = capacity;
        }
        self.values.push_back(value);
        Ok(())
    }

    pub(super) fn pop<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Step<Option<u32>> {
        session.items(1)?;
        Ok(self.values.pop_front())
    }
}

pub(super) fn admit<T, A: Admission>(count: usize, session: &mut Session<'_, A>) -> Step<()> {
    let Ok(layout) = Layout::array::<T>(count) else {
        return session.fail(Error::Overflow);
    };
    if layout.size() != 0 {
        session.admit(Request::Allocation {
            bytes: layout.size(),
            alignment: layout.align(),
        })?;
    }
    Ok(())
}
