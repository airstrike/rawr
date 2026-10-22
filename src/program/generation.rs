//! Opaque identities for immutable executable generations.

use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};

/// An opaque identifier issued for one compiled program generation.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GenerationId(usize);

impl fmt::Debug for GenerationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GenerationId(<private>)")
    }
}

impl GenerationId {
    pub(crate) fn issue() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(1);

        #[allow(deprecated)] // MSRV 1.91 predates `AtomicUsize::try_update`.
        let issued = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .expect("Rawr program generation identifiers cannot be exhausted");
        Self(issued)
    }
}
