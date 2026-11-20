//! Rust's `Debug` text of host values, for `{:?}` of a facade type that
//! provides `Debug`.
//!
//! A facade names each such type once with [`register`]. Formatting a value
//! of an unnamed type falls back to its type name.

use std::any::{Any, TypeId};
use std::collections::BTreeMap;
use std::sync::{OnceLock, RwLock};

type Format = fn(&dyn Any) -> Option<String>;

fn formats() -> &'static RwLock<BTreeMap<TypeId, Format>> {
    static FORMATS: OnceLock<RwLock<BTreeMap<TypeId, Format>>> = OnceLock::new();
    FORMATS.get_or_init(RwLock::default)
}

/// Print values of `T` with its `Debug` implementation.
pub fn register<T: std::fmt::Debug + 'static>() {
    fn format<T: std::fmt::Debug + 'static>(value: &dyn Any) -> Option<String> {
        value.downcast_ref::<T>().map(|value| format!("{value:?}"))
    }
    if let Ok(mut formats) = formats().write() {
        formats.entry(TypeId::of::<T>()).or_insert(format::<T>);
    }
}

/// The `Debug` text of `value`, when its type was registered.
pub(crate) fn text(value: &dyn Any) -> Option<String> {
    let format = *formats().read().ok()?.get(&value.type_id())?;
    format(value)
}
