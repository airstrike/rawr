//! A program's value coerced to a trait object of a crate's trait (D872),
//! as `&count as &dyn tracing::field::Value` is.
//!
//! Rawr never makes the object. The value stays the program's, tagged with
//! the type it was coerced from, and the runner's adapter coerces it where
//! native code takes the object, as rustc coerces it: it finds the tag
//! among the crate's impls of the trait and makes that type's Rust value,
//! or reaches the crate value where it is.

use crate::{Dynamic, ImmutableString};

/// A program's value where a crate's trait object is.
#[derive(Clone)]
pub struct Coerced {
    value: Dynamic,
    /// The type it was coerced from, as the importer tags it
    /// (`rawr_import::classify::representability::object::tag`).
    tag: ImmutableString,
}

impl Coerced {
    /// `value`, coerced from the type `tag` names.
    pub(crate) fn new(value: Dynamic, tag: ImmutableString) -> Self {
        Self { value, tag }
    }

    /// The value and its tag, when `value` is a coerced one; `None` for an
    /// object native code gave.
    #[must_use]
    pub fn of(value: &Dynamic) -> Option<(Dynamic, ImmutableString)> {
        value
            .read_lock::<Self>()
            .map(|coerced| (coerced.value.clone(), coerced.tag.clone()))
    }
}
