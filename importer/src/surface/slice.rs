//! A crate trait's members on a slice, from a blanket impl over a borrow of
//! its own type applied through an impl of its bound for `&[T]`, as rayon's
//! `IntoParallelRefIterator` gives `[T]` its `par_iter` (`classify::slice`).
//! A slice has no facade type to hold them, so a facade declares each as a
//! function beside the trait, whose first input is the slice's borrow, and
//! Rawr calls it for `items.par_iter()` with the trait in scope. The runner
//! calls it through the trait, `<[_] as Trait>::member(slice)`, and lends
//! the slice for as long as the output borrows it.

use crate::classify::representability::{convert, Representability};

/// The path a facade declares the member `member` of the trait at
/// `trait_path` under, for a slice: `IntoParallelRefIterator::__rawr_slice::
/// par_iter` for `<[T] as IntoParallelRefIterator>::par_iter`.
#[must_use]
pub fn path(trait_path: &str, member: &str) -> String {
    format!("{trait_path}::__rawr_slice::{member}")
}

/// The instance a slice member is called at: `[_]`, whose element the
/// call's slice gives.
#[must_use]
pub fn instance() -> convert::Type {
    convert::Type::Sequence {
        element: Box::new(convert::Type::Inferred),
        owned: false,
        length: None,
    }
}

/// Whether `ty` is a slice member's instance (`instance`).
#[must_use]
pub fn is_instance(ty: &convert::Type) -> bool {
    matches!(ty, convert::Type::Sequence {
        element,
        owned: false,
        length: None,
    } if matches!(**element, convert::Type::Inferred))
}

/// Whether `representability` is a trait's member on a slice, called
/// through the trait (`path`).
#[must_use]
pub fn is_member(representability: &Representability) -> bool {
    let signature = match representability {
        Representability::Function(plan) => plan.signature(),
        Representability::FunctionItem(item) => item.signature(),
        _ => return false,
    };
    signature.through().is_some() && signature.instance().is_some_and(is_instance)
}
