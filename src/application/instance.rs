//! Opaque identities for live application state instances.

use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};

/// An opaque identifier issued for one live application state instance.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Instance(usize);

impl Instance {
    /// Issue a fresh application instance identity.
    #[must_use]
    pub fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(1);

        #[allow(deprecated)] // MSRV 1.91 predates `AtomicUsize::try_update`.
        let issued = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .expect("Rawr application instance identifiers cannot be exhausted");
        Self(issued)
    }
}

impl Default for Instance {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Instance {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Instance(<private>)")
    }
}
