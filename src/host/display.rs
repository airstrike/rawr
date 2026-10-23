//! Rust's `Display` text of host values, for `to_string()` and `{}` of an
//! imported type that implements `Display`.
//!
//! The generated bridge names its handle type once with [`register`].
//! Formatting a value of an unnamed type falls back to its type name.

use std::any::{Any, TypeId};
use std::collections::BTreeMap;
use std::sync::{OnceLock, RwLock};

type Format = fn(&dyn Any) -> Option<String>;

fn formats() -> &'static RwLock<BTreeMap<TypeId, Format>> {
    static FORMATS: OnceLock<RwLock<BTreeMap<TypeId, Format>>> = OnceLock::new();
    FORMATS.get_or_init(RwLock::default)
}

/// Print values of `T` with its `Display` implementation.
pub fn register<T: std::fmt::Display + 'static>() {
    fn format<T: std::fmt::Display + 'static>(value: &dyn Any) -> Option<String> {
        value.downcast_ref::<T>().map(ToString::to_string)
    }
    if let Ok(mut formats) = formats().write() {
        formats.entry(TypeId::of::<T>()).or_insert(format::<T>);
    }
}

/// The `Display` text of `value`, when its type was registered.
pub(crate) fn text(value: &dyn Any) -> Option<String> {
    let format = *formats().read().ok()?.get(&value.type_id())?;
    format(value)
}
