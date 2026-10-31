//! A place below another: a native element named by its root place and
//! the indexes down to it (D651).
//!
//! The projection holds no reference to the element. Each operation claims
//! the root through its own loan or owner and walks the path inside that
//! claim, so the element is borrowed only for the operation. A projection
//! therefore lives as long as Rust's borrow of the element may, crossing
//! other borrows' lifetimes freely, and expires with its root's loan: once
//! the callback that lent the root returns, every projection of it is
//! refused before any native access. The borrow checker keeps the root
//! from changing shape while a borrow of the element lives, so the path
//! names the same element at each use.

use super::{failure, Access, Operation, Place, Policy, Storage};
use crate::host::runtime::Witness;
use crate::{Dynamic, RawrResult, RawrResultOf, Shared};
use std::any::Any;

struct Projection {
    root: Place,
    path: Box<[usize]>,
}

impl Projection {
    fn joined(&self, path: &[usize]) -> Vec<usize> {
        self.path.iter().chain(path).copied().collect()
    }
}

impl Storage for Projection {
    fn invoke(
        &self,
        _witness: &Witness,
        path: &[usize],
        operation: Operation,
        arguments: &mut [Dynamic],
        rest: Option<&mut crate::scope::Rest<'_>>,
    ) -> RawrResult {
        let path = self.joined(path);
        self.root
            .storage
            .invoke(&self.root.witness, &path, operation, arguments, rest)
    }

    fn reach(
        &self,
        _witness: &Witness,
        path: &[usize],
        mutable: bool,
        visit: &mut dyn FnMut(Access<'_, dyn Any>) -> RawrResult,
    ) -> RawrResult {
        let path = self.joined(path);
        self.root
            .storage
            .reach(&self.root.witness, &path, mutable, visit)
    }

    fn copies(&self, path: &[usize]) -> bool {
        self.root.storage.copies(&self.joined(path))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn of(place: &Place) -> Option<&Projection> {
    place.storage.as_any().downcast_ref::<Projection>()
}

/// Whether `place` is an element below another place.
pub(super) fn is(place: &Place) -> bool {
    of(place).is_some()
}

/// Whether two projections name the same element of the same root.
pub(super) fn same(left: &Place, right: &Place) -> bool {
    match (of(left), of(right)) {
        (Some(left), Some(right)) => left.path == right.path && left.root.same_storage(&right.root),
        _ => false,
    }
}

pub(super) fn project(place: &Place, at: usize) -> RawrResultOf<Place> {
    let witness = place
        .witness
        .sequence_element()
        .ok_or_else(|| failure("only a native sequence has elements"))?;
    let length = place
        .invoke(Operation::Len, &mut [])?
        .as_int()
        .ok()
        .and_then(|length| usize::try_from(length).ok())
        .ok_or_else(|| failure("a native sequence length is not a usize"))?;
    if at >= length {
        return Err(crate::EvalAltResult::ErrorArrayBounds(
            length,
            crate::INT::try_from(at).unwrap_or(crate::INT::MAX),
            crate::Position::NONE,
        )
        .into());
    }
    let (root, path) = match of(place) {
        Some(parent) => (parent.root.clone(), parent.joined(&[at])),
        None => (place.clone(), vec![at]),
    };
    Ok(Place {
        witness,
        storage: Shared::new(Projection {
            root,
            path: path.into_boxed_slice(),
        }),
        mutable: place.mutable,
    })
}

/// Reach a projected element as `P`'s value, inside one claim of its root.
pub(super) fn reach<P: Policy, R>(
    place: &Place,
    mutable: bool,
    visit: impl FnOnce(Access<'_, P::Value>) -> RawrResultOf<R>,
) -> RawrResultOf<R> {
    let projection = of(place).ok_or_else(|| failure("the native place is not an element"))?;
    let mut visit = Some(visit);
    let mut result = None;
    let mismatch = || failure("a native element has another concrete type");
    let _ = projection.root.storage.reach(
        &projection.root.witness,
        &projection.path,
        mutable,
        &mut |value| {
            let visit = visit
                .take()
                .ok_or_else(|| failure("a native element was reached twice"))?;
            result = Some(match value {
                Access::Shared(value) => {
                    visit(Access::Shared(value.downcast_ref().ok_or_else(mismatch)?))
                }
                Access::Mutable(value) => {
                    visit(Access::Mutable(value.downcast_mut().ok_or_else(mismatch)?))
                }
            });
            Ok(Dynamic::UNIT)
        },
    )?;
    result.ok_or_else(|| failure("a native element was not reached"))?
}
