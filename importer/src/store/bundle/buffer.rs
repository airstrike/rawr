//! Retained traversal slots with prospectively admitted replacement layouts.

use crate::node::Error;

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
    pub(crate) fn push(&mut self, value: T) -> Result<(), Error> {
        let admitted = self.admitted.ok_or(Error::State(
            "legacy traversal slots cannot acquire shared authority",
        ))?;
        if self.values.len() == admitted {
            let capacity = admitted.checked_mul(2).ok_or(Error::Allocation)?.max(1);
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
        let mut values = Buffer::new();
        values.push(1u64).unwrap();
        values.push(2).unwrap();
        values.push(3).unwrap();
        values.push(4).unwrap();
        assert_eq!(values.admitted, Some(4));
        // The next geometric layout (8 slots) is charged in full to the
        // original account before growth.
        values.push(5).unwrap();
        assert_eq!(values.admitted, Some(8));
        assert!(values.values.capacity() >= 8);
        assert_eq!(values.into_vec(), [1, 2, 3, 4, 5]);
    }

    #[test]
    fn slot_reuse_spends_no_new_ownership_or_fictitious_observation() {
        let mut values = Buffer::new();
        values.push(1u64).unwrap();
        assert_eq!(values.pop(), Some(1));
        values.push(2).unwrap();
        assert_eq!(values.into_vec(), [2]);
    }

    #[test]
    fn legacy_slots_cannot_be_promoted_to_shared_ownership() {
        let mut values = Buffer::new();
        values.push_legacy(1u64).unwrap();
        assert!(matches!(values.push(2), Err(Error::State(_))));
        assert_eq!(values.into_vec(), [1]);
    }
}
