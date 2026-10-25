//! Retained generated-child metadata and scoped candidate allocation.
//!
//! Accounting ownership does not authenticate a live native generation. The
//! compiler owner must independently bind its engine, profile and process epoch.

#[cfg(test)]
pub(crate) mod tests;

pub(crate) mod support;

use std::{
    fmt,
    num::NonZeroU64,
    sync::{Arc, Mutex, MutexGuard},
};

use crate::host::ImportAllocation;
use rawr_import::{cargo::Limits, exceedance, runner::manifest::Carrier, surface::Error};

const ITEMS_RESOURCE: &str = "runner retained items";
const BYTES_RESOURCE: &str = "runner retained bytes";

/// The shared bounded codec used by generated runner observations.
#[doc(hidden)]
pub use rawr_import::protocol::account as wire;

/// Decoded native metadata and the charges held with its compiler/runtime owner.
///
/// Keep this owner until all reconstructed native metadata has retired. Failed
/// setup can have installed part of an engine, so its charges remain here too.
#[doc(hidden)]
pub struct Native {
    inner: Arc<Baseline>,
}

struct Baseline {
    carrier: Carrier,
    allocation: Mutex<Attempt>,
}

/// One candidate's contribution to the same aggregate child account.
///
/// Dropping an unsuccessful attempt releases its contribution. A successful
/// candidate consumes this scope into a receipt held by its executable owner.
#[doc(hidden)]
#[must_use]
pub struct Attempt {
    account: Arc<Mutex<State>>,
    // The baseline scope itself has no owner, so this cannot form a cycle.
    baseline: Option<Arc<Baseline>>,
    items: usize,
    bytes: usize,
}

/// The allocation retained until the last owner of one candidate retires.
///
/// This cannot be constructed or cloned independently of an actual attempt.
#[doc(hidden)]
#[derive(Debug)]
#[must_use]
pub struct Receipt {
    allocation: Attempt,
}

/// One original child account's fixed facts, without admission authority.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Snapshot {
    /// Process-unique account identity, drawn from the one exceedance id space
    /// every published account shares; absent only after identity exhaustion.
    pub account: Option<NonZeroU64>,
    /// Currently retained items and bytes, in that order.
    pub current: [usize; 2],
    /// Largest accepted aggregate on each axis, including overlapping owners.
    pub peak: [usize; 2],
    /// Unchanged original item and byte accounting targets.
    pub limits: [usize; 2],
    /// Earliest rejected charge, retained after an unsuccessful scope retires.
    /// Only checked-arithmetic failures refuse.
    pub first_refusal: Option<Refusal>,
    /// Accounting targets crossed so far, with truthful high waters.
    pub exceeded: exceedance::Record,
}

/// A resource axis in the original retained metadata account.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Axis {
    /// Retained metadata items.
    Items,
    /// Retained metadata bytes.
    Bytes,
}

/// The original check which rejected a charge, in its original check order.
///
/// Only genuine arithmetic failures refuse; a representable total
/// above the item or byte target is recorded as an exceedance and accepted.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cause {
    /// The producer could not represent the amount passed to the account.
    MissingAmount,
    /// Adding to this scope's accepted contribution overflowed.
    OwnedOverflow,
    /// Adding to the shared account's accepted contribution overflowed.
    AggregateOverflow,
}

/// Fixed evidence of one refused charge, separate from accepted usage.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Refusal {
    /// The first axis rejected by the original check order.
    pub axis: Axis,
    /// The original arithmetic check which refused the charge.
    pub cause: Cause,
    /// Aggregate accepted items and bytes when this refusal was recorded.
    pub current: [usize; 2],
    /// Requested aggregate on the refused axis, absent when unrepresentable.
    pub requested: Option<usize>,
    /// Original limit for the refused axis.
    pub limit: usize,
}

/// Read-only observation of the same account, without retaining native owners.
///
/// This uncloneable handle owns one charged fixed slot. It never owns a carrier,
/// baseline or executable receipt, and records no history of individual charges.
#[doc(hidden)]
#[must_use]
pub struct Observation {
    state: ObservationState,
}

// Boxing `Refused` would allocate merely to report a refusal (see below).
#[allow(clippy::large_enum_variant)]
enum ObservationState {
    Unreached,
    // A rejected constructor retains only by-value evidence; refused shared or
    // observation storage must not be allocated merely to report its refusal.
    Refused(Snapshot),
    Retained(Attempt),
}

