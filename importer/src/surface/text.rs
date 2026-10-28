//! A crate trait's methods on Rawr's string, from the trait's impl for
//! `&str`, as iced's `impl Widget<..> for &str` gives the
//! `Widget::_boxed("label")` that `column!` writes. Rawr's string has no
//! facade type to hold them, so a facade declares each as a function
//! beside the trait, whose first input is the string, and Rawr calls it for
//! `Trait::method(string, ..)`. The runner takes the string as `&'static
//! str`, pinned for its life.

use crate::classify::representability::{convert, Representability};

/// The path a facade declares the method `member` of the trait at
/// `trait_path` under, for a string receiver: `Widget::__rawr_str::_boxed`
/// for `Widget::_boxed`.
#[must_use]
pub fn path(trait_path: &str, member: &str) -> String {
    format!("{trait_path}::__rawr_str::{member}")
}

/// Whether `representability` is a method of a trait's impl for `&str`,
/// which takes Rawr's string as its receiver.
#[must_use]
pub fn is_member(representability: &Representability) -> bool {
    matches!(
        representability,
        Representability::Invocation(plan)
            if plan.signature().instance()
                == Some(&convert::Type::Builtin(convert::Builtin::StaticStr))
    )
}
