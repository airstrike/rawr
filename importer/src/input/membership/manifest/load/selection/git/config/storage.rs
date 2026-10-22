// Copyright the Rust Project Developers and smallvec contributors. MIT OR Apache-2.0.
//! Source Vec/VecDeque/SmallVec requests, with fallible prospective admission.

use super::{Error, Id, Session, Step};
use rawr_toml::document::{Admission, Request};
use std::{alloc::Layout, collections::VecDeque};

pub(super) fn allocation<A: Admission>(layout: Layout, session: &mut Session<'_, A>) -> Step<()> {
    if layout.size() != 0 {
        session.admit(Request::Allocation {
            bytes: layout.size(),
            alignment: layout.align(),
        })?;
    }
    Ok(())
}

pub(super) fn array<T, A: Admission>(count: usize, session: &mut Session<'_, A>) -> Step<()> {
    let Ok(layout) = Layout::array::<T>(count) else {
        return session.fail(Error::Overflow);
    };
    allocation(layout, session)
}

#[derive(Debug)]
pub(super) struct Vector<T> {
    values: Vec<T>,
    capacity: usize,
}
impl<T> Default for Vector<T> {
    fn default() -> Self {
        Self::new()
    }
}
impl<T> Vector<T> {
    pub(super) const fn new() -> Self {
        Self {
            values: Vec::new(),
            capacity: 0,
        }
    }
    pub(super) fn with_capacity<A: Admission>(
        capacity: usize,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        array::<T, _>(capacity, session)?;
        let mut values = Vec::new();
        if let Err(error) = values.try_reserve_exact(capacity) {
            return session.fail(Error::Allocation(error));
        }
        Ok(Self { values, capacity })
    }
    pub(super) fn len(&self) -> usize {
        self.values.len()
    }
    pub(super) fn as_slice(&self) -> &[T] {
        &self.values
    }
    pub(super) fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.values
    }
    pub(super) fn pop(&mut self) -> Option<T> {
        self.values.pop()
    }
    pub(super) fn into_values(self) -> Vec<T> {
        self.values
    }
    pub(super) fn into_iter(self) -> std::vec::IntoIter<T> {
        self.values.into_iter()
    }
    pub(super) fn reserve<A: Admission>(
        &mut self,
        additional: usize,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        let Some(required) = self.len().checked_add(additional) else {
            return session.fail(Error::Overflow);
        };
        if required <= self.capacity {
            return Ok(());
        }
        let minimum = if std::mem::size_of::<T>() == 1 {
            8
        } else if std::mem::size_of::<T>() <= 1024 {
            4
        } else {
            1
        };
        let Some(capacity) = self
            .capacity
            .checked_mul(2)
            .map(|n| n.max(required).max(minimum))
        else {
            return session.fail(Error::Overflow);
        };
        let mut replacement = Self::with_capacity(capacity, session)?;
        session.items(self.len())?;
        replacement.values.append(&mut self.values);
        *self = replacement;
        Ok(())
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
    pub(super) fn extend_copy<A: Admission>(
        &mut self,
        bytes: &[T],
        session: &mut Session<'_, A>,
    ) -> Step<()>
    where
        T: Copy,
    {
        self.reserve(bytes.len(), session)?;
        session.items(bytes.len())?;
        self.values.extend_from_slice(bytes);
        Ok(())
    }
}

#[derive(Debug, Default)]
pub(super) struct Order {
    values: VecDeque<Id>,
    capacity: usize,
}
impl Order {
    pub(super) fn reserve<A: Admission>(
        &mut self,
        additional: usize,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        let Some(required) = self.values.len().checked_add(additional) else {
            return session.fail(Error::Overflow);
        };
        if required <= self.capacity {
            return Ok(());
        }
        let Some(capacity) = self.capacity.checked_mul(2).map(|n| n.max(required).max(4)) else {
            return session.fail(Error::Overflow);
        };
        array::<Id, _>(capacity, session)?;
        // This slice uses push_back and consuming iteration only, so head is
        // always zero. Exact reservation preserves that source storage shape.
        if let Err(error) = self.values.try_reserve_exact(capacity - self.values.len()) {
            return session.fail(Error::Allocation(error));
        }
        self.capacity = capacity;
        Ok(())
    }
    pub(super) fn push<A: Admission>(&mut self, id: Id, session: &mut Session<'_, A>) -> Step<()> {
        session.items(1)?;
        self.reserve(1, session)?;
        self.values.push_back(id);
        Ok(())
    }
    pub(super) fn iter(&self) -> impl Iterator<Item = &Id> {
        self.values.iter()
    }
    pub(super) fn into_iter(self) -> std::collections::vec_deque::IntoIter<Id> {
        self.values.into_iter()
    }
}

pub(super) fn small_reserve<T, const N: usize, A: Admission>(
    values: &mut smallvec::SmallVec<[T; N]>,
    additional: usize,
    session: &mut Session<'_, A>,
) -> Step<()> {
    let Some(required) = values.len().checked_add(additional) else {
        return session.fail(Error::Overflow);
    };
    if required > values.capacity() {
        let Some(capacity) = required.checked_next_power_of_two() else {
            return session.fail(Error::Overflow);
        };
        array::<T, _>(capacity, session)?;
        session.items(1)?;
        if !values.spilled() {
            session.items(values.len())?;
        }
        if let Err(error) = values.try_reserve(additional) {
            return session.fail(Error::InlineAllocation(error));
        }
    }
    Ok(())
}

pub(super) fn small_push<T, const N: usize, A: Admission>(
    values: &mut smallvec::SmallVec<[T; N]>,
    value: T,
    session: &mut Session<'_, A>,
) -> Step<()> {
    session.items(1)?;
    small_reserve(values, 1, session)?;
    values.push(value);
    Ok(())
}
