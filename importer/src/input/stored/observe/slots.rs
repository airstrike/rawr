use super::super::{Budget, Error, Result};

pub(in crate::input) struct Slots<T> {
    values: Vec<T>,
    admitted: usize,
}

impl<T> Slots<T> {
    pub(in crate::input) const fn new() -> Self {
        Self {
            values: Vec::new(),
            admitted: 0,
        }
    }

    pub(in crate::input) fn as_slice(&self) -> &[T] {
        &self.values
    }

    pub(in crate::input) fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.values
    }

    pub(in crate::input) fn push(&mut self, value: T, budget: &mut Budget<'_>) -> Result<()> {
        self.reserve(budget)?;
        self.values.push(value);
        Ok(())
    }

    /// Admit the next real element layout before its payload is observed.
    pub(in crate::input) fn reserve(&mut self, budget: &mut Budget<'_>) -> Result<()> {
        budget.check()?;
        if self.values.len() == self.admitted {
            let capacity = self
                .admitted
                .checked_mul(2)
                .ok_or_else(|| budget.overflow())?
                .max(1);
            let bytes = capacity
                .checked_mul(std::mem::size_of::<T>())
                .ok_or_else(|| budget.overflow())?;
            budget.storage(capacity, bytes)?;
            self.values
                .try_reserve_exact(capacity - self.values.len())
                .map_err(|error| budget.failure(Error::Allocation(error)))?;
            self.admitted = capacity;
        }
        Ok(())
    }

    pub(super) fn pop(&mut self) -> Option<T> {
        self.values.pop()
    }

    pub(in crate::input) fn finish(self, budget: &mut Budget<'_>) -> Result<Box<[T]>> {
        budget.owned_box(self.values)
    }

    pub(in crate::input) fn unique(mut self, budget: &mut Budget<'_>) -> Result<Box<[T]>>
    where
        T: Ord,
    {
        budget.check()?;
        self.values.sort_unstable();
        self.values.dedup();
        self.finish(budget)
    }

    pub(in crate::input) fn sorted(mut self, budget: &mut Budget<'_>) -> Result<Box<[T]>>
    where
        T: Ord,
    {
        budget.check()?;
        // Flat in-place sorting: count and compared byte extents are admitted
        // ownership, not additional filesystem or candidate-comparison work.
        self.values.sort_unstable();
        self.finish(budget)
    }
}