#[derive(Debug)]
struct State {
    identity: Option<NonZeroU64>,
    items: usize,
    bytes: usize,
    item_limit: usize,
    byte_limit: usize,
    peak: [usize; 2],
    first_refusal: Option<Refusal>,
    exceeded: exceedance::Record,
}

impl Native {
    /// Decode the compiled carrier and retain its actual metadata ownership.
    pub fn decode(manifest: &[u8]) -> Result<Self, super::Error> {
        Ok(Self::transform(Carrier::transform(manifest)?)?)
    }

    /// Decode once and retain fixed evidence through success or constructor failure.
    ///
    /// Carrier failure before the native account is reached has no snapshot. The
    /// returned result preserves the same original error as ordinary decoding.
    pub fn decode_observed(manifest: &[u8]) -> (Result<Self, super::Error>, Observation) {
        let carrier = match Carrier::transform(manifest) {
            Ok(carrier) => carrier,
            Err(error) => {
                return (
                    Err(error.into()),
                    Observation {
                        state: ObservationState::Unreached,
                    },
                );
            }
        };
        let limits = Limits::standard();
        let (result, observation) =
            Self::with_limits_observed(carrier, limits.item_limit(), limits.byte_limit());
        (result.map_err(Into::into), observation)
    }

    fn reconstruct_observed(&self, manifest: &[u8]) -> (Result<Self, super::Error>, Observation) {
        let carrier = match Carrier::transform(manifest) {
            Ok(carrier) => carrier,
            Err(error) => {
                return (
                    Err(error.into()),
                    Observation {
                        state: ObservationState::Refused(self.snapshot()),
                    },
                );
            }
        };
        // A replacement owns its own carrier and facade scope. Only the original
        // account is shared: retaining the previous baseline would build an
        // ever-growing chain of retired engines' metadata.
        let allocation = Attempt {
            account: Arc::clone(&lock(&self.inner.allocation).account),
            baseline: None,
            items: 0,
            bytes: 0,
        };
        let (allocation, observation) = Attempt::observe(allocation);
        (
            allocation
                .and_then(|allocation| Self::retain(carrier, allocation))
                .map_err(Into::into),
            observation,
        )
    }

    /// Retain already-decoded metadata under the unchanged child limits.
    pub fn transform(carrier: Carrier) -> Result<Self, Error> {
        let limits = Limits::standard();
        Self::with_limits(carrier, limits.item_limit(), limits.byte_limit())
    }

    fn with_limits(carrier: Carrier, item_limit: usize, byte_limit: usize) -> Result<Self, Error> {
        Self::retain(carrier, Attempt::empty(item_limit, byte_limit))
    }

    fn with_limits_observed(
        carrier: Carrier,
        item_limit: usize,
        byte_limit: usize,
    ) -> (Result<Self, Error>, Observation) {
        let (allocation, observation) = Attempt::empty_observed(item_limit, byte_limit);
        (
            allocation.and_then(|allocation| Self::retain(carrier, allocation)),
            observation,
        )
    }

    fn retain(carrier: Carrier, mut allocation: Attempt) -> Result<Self, Error> {
        let (items, bytes) = carrier.retained_usage();
        allocation.retain_checked(items.checked_add(1), bytes.checked_add(baseline_bytes()))?;
        Ok(Self {
            inner: Arc::new(Baseline {
                carrier,
                allocation: Mutex::new(allocation),
            }),
        })
    }

    /// Borrow inventory metadata; this alone grants no live reuse authority.
    #[must_use]
    pub fn carrier(&self) -> &Carrier {
        &self.inner.carrier
    }

    /// Begin an independently retired candidate scope in this same account.
    pub fn attempt(&self) -> Result<Attempt, Error> {
        let mut attempt = Attempt {
            account: Arc::clone(&lock(&self.inner.allocation).account),
            baseline: Some(Arc::clone(&self.inner)),
            items: 0,
            bytes: 0,
        };
        attempt.retain_checked(Some(1), Some(std::mem::size_of::<Attempt>()))?;
        Ok(attempt)
    }

    /// Report the aggregate native, live and pending retained metadata.
    #[must_use]
    pub fn usage(&self) -> (usize, usize) {
        let allocation = lock(&self.inner.allocation);
        let state = lock(&allocation.account);
        (state.items, state.bytes)
    }

