//! Rust's `Display` text of host values, for `to_string()` and `{}` of an
//! imported type that implements `Display`.
//!
//! The generated bridge names its handle type once with [`register`].
//! Formatting a value of an unnamed type falls back to its type name.

use std::any::{Any, TypeId};

/// Print values of `T` with its `Display` implementation.
pub fn register<T: std::fmt::Display + 'static>() {
    fn format<T: std::fmt::Display + 'static>(value: &dyn Any) -> Option<String> {
        value.downcast_ref::<T>().map(ToString::to_string)
    }
    super::value::declare(TypeId::of::<T>(), |declared| {
        declared.display.get_or_insert(format::<T>);
    });
}

/// The `Display` text of `value`, when its type was registered.
pub(crate) fn text(value: &dyn Any) -> Option<String> {
    super::value::declared(value.type_id())?.display?(value)
}
