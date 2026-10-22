//! Private identities for compiler engine provenance.

use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct Origin(usize);

pub(crate) struct Slot(AtomicUsize);

impl Origin {
    fn issue() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(1);

        #[allow(deprecated)] // MSRV 1.91 predates `AtomicUsize::try_update`.
        let issued = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .expect("Rawr engine origin identifiers cannot be exhausted");
        Self(issued)
    }
}

impl Slot {
    pub(crate) const fn unissued() -> Self {
        Self(AtomicUsize::new(0))
    }

    pub(crate) fn get(&self) -> Origin {
        let current = self.0.load(Ordering::Acquire);
        if current != 0 {
            return Origin(current);
        }

        let issued = Origin::issue();
        match self
            .0
            .compare_exchange(0, issued.0, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => issued,
            Err(current) => Origin(current),
        }
    }
}
