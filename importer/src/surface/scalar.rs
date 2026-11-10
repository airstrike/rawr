//! A crate trait's members on a number, `bool` or `char`, from the trait's
//! impl for it, as num-traits' `impl NumCast for u64` gives `u64` its
//! `to_u32` and `from`. A number has no facade type to hold them, so a
//! facade declares each as a function beside the trait, whose first input
//! is the number (or its borrow) for a method, and Rawr calls it for
//! `count.to_u32()` with the trait in scope, or for `<u64 as
//! NumCast>::from(n)`. The runner calls it through the trait, as `u64` has
//! `From::from` too.

use crate::classify::representability::{convert, Representability};

/// The path a facade declares the member `member` of the trait at
/// `trait_path` under, for the scalar `scalar` (its Rust name):
/// `NumCast::__rawr_u64::to_u32` for `<u64 as NumCast>::to_u32`.
#[must_use]
pub fn path(trait_path: &str, scalar: &str, member: &str) -> String {
    format!("{trait_path}::__rawr_{scalar}::{member}")
}

/// The Rust name of the scalar whose impl of a trait `representability`
/// is a member of, called through the trait (`path`). `None` for anything
/// else.
#[must_use]
pub fn of(representability: &Representability) -> Option<&'static str> {
    let Representability::Function(plan) = representability else {
        return None;
    };
    let signature = plan.signature();
    signature.through()?;
    match signature.instance()? {
        convert::Type::Builtin(builtin) => builtin.rust_name(),
        _ => None,
    }
}
