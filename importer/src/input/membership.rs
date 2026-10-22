//! Pinned physical-source selection over bounded, capability-free observations.
//!
//! An observation image and an archive projection are data, not current source
//! authority. Only the full contained selector, actual cold Cargo success, and
//! equal before/after determinants can eventually authorize package replay.

mod archive;
mod document;
mod git_read;
mod host;
mod human;
mod live;
mod manifest;
mod observed;
mod rejection;
mod stored;

#[cfg(test)]
mod tests;

#[cfg(test)]
thread_local! {
    static OWNED_VECTORS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static OWNED_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

use std::collections::TryReserveError;

use super::Allocation;
use crate::node;

const CARGO_COMMIT: &str = "59800466c5c41c444d264b1010b4d57e85a7117f";

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error(transparent)]
    Input(#[from] crate::Error),
    #[error(transparent)]
    Budget(#[from] node::Error),
    #[error("membership allocation failed")]
    Allocation(#[from] TryReserveError),
    #[error("membership inline-vector allocation failed: {0:?}")]
    InlineAllocation(smallvec::CollectionAllocErr),
    #[error("invalid membership image: {0}")]
    Encoding(&'static str),
    #[error("membership computation requested an unrecorded observation")]
    Unobserved,
    #[error("membership computation did not consume the complete observation image")]
    Unconsumed,
    #[error("membership observation has the wrong result type")]
    ObservationType,
    #[error("membership source observation failed with OS error {0}")]
    Io(i32),
    #[error("membership manifest is not valid UTF-8: {0}")]
    ManifestEncoding(#[from] std::str::Utf8Error),
    #[error("membership TOML allocation failed")]
    DocumentAllocation,
    #[error("membership TOML storage layout overflowed")]
    DocumentOverflow,
    #[error("invalid membership TOML: {0:?}")]
    Document(rawr_toml::document::Diagnostic),
    #[error("membership source rejected: {0}")]
    Source(#[from] archive::Rejection),
    #[error("Cargo package output differs from its expected archive: {0}")]
    Output(#[from] human::Mismatch),
}

type Result<T> = std::result::Result<T, Error>;

/// The caller's original cumulative ownership and graph-work accounts.
struct Budget<'budget> {
    graph: &'budget node::GraphBudget,
    allocation: &'budget mut Allocation,
}

impl Budget<'_> {
    /// Own one string. Its size is an accounting target recorded on the
    /// original account; the checked charge in `bytes` is the admission.
    fn string(&mut self, value: &[u8]) -> Result<Vec<u8>> {
        self.allocation
            .string_exceeded("membership owned string bytes", value.len());
        self.bytes(value)
    }

    fn sort<T>(&self, values: &mut [T], compare: impl Fn(&T, &T) -> std::cmp::Ordering) {
        // Every caller owns entries admitted through reserve(). Heapsort uses
        // constant scratch and at most O(n log n) comparisons over that admitted
        // item/byte envelope; pure comparisons are not filesystem path checks.
        fn sift<T>(
            values: &mut [T],
            mut parent: usize,
            compare: &impl Fn(&T, &T) -> std::cmp::Ordering,
        ) {
            while let Some(left) = parent.checked_mul(2).and_then(|value| value.checked_add(1)) {
                if left >= values.len() {
                    break;
                }
                let mut larger = left;
                if left + 1 < values.len() && compare(&values[left], &values[left + 1]).is_lt() {
                    larger += 1;
                }
                if !compare(&values[parent], &values[larger]).is_lt() {
                    break;
                }
                values.swap(parent, larger);
                parent = larger;
            }
        }
        for parent in (0..values.len() / 2).rev() {
            sift(values, parent, &compare);
        }
        for end in (1..values.len()).rev() {
            values.swap(0, end);
            sift(&mut values[..end], 0, &compare);
        }
    }

    fn reserve<T>(&mut self, values: &mut Vec<T>, additional: usize) -> Result<()> {
        let bytes = additional
            .checked_mul(std::mem::size_of::<T>())
            .ok_or(Error::Encoding("allocation length overflow"))?;
        self.allocation.items(additional)?;
        self.allocation.bytes(bytes)?;
        values.try_reserve_exact(additional)?;
        #[cfg(test)]
        OWNED_VECTORS.with(|count| count.set(count.get() + 1));
        Ok(())
    }

    fn bytes(&mut self, value: &[u8]) -> Result<Vec<u8>> {
        let mut owned = Vec::new();
        self.allocation.items(1)?;
        self.allocation.bytes(value.len())?;
        owned.try_reserve_exact(value.len())?;
        #[cfg(test)]
        OWNED_BYTES.with(|count| count.set(count.get() + 1));
        owned.extend_from_slice(value);
        Ok(owned)
    }
}