    /// Copy fixed facts from the actual shared native account.
    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        let allocation = lock(&self.inner.allocation);
        lock(&allocation.account).snapshot()
    }

    pub(super) fn allocation(&mut self) -> MutexGuard<'_, Attempt> {
        lock(&self.inner.allocation)
    }

    pub(super) fn parts(&mut self) -> (&Carrier, MutexGuard<'_, Attempt>) {
        (&self.inner.carrier, lock(&self.inner.allocation))
    }
}

impl crate::Engine {
    /// Reconstruct a fresh compiler's metadata in this child's original account.
    ///
    /// Each compiler keeps its own carrier, facades and module cache. This shares
    /// only allocation ownership and grants no artifact or reuse authority.
    #[doc(hidden)]
    pub fn reconstruct_imported_native(
        &self,
        manifest: &[u8],
    ) -> (Result<Native, super::Error>, Observation) {
        match &self.imported_native {
            Some(native) => native.reconstruct_observed(manifest),
            None => (
                Err(super::Error::NativeOwner),
                Observation {
                    state: ObservationState::Unreached,
                },
            ),
        }
    }
}

impl Attempt {
    /// Open the shared account with its storage charged. An item or byte
    /// target below that storage is recorded as an exceedance, never refused.
    fn empty(item_limit: usize, byte_limit: usize) -> Self {
        let mut state = State {
            identity: NonZeroU64::new(exceedance::next_id()),
            items: 0,
            bytes: 0,
            item_limit,
            byte_limit,
            peak: [0; 2],
            first_refusal: None,
            exceeded: exceedance::Record::new(),
        };
        // Shared storage remains live even if Native retires before a
        // candidate. Its charge belongs to the account, not one scope.
        state.items = 1;
        state.bytes = shared_bytes();
        state.peak = [state.items, state.bytes];
        state.exceeded(Axis::Items, state.items);
        state.exceeded(Axis::Bytes, state.bytes);
        Self {
            account: Arc::new(Mutex::new(state)),
            baseline: None,
            items: 0,
            bytes: 0,
        }
    }

    fn empty_observed(item_limit: usize, byte_limit: usize) -> (Result<Self, Error>, Observation) {
        Self::observe(Self::empty(item_limit, byte_limit))
    }

    fn observe(allocation: Self) -> (Result<Self, Error>, Observation) {
        let mut observation = Self {
            account: Arc::clone(&allocation.account),
            baseline: None,
            items: 0,
            bytes: 0,
        };
        if let Err(error) =
            observation.retain_checked(Some(1), Some(std::mem::size_of::<Observation>()))
        {
            let snapshot = lock(&allocation.account).snapshot();
            return (
                Err(error),
                Observation {
                    state: ObservationState::Refused(snapshot),
                },
            );
        }
        (
            Ok(allocation),
            Observation {
                state: ObservationState::Retained(observation),
            },
        )
    }

    pub(super) fn standard() -> Result<Self, Error> {
        let limits = Limits::standard();
        let mut attempt = Self::empty(limits.item_limit(), limits.byte_limit());
        attempt.retain_checked(Some(1), Some(std::mem::size_of::<Self>()))?;
        Ok(attempt)
    }

    /// Transfer this exact contribution to its successful candidate owner.
    pub fn finish(self) -> Receipt {
        Receipt { allocation: self }
    }

    /// Report this scope's retained contribution, including receipt storage.
    #[must_use]
    pub const fn usage(&self) -> (usize, usize) {
        (self.items, self.bytes)
    }
}

impl Observation {
    /// Read current facts; absent means carrier decoding never reached this account.
    #[must_use]
    pub fn snapshot(&self) -> Option<Snapshot> {
        match &self.state {
            ObservationState::Unreached => None,
            ObservationState::Refused(snapshot) => Some(*snapshot),
            ObservationState::Retained(allocation) => Some(lock(&allocation.account).snapshot()),
        }
    }
}

