//! Retained generated-child metadata and the refusal latch its scopes share.
//!
//! Ownership does not authenticate a live native generation. The compiler
//! owner must independently bind its engine, profile and process epoch.
//!
//! No figure is counted here (D674). A scope keeps the decoded metadata
//! alive, and a producer whose checked arithmetic could not size a retained
//! value refuses. That refusal latches every scope sharing the account and
//! keeps its first cause.

#[cfg(test)]
pub(crate) mod tests;

pub(crate) mod support;

use std::{
    fmt,
    sync::{Arc, Mutex, MutexGuard},
};

use crate::host::ImportAllocation;
use rawr_import::{runner::manifest::Carrier, surface::Error};

/// Decoded native metadata held with its compiler/runtime owner.
///
/// Keep this owner until all reconstructed native metadata has retired.
#[doc(hidden)]
pub struct Native {
    inner: Arc<Baseline>,
}

struct Baseline {
    carrier: Carrier,
    allocation: Mutex<Attempt>,
}

/// One candidate's scope in the shared account.
///
/// A successful candidate consumes this scope into a receipt held by its
/// executable owner, which keeps the decoded metadata alive.
#[doc(hidden)]
#[must_use]
pub struct Attempt {
    latch: Arc<Mutex<Option<Refusal>>>,
    // The baseline scope itself has no owner, so this cannot form a cycle.
    baseline: Option<Arc<Baseline>>,
}

/// The scope retained until the last owner of one candidate retires.
///
/// This cannot be constructed or cloned independently of an actual attempt.
#[doc(hidden)]
#[derive(Debug)]
#[must_use]
pub struct Receipt {
    // Held only to keep the scope's decoded metadata alive.
    _allocation: Attempt,
}

/// The axis whose amount a producer's checked arithmetic could not represent.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Axis {
    /// Retained metadata items.
    Items,
    /// Retained metadata bytes.
    Bytes,
}

/// The first refused charge, which latches the whole shared account.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Refusal {
    /// The first axis the producer could not size, items before bytes.
    pub axis: Axis,
}

impl Native {
    /// Decode the compiled carrier and retain its metadata.
    pub fn decode(manifest: &[u8]) -> Result<Self, super::Error> {
        Ok(Self::transform(Carrier::transform(manifest)?))
    }

    /// Retain already-decoded metadata in a fresh account.
    pub fn transform(carrier: Carrier) -> Self {
        Self::retain(carrier, Attempt::fresh())
    }

    fn retain(carrier: Carrier, allocation: Attempt) -> Self {
        Self {
            inner: Arc::new(Baseline {
                carrier,
                allocation: Mutex::new(allocation),
            }),
        }
    }

    fn reconstruct(&self, manifest: &[u8]) -> Result<Self, super::Error> {
        let carrier = Carrier::transform(manifest)?;
        // A replacement owns its own carrier and facade scope. Only the latch
        // is shared: retaining the previous baseline would build an
        // ever-growing chain of retired engines' metadata.
        let allocation = Attempt {
            latch: Arc::clone(&lock(&self.inner.allocation).latch),
            baseline: None,
        };
        allocation.admit()?;
        Ok(Self::retain(carrier, allocation))
    }

    /// Borrow inventory metadata; this alone grants no live reuse authority.
    #[must_use]
    pub fn carrier(&self) -> &Carrier {
        &self.inner.carrier
    }

    /// Begin a candidate scope in this same account.
    pub fn attempt(&self) -> Result<Attempt, Error> {
        let attempt = Attempt {
            latch: Arc::clone(&lock(&self.inner.allocation).latch),
            baseline: Some(Arc::clone(&self.inner)),
        };
        attempt.admit()?;
        Ok(attempt)
    }

    /// The first refusal that latched this account, if any.
    #[must_use]
    pub fn first_refusal(&self) -> Option<Refusal> {
        *lock(&lock(&self.inner.allocation).latch)
    }

    /// How many scopes keep this decoded metadata alive.
    #[cfg(test)]
    pub(crate) fn owners(&self) -> usize {
        Arc::strong_count(&self.inner)
    }

    pub(super) fn allocation(&mut self) -> MutexGuard<'_, Attempt> {
        lock(&self.inner.allocation)
    }

    pub(super) fn parts(&mut self) -> (&Carrier, MutexGuard<'_, Attempt>) {
        (&self.inner.carrier, lock(&self.inner.allocation))
    }
}

impl crate::Engine {
    /// Reconstruct a fresh compiler's metadata under this child's latch.
    ///
    /// Each compiler keeps its own carrier, facades and module cache. This shares
    /// only the latch and grants no artifact or reuse authority.
    #[doc(hidden)]
    pub fn reconstruct_imported_native(&self, manifest: &[u8]) -> Result<Native, super::Error> {
        self.imported_native
            .as_ref()
            .ok_or(super::Error::NativeOwner)?
            .reconstruct(manifest)
    }
}

impl Attempt {
    /// A scope in a fresh account that shares no native owner.
    pub(super) fn fresh() -> Self {
        Self {
            latch: Arc::new(Mutex::new(None)),
            baseline: None,
        }
    }

    /// Refuse a new scope once the account has latched.
    fn admit(&self) -> Result<(), Error> {
        match *lock(&self.latch) {
            Some(_) => Err(Error::Allocation),
            None => Ok(()),
        }
    }

    /// Transfer this scope to its successful candidate owner.
    pub fn finish(self) -> Receipt {
        Receipt { _allocation: self }
    }
}

impl ImportAllocation for Attempt {
    fn retain_checked(&mut self, items: Option<usize>, bytes: Option<usize>) -> Result<(), Error> {
        let mut latch = lock(&self.latch);
        // A refusal is terminal for the whole shared account. Every later
        // admission (a charge, a fresh attempt or a reconstruction sharing
        // the account) is refused and the first cause is preserved.
        if latch.is_some() {
            return Err(Error::Allocation);
        }
        let axis = match (items, bytes) {
            (Some(_), Some(_)) => return Ok(()),
            (None, _) => Axis::Items,
            (Some(_), None) => Axis::Bytes,
        };
        *latch = Some(Refusal { axis });
        Err(Error::Allocation)
    }
}

impl fmt::Debug for Attempt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Attempt")
            .field("retains_native", &self.baseline.is_some())
            .finish_non_exhaustive()
    }
}

fn lock<T>(account: &Mutex<T>) -> MutexGuard<'_, T> {
    match account.lock() {
        Ok(state) => state,
        Err(poisoned) => poisoned.into_inner(),
    }
}
