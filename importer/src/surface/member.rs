//! A crate trait's methods on a program's own type that implements the
//! trait, as `column!`'s `Widget::_boxed(circle)` calls the provided
//! `_boxed` of a script's `impl Widget<..> for Circle`. No impl of the
//! crate's has the script type as its `Self`, so a facade declares each
//! method beside the trait as a function generic over the trait's `Self`,
//! whose first input is the receiver, and Rawr calls it for
//! `Trait::method(value, ..)`. The runner calls it as `Trait::<..>::method`
//! on the script type's proxy, which implements the trait through the
//! script's impl (D627).

use super::identity::{Definition, Implementation};

/// The path a facade declares the method `member` of the trait at
/// `trait_path` under, for a receiver of a program's own type:
/// `Widget::__rawr_self::_boxed` for `Widget::_boxed`.
#[must_use]
pub fn path(trait_path: &str, member: &str) -> String {
    format!("{trait_path}::__rawr_self::{member}")
}

/// Whether `implementation` holds the methods of the trait whose identity
/// is `trait_identity` over its `Self`.
#[must_use]
pub fn is_members(implementation: Implementation, trait_identity: &Definition) -> bool {
    implementation == Implementation::self_members(trait_identity)
}