impl State {
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            account: self.identity,
            current: [self.items, self.bytes],
            peak: self.peak,
            limits: [self.item_limit, self.byte_limit],
            first_refusal: self.first_refusal,
            exceeded: self.exceeded,
        }
    }

    /// A representable aggregate above its target is recorded on this
    /// account with its truthful high water and published; the charge is
    /// accepted and no latch is set.
    fn exceeded(&mut self, axis: Axis, requested: usize) {
        let (resource, target) = match axis {
            Axis::Items => (ITEMS_RESOURCE, self.item_limit),
            Axis::Bytes => (BYTES_RESOURCE, self.byte_limit),
        };
        if requested > target
            && self
                .exceeded
                .exceeded(resource, target as u64, requested as u64, None)
        {
            exceedance::publish(
                exceedance::Account::Runner,
                self.identity.map_or(0, NonZeroU64::get),
                &self.exceeded,
            );
        }
    }

    fn refuse(&mut self, axis: Axis, cause: Cause, requested: Option<usize>) -> Error {
        if self.first_refusal.is_none() {
            self.first_refusal = Some(Refusal {
                axis,
                cause,
                current: [self.items, self.bytes],
                requested,
                limit: match axis {
                    Axis::Items => self.item_limit,
                    Axis::Bytes => self.byte_limit,
                },
            });
        }
        Error::Allocation
    }
}

impl Receipt {
    /// Report this executable owner's retained contribution.
    #[must_use]
    pub const fn usage(&self) -> (usize, usize) {
        self.allocation.usage()
    }
}

impl ImportAllocation for Attempt {
    fn retain_checked(&mut self, items: Option<usize>, bytes: Option<usize>) -> Result<(), Error> {
        let mut state = lock(&self.account);
        // A checked-arithmetic refusal is terminal for the whole shared
        // account. Under the state lock, every later admission (a small charge,
        // a fresh attempt, observation or reconstruction sharing the account)
        // is refused without charging and the first cause is preserved; only
        // cleanup, which releases through `Drop`, still proceeds.
        if state.first_refusal.is_some() {
            return Err(Error::Allocation);
        }
        let items = items.ok_or_else(|| state.refuse(Axis::Items, Cause::MissingAmount, None))?;
        let bytes = bytes.ok_or_else(|| state.refuse(Axis::Bytes, Cause::MissingAmount, None))?;
        let owned_items = self.items.checked_add(items).ok_or_else(|| {
            let requested = state.items.checked_add(items);
            state.refuse(Axis::Items, Cause::OwnedOverflow, requested)
        })?;
        let owned_bytes = self.bytes.checked_add(bytes).ok_or_else(|| {
            let requested = state.bytes.checked_add(bytes);
            state.refuse(Axis::Bytes, Cause::OwnedOverflow, requested)
        })?;
        let total_items = state
            .items
            .checked_add(items)
            .ok_or_else(|| state.refuse(Axis::Items, Cause::AggregateOverflow, None))?;
        let total_bytes = state
            .bytes
            .checked_add(bytes)
            .ok_or_else(|| state.refuse(Axis::Bytes, Cause::AggregateOverflow, None))?;
        // The item and byte targets are accounting; crossing one is
        // recorded with the truthful aggregate and the charge is accepted.
        state.exceeded(Axis::Items, total_items);
        state.exceeded(Axis::Bytes, total_bytes);
        state.items = total_items;
        state.bytes = total_bytes;
        state.peak[0] = state.peak[0].max(total_items);
        state.peak[1] = state.peak[1].max(total_bytes);
        self.items = owned_items;
        self.bytes = owned_bytes;
        Ok(())
    }
}

impl Drop for Attempt {
    fn drop(&mut self) {
        let mut state = lock(&self.account);
        // Only this uncloneable scope can release its accepted contribution.
        state.items -= self.items;
        state.bytes -= self.bytes;
    }
}

impl fmt::Debug for Attempt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Attempt")
            .field("items", &self.items)
            .field("bytes", &self.bytes)
            .field("retains_native", &self.baseline.is_some())
            .finish_non_exhaustive()
    }
}

const fn baseline_bytes() -> usize {
    // Carrier reports its inline storage already. Include the Native handle,
    // shared baseline storage and its Arc counters exactly once.
    std::mem::size_of::<Native>() + std::mem::size_of::<Baseline>() - std::mem::size_of::<Carrier>()
        + 2 * std::mem::size_of::<usize>()
}

const fn shared_bytes() -> usize {
    // The same two-counter Arc allocation convention used by facade proofs.
    std::mem::size_of::<Mutex<State>>() + 2 * std::mem::size_of::<usize>()
}

fn lock<T>(account: &Mutex<T>) -> MutexGuard<'_, T> {
    match account.lock() {
        Ok(state) => state,
        Err(poisoned) => poisoned.into_inner(),
    }
}
