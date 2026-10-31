//! Typed storage access; all borrowed pointers remain inside `scope`.

use super::ownership::Owner;
use super::{failure, Access, Operation, Place, Policy, Storage};
use crate::host::runtime::Witness;
use crate::{Dynamic, RawrResult, RawrResultOf, Shared};
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::marker::PhantomData;

thread_local! {
    // A read already holding an owned value's mutex on this thread lends
    // its actual shared reference. Reentry may share that loan; no other
    // thread can find it, so a merely Send value never acquires Sync rights.
    static READS: RefCell<HashMap<usize, crate::scope::Nonce>> = RefCell::default();
}

struct Reading(usize);

impl Drop for Reading {
    fn drop(&mut self) {
        READS.with_borrow_mut(|reads| reads.remove(&self.0));
    }
}

struct Family<P>(PhantomData<P>);

impl<P: Policy> crate::scope::Family for Family<P> {
    type Of<'a> = P::Value;
}

enum Source<P: Policy> {
    Borrowed(crate::scope::Nonce),
    Owned(<P::Owner as Owner<P::Value>>::Stored),
}

impl<P: Policy> Source<P> {
    fn access<R>(
        &self,
        mutable: bool,
        visit: impl FnOnce(Access<'_, P::Value>) -> RawrResultOf<R>,
    ) -> RawrResultOf<R> {
        match self {
            Self::Borrowed(nonce) if mutable => {
                crate::scope::with::<Family<P>, _>(*nonce, |value| visit(Access::Mutable(value)))
                    .map_err(|error| failure(error.to_string()))?
            }
            Self::Borrowed(nonce) => {
                crate::scope::with_ref::<Family<P>, _>(*nonce, |value| visit(Access::Shared(value)))
                    .map_err(|error| failure(error.to_string()))?
            }
            Self::Owned(value) => {
                let identity = std::ptr::from_ref(self).addr();
                if let Some(nonce) = READS.with_borrow(|reads| reads.get(&identity).copied()) {
                    return if mutable {
                        Err(failure("the owned native place is shared"))
                    } else {
                        crate::scope::with_ref::<Family<P>, _>(nonce, |value| {
                            visit(Access::Shared(value))
                        })
                        .map_err(|error| failure(error.to_string()))?
                    };
                }
                P::Owner::access(value, mutable, |value| {
                    if mutable {
                        visit(value)
                    } else {
                        crate::scope::lend_ref::<Family<P>, _>(value.read(), |nonce| {
                            READS.with_borrow_mut(|reads| reads.insert(identity, nonce));
                            let _reading = Reading(identity);
                            crate::scope::with_ref::<Family<P>, _>(nonce, |value| {
                                visit(Access::Shared(value))
                            })
                            .map_err(|error| failure(error.to_string()))?
                        })
                    }
                })
            }
        }
    }
}

impl<P: Policy> Storage for Source<P> {
    fn invoke(
        &self,
        witness: &Witness,
        path: &[usize],
        operation: Operation,
        arguments: &mut [Dynamic],
        rest: Option<&mut crate::scope::Rest<'_>>,
    ) -> RawrResult {
        self.access(operation.mutable(), |value| {
            P::project(value, witness, path, operation, arguments, rest)
        })
    }

