//! Ownership carriers independent of borrowed native access.

use super::{failure, Access};
use crate::RawrResultOf;
use std::any::Any;
use std::sync::{Mutex, MutexGuard, TryLockError};

/// A native ownership strategy. Generated arena strategies retain non-Send
/// values on their existing owner thread; ordinary Send values can live here.
pub trait Owner<T: 'static>: crate::func::SendSync + 'static {
    /// The opaque carrier, which can be held by an interpreter handle.
    type Stored: Any + crate::func::SendSync;
    /// The same ownership strategy for a vector of these values.
    type Vector: Owner<Vec<T>>;

    /// Transfer actual native ownership into its carrier.
    fn store(value: T) -> RawrResultOf<Self::Stored>;
    /// Reach the carrier while its value is borrowed for one operation.
    fn access<R>(
        stored: &Self::Stored,
        mutable: bool,
        visit: impl FnOnce(Access<'_, T>) -> RawrResultOf<R>,
    ) -> RawrResultOf<R>;
    /// Move ownership out once, invalidating all aliases of the carrier.
    fn take(stored: &Self::Stored) -> RawrResultOf<T>;
}

/// Direct ownership for a native Send value; sharing the handle grants no
/// simultaneous access to a merely Send, non-Sync referent.
pub struct Native;

/// A native value, present until its ownership is moved out.
pub struct Stored<T>(Mutex<Option<T>>);

fn lock<T>(value: &Mutex<T>) -> RawrResultOf<MutexGuard<'_, T>> {
    match value.try_lock() {
        Ok(value) => Ok(value),
        Err(TryLockError::Poisoned(error)) => {
            value.clear_poison();
            Ok(error.into_inner())
        }
        Err(TryLockError::WouldBlock) => Err(failure("the owned native place is already borrowed")),
    }
}

impl<T: Send + 'static> Owner<T> for Native {
    type Stored = Stored<T>;
    type Vector = Self;

    fn store(value: T) -> RawrResultOf<Self::Stored> {
        Ok(Stored(Mutex::new(Some(value))))
    }

    fn access<R>(
        stored: &Self::Stored,
        mutable: bool,
        visit: impl FnOnce(Access<'_, T>) -> RawrResultOf<R>,
    ) -> RawrResultOf<R> {
        let mut value = lock(&stored.0)?;
        let value = value
            .as_mut()
            .ok_or_else(|| failure("the owned native place has already moved"))?;
        visit(if mutable {
            Access::Mutable(value)
        } else {
            Access::Shared(value)
        })
    }

    fn take(stored: &Self::Stored) -> RawrResultOf<T> {
        lock(&stored.0)?
            .take()
            .ok_or_else(|| failure("the owned native place has already moved"))
    }
}
