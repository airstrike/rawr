//! A crate trait's methods on an object of the trait, as `held.0.area()`
//! calls `Shape::area` on the `&'static dyn Shape` a crate value holds
//! (#1593). An object has no facade type to hold them, so a facade
//! declares each `&self` method whose signature names `Self` only in its
//! receiver as a function beside the trait, whose first input is the
//! object, and Rawr calls it for `object.method(..)`. The runner calls it
//! through the trait, `<(dyn Shape) as Shape>::area(object)`, so rustc
//! dispatches it by the object's vtable.

use crate::classify::representability::{convert, Representability};

/// The path a facade declares the method `member` of the trait at
/// `trait_path` under, for its object: `Shape::__rawr_dyn::area` for
/// `<dyn Shape as Shape>::area`.
#[must_use]
pub fn path(trait_path: &str, member: &str) -> String {
    format!("{trait_path}::__rawr_dyn::{member}")
}

/// Whether `representability` is a method of a trait's object, called
/// through the trait (`path`).
#[must_use]
pub fn of(representability: &Representability) -> bool {
    let Representability::FunctionItem(item) = representability else {
        return false;
    };
    let signature = item.signature();
    signature.through().is_some() && matches!(signature.instance(), Some(convert::Type::Object(_)))
}
