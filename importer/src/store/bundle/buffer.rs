//! Retained traversal slots with prospectively admitted replacement layouts.

use crate::node::{Error, GraphBudget};

pub(crate) struct Buffer<T> {
    values: Vec<T>,
    admitted: Option<usize>,
}

impl<T> Buffer<T> {
    pub(crate) const fn new() -> Self {
        Self {
            values: Vec::new(),
            admitted: Some(0),
        }
    }

    /// `value` already owns its separately admitted bytes. This admits the
    /// complete new slot allocation before growth, while prior storage remains
    /// cumulatively charged. No allocator-provided excess admits more slots.
    pub(crate) fn push(&mut self, value: T, budget: &GraphBudget) -> Result<(), Error> {
        let admitted = self.admitted.ok_or(Error::State(
            "legacy traversal slots cannot acquire shared authority",
        ))?;
        if self.values.len() == admitted {
            let capacity = admitted.checked_mul(2).ok_or(Error::Allocation)?.max(1);
            budget.decode(
                capacity
                    .checked_mul(std::mem::size_of::<T>())
                    .ok_or(Error::Allocation)?,
            )?;
            self.values
                .try_reserve_exact(capacity - self.values.len())
                .map_err(|_| Error::Allocation)?;
            self.admitted = Some(capacity);
        }
        self.values.push(value);
        Ok(())
    }

    /// Only the parent bundle's explicit legacy branch can use this seam.
    pub(super) fn push_legacy(&mut self, value: T) -> Result<(), Error> {
        if self.admitted.is_some_and(|admitted| admitted != 0) {
            return Err(Error::State(
                "shared traversal slots cannot discard their accounting",
            ));
        }
        self.admitted = None;
        self.values.try_reserve(1).map_err(|_| Error::Allocation)?;
        self.values.push(value);
        Ok(())
    }

    pub(crate) fn pop(&mut self) -> Option<T> {
        self.values.pop()
    }

    pub(crate) fn len(&self) -> usize {
        self.values.len()
    }

    pub(crate) fn into_vec(self) -> Vec<T> {
        self.values
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometric_replacement_layout_is_precharged_with_nonzero_prior_ownership() {
        let budget = GraphBudget::new();
        let cap = usize::try_from(crate::node::MAX_DECODED_BYTES_PER_GRAPH).unwrap();
        budget.decode(cap - 7 * std::mem::size_of::<u64>()).unwrap();
        let mut values = Buffer::new();
        values.push(1u64, &budget).unwrap();
        values.push(2, &budget).unwrap();
        values.push(3, &budget).unwrap();
        values.push(4, &budget).unwrap();
        assert_eq!(values.admitted, Some(4));
        assert!(!budget.exceedances().is_exceeded());
        // The next geometric layout (8 slots) is still charged in full to
        // the original account before growth; crossing the target records the
        // exact cumulative total instead of refusing the push.
        values
            .push(5, &budget)
            .expect("an accounting axis never refuses");
        assert_eq!(values.admitted, Some(8));
        assert!(values.values.capacity() >= 8);
        let axis = budget
            .exceedances()
            .axis(crate::node::DECODED_RESOURCE)
            .copied()
            .expect("the decoded-bytes axis crossed its target");
        assert_eq!(axis.target, crate::node::MAX_DECODED_BYTES_PER_GRAPH);
        assert_eq!(
            axis.first_requested,
            crate::node::MAX_DECODED_BYTES_PER_GRAPH + 8 * std::mem::size_of::<u64>() as u64
        );
        assert_eq!(axis.high_water, axis.first_requested);
        assert_eq!(values.into_vec(), [1, 2, 3, 4, 5]);
    }

    #[test]
    fn slot_reuse_spends_no_new_ownership_or_fictitious_observation() {
        let budget = GraphBudget::new();
        let mut values = Buffer::new();
        values.push(1u64, &budget).unwrap();
        assert_eq!(values.pop(), Some(1));
        let cap = usize::try_from(crate::node::MAX_DECODED_BYTES_PER_GRAPH).unwrap();
        budget.decode(cap - std::mem::size_of::<u64>()).unwrap();
        budget
            .paths(crate::node::MAX_PATH_CHECKS_PER_GRAPH)
            .unwrap();
        budget.exhaust_comparisons();
        values.push(2, &budget).unwrap();
        assert_eq!(values.into_vec(), [2]);
    }

    #[test]
    fn legacy_slots_cannot_be_promoted_to_shared_ownership() {
        let mut values = Buffer::new();
        values.push_legacy(1u64).unwrap();
        assert!(matches!(
            values.push(2, &GraphBudget::new()),
            Err(Error::State(_))
        ));
        assert_eq!(values.into_vec(), [1]);
    }
}
