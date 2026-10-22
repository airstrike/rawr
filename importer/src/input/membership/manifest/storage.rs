//! Fallible flat storage. Allocator slack never authorizes logical growth.

use std::alloc::Layout;

use rawr_toml::document;

use super::{session, Error, Step};

pub(super) mod slots;

#[cfg(test)]
thread_local! {
    pub(super) static ALLOCATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Debug)]
pub(super) struct Buffer<T> {
    values: Vec<T>,
    admitted: usize,
}

impl Buffer<u8> {
    /// Transfer final byte storage without collection, shrinking or growth.
    /// Consumers must not use allocator slack as authority for later mutation.
    pub(super) fn into_bytes(self) -> Vec<u8> {
        self.values
    }
}

impl<T> Buffer<T> {
    pub(super) const fn new() -> Self {
        Self {
            values: Vec::new(),
            admitted: 0,
        }
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
    /// Transfer the admitted allocation without cloning or collecting payloads.
    pub(super) fn into_iter(self) -> std::vec::IntoIter<T> {
        self.values.into_iter()
    }
    pub(super) fn clear(&mut self) {
        self.values.clear();
    }
    /// Drop a suffix without shrinking or refunding the admitted extent.
    pub(super) fn truncate(&mut self, length: usize) {
        self.values.truncate(length);
    }

    pub(super) fn push<A: document::Admission>(
        &mut self,
        value: T,
        session: &mut session::Session<'_, A>,
    ) -> Step<usize> {
        session.items(1)?;
        self.reserve_next(session)?;
        let index = self.values.len();
        self.values.push(value);
        Ok(index)
    }

    /// Reserve a next slot before evaluating an element whose construction
    /// itself has admitted effects (for example pinned vec![expression]).
    pub(super) fn reserve_next<A: document::Admission>(
        &mut self,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        self.reserve_more(1, session)
    }

    /// Reserve a known incoming batch before evaluating or moving its values.
    pub(super) fn reserve_more<A: document::Admission>(
        &mut self,
        additional: usize,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        session.check()?;
        let Some(required) = self.values.len().checked_add(additional) else {
            return session.fail(Error::Overflow);
        };
        if required > self.admitted {
            let Some(doubled) = self.admitted.max(2).checked_mul(2) else {
                return session.fail(Error::Overflow);
            };
            let capacity = doubled.max(required);
            let Ok(layout) = Layout::array::<T>(capacity) else {
                return session.fail(Error::Overflow);
            };
            session.admit(document::Request::Allocation {
                bytes: layout.size(),
                alignment: layout.align(),
            })?;
            // A separate replacement ensures the requested full layout is
            // prospective even if the old allocator supplied bonus capacity.
            let mut replacement = Vec::new();
            if let Err(error) = replacement.try_reserve_exact(capacity) {
                return session.fail(Error::Allocation(error));
            }
            #[cfg(test)]
            ALLOCATIONS.with(|count| count.set(count.get() + 1));
            replacement.append(&mut self.values);
            self.values = replacement;
            self.admitted = capacity;
        }
        Ok(())
    }
}

#[cfg(test)]
pub(super) fn bonus() -> Buffer<u64> {
    let mut values = Vec::with_capacity(64);
    values.extend([1, 2, 3, 4]);
    Buffer {
        values,
        admitted: 4,
    }
}

/// Allocation-free heapsort over already admitted entries and key bytes.
pub(super) fn sort<T>(values: &mut [T], compare: impl Fn(&T, &T) -> std::cmp::Ordering) {
    let result = try_sort(values, |left, right| {
        Ok::<_, std::convert::Infallible>(compare(left, right))
    });
    match result {
        Ok(()) => {}
        Err(never) => match never {},
    }
}

/// Allocation-free heapsort with immediate comparator failure propagation.
pub(super) fn try_sort<T, E>(
    values: &mut [T],
    mut compare: impl FnMut(&T, &T) -> Result<std::cmp::Ordering, E>,
) -> Result<(), E> {
    fn sift<T, E>(
        values: &mut [T],
        mut parent: usize,
        compare: &mut impl FnMut(&T, &T) -> Result<std::cmp::Ordering, E>,
    ) -> Result<(), E> {
        while let Some(left) = parent.checked_mul(2).and_then(|n| n.checked_add(1)) {
            if left >= values.len() {
                break;
            }
            let mut larger = left;
            if left + 1 < values.len() && compare(&values[left], &values[left + 1])?.is_lt() {
                larger += 1;
            }
            if !compare(&values[parent], &values[larger])?.is_lt() {
                break;
            }
            values.swap(parent, larger);
            parent = larger;
        }
        Ok(())
    }
    for parent in (0..values.len() / 2).rev() {
        sift(values, parent, &mut compare)?;
    }
    for end in (1..values.len()).rev() {
        values.swap(0, end);
        sift(&mut values[..end], 0, &mut compare)?;
    }
    Ok(())
}
