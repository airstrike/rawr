//! Rust's `Debug` text of host values, for `{:?}` of a facade type that
//! provides `Debug`.
//!
//! A facade names each such type once with [`register`]. Formatting a value
//! of an unnamed type falls back to its type name.

use std::any::{Any, TypeId};

/// Print values of `T` with its `Debug` implementation.
pub fn register<T: std::fmt::Debug + 'static>() {
    fn format<T: std::fmt::Debug + 'static>(value: &dyn Any) -> Option<String> {
        value.downcast_ref::<T>().map(|value| format!("{value:?}"))
    }
    super::value::declare(TypeId::of::<T>(), |declared| {
        declared.debug.get_or_insert(format::<T>);
    });
}

/// The `Debug` text of `value`, when its type was registered.
pub(crate) fn text(value: &dyn Any) -> Option<String> {
    super::value::declared(value.type_id())?.debug?(value)
}
