//! Actual full replacement layouts, including inline native transfer buffers.

use crate::input::membership::{Budget, Error, Result};

pub(super) struct Buffer<T> {
    values: Vec<T>,
    admitted: usize,
}
impl<T> Buffer<T> {
    pub(super) const fn new() -> Self {
        Self {
            values: Vec::new(),
            admitted: 0,
        }
    }
    pub(super) fn as_slice(&self) -> &[T] {
        &self.values
    }
    pub(super) fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.values
    }
    pub(super) fn into_values(self) -> Vec<T> {
        self.values
    }
    pub(super) fn remove(&mut self, index: usize) -> T {
        self.values.remove(index)
    }

    pub(super) fn prepare(&mut self, budget: &mut Budget<'_>) -> Result<()> {
        budget.allocation.items(1)?;
        if self.values.len() == self.admitted {
            let next = self
                .admitted
                .max(2)
                .checked_mul(2)
                .ok_or(Error::Unconsumed)?;
            let bytes = std::alloc::Layout::array::<T>(next)
                .map_err(|_| Error::Encoding("traversal storage overflow"))?
                .size();
            budget.allocation.bytes(bytes)?;
            let mut replacement = Vec::new();
            replacement.try_reserve_exact(next)?;
            replacement.append(&mut self.values);
            self.values = replacement;
            self.admitted = next;
        }
        Ok(())
    }

    pub(super) fn push(&mut self, value: T) -> Result<()> {
        if self.values.len() >= self.admitted {
            return Err(Error::Unconsumed);
        }
        self.values.push(value);
        Ok(())
    }
}