    fn reach(
        &self,
        _witness: &Witness,
        path: &[usize],
        mutable: bool,
        visit: &mut dyn FnMut(Access<'_, dyn Any>) -> RawrResult,
    ) -> RawrResult {
        self.access(mutable, |value| P::reach_element(value, path, visit))
    }

    fn copies(&self, path: &[usize]) -> bool {
        P::copies_at(path)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// An operation on the value a place names itself, its own element or
/// root, under the claim already held on it.
pub(super) fn leaf<P: Policy>(
    mut value: Access<'_, P::Value>,
    witness: &Witness,
    operation: Operation,
    arguments: &mut [Dynamic],
    rest: Option<&mut crate::scope::Rest<'_>>,
) -> RawrResult {
    if operation == Operation::SwapValues {
        let other = super::argument(arguments, 0)?;
        let other = super::get(&other)
            .ok_or_else(|| failure("a native swap operand must be a checked place"))?;
        return access::<P, _>(&other, witness, |other| {
            std::mem::swap(value.write()?, other);
            Ok(Dynamic::UNIT)
        });
    }
    P::invoke(value, witness, operation, arguments, rest)
}

fn checked<P: Policy>(source: Source<P>, witness: &Witness, mutable: bool) -> RawrResultOf<Place> {
    if !P::accepts(witness) {
        return Err(failure(
            "the native place's concrete type does not match its witness",
        ));
    }
    Ok(Place {
        witness: witness.clone(),
        storage: Shared::new(source),
        mutable,
    })
}

/// Lend native storage mutably for exactly one continuation.
pub fn lend<P: Policy>(
    value: &mut P::Value,
    witness: &Witness,
    rest: &mut crate::scope::Rest<'_>,
) -> RawrResult {
    borrow::<P, _>(value, witness, rest)?
}

/// Lend native storage around a caller whose result has an arbitrary type.
/// The result cannot contain a reference to the loan's native storage.
pub fn borrow<P: Policy, R>(
    value: &mut P::Value,
    witness: &Witness,
    rest: impl FnOnce(Dynamic) -> R,
) -> RawrResultOf<R> {
    crate::scope::lend::<Family<P>, _>(value, |nonce| {
        let place = checked(Source::<P>::Borrowed(nonce), witness, true)?;
        Ok(rest(Dynamic::from(place)))
    })
}

/// Lend native storage shared for exactly one continuation.
pub fn lend_ref<P: Policy>(
    value: &P::Value,
    witness: &Witness,
    rest: &mut crate::scope::Rest<'_>,
) -> RawrResult {
    borrow_ref::<P, _>(value, witness, rest)?
}

/// Shared counterpart of [`borrow`].
pub fn borrow_ref<P: Policy, R>(
    value: &P::Value,
    witness: &Witness,
    rest: impl FnOnce(Dynamic) -> R,
) -> RawrResultOf<R> {
    crate::scope::lend_ref::<Family<P>, _>(value, |nonce| {
        let place = checked(Source::<P>::Borrowed(nonce), witness, false)?;
        Ok(rest(Dynamic::from(place)))
    })
}

/// Transfer a native value into owned storage without changing its allocation.
pub fn owned<P: Policy>(value: P::Value, witness: &Witness) -> RawrResult {
    checked(Source::<P>::Owned(P::Owner::store(value)?), witness, true).map(Dynamic::from)
}

/// Consume a native owned carrier of the exact concrete and logical type.
/// Borrowed places cannot be consumed by this operation.
pub fn take<P: Policy>(value: &Place, witness: &Witness) -> RawrResultOf<P::Value> {
    if !value.witness.same_native_type(witness) || !P::accepts(witness) {
        return Err(failure("a moved native value has the wrong logical type"));
    }
    // Rust moves no value out of a borrowed element, so an element's place
    // reaches an owned operand only where a script copies it, as `.clone()`
    // of a native value copies its handle: the element's own `Clone` makes
    // the owned value, as the arena's `take` of a copied handle does.
    if super::projection::is(value) {
        let cloning = P::cloning()
            .ok_or_else(|| failure("cannot move a native element out of its borrowed place"))?;
        return reach::<P, _>(value, witness, false, |element| {
            Ok((cloning.element)(element.read()))
        });
    }
    let source = value
        .storage
        .as_any()
        .downcast_ref::<Source<P>>()
        .ok_or_else(|| failure("a moved native value has the wrong concrete type"))?;
    match source {
        Source::Borrowed(_) => Err(failure(
            "cannot move a native value out of its borrowed place",
        )),
        Source::Owned(value) => P::Owner::take(value),
    }
}

pub(super) fn access<P: Policy, R>(
    place: &Place,
    witness: &Witness,
    visit: impl FnOnce(&mut P::Value) -> RawrResultOf<R>,
) -> RawrResultOf<R> {
    reach::<P, R>(place, witness, true, |mut value| visit(value.write()?))
}

/// Reach a checked native place for a generated call. The callback cannot
/// return a reference into the claim; its concrete policy and logical witness
/// must both match the authority that originally constructed the place.
pub fn reach<P: Policy, R>(
    place: &Place,
    witness: &Witness,
    mutable: bool,
    visit: impl FnOnce(Access<'_, P::Value>) -> RawrResultOf<R>,
) -> RawrResultOf<R> {
    if !place.witness.same_native_type(witness) || !P::accepts(witness) {
        return Err(failure("the native operand has the wrong logical type"));
    }
    if super::projection::is(place) {
        return super::projection::reach::<P, R>(place, mutable, visit);
    }
    let source = place
        .storage
        .as_any()
        .downcast_ref::<Source<P>>()
        .ok_or_else(|| failure("the native operand has the wrong concrete type"))?;
    source.access(mutable, visit)
}
