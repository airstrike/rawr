//! Rawr's `Option<T>` is Rust's prelude enum, a builtin generic until Rawr
//! has generics of its own.
//!
//! At runtime an option is one `Option<Dynamic>`: `None`, or `Some` of the
//! Rawr value of `T`. A host that takes or returns Rust's `Option<T>` reads
//! and writes it with these functions, as `on_press_maybe` does.

use crate::types::dynamic::Variant;
use crate::Dynamic;
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

/// The runtime representation of a Rawr `Option<T>`.
pub type Value = Option<Dynamic>;

/// `Some(value)`.
pub fn some(value: Dynamic) -> Dynamic {
    Dynamic::from(Some(value))
}

/// `None`.
pub fn none() -> Dynamic {
    Dynamic::from(Value::None)
}

/// Rust's `Option<T>` as a Rawr option of the Rawr value of `T`.
pub fn from<T: Variant + Clone>(value: Option<T>) -> Dynamic {
    Dynamic::from(value.map(Dynamic::from))
}

/// Whether `value` is a Rawr option.
#[must_use]
pub fn is(value: &Dynamic) -> bool {
    value.is::<Value>()
}

/// The option `value` holds, leaving `()` behind; `None` when `value` is not
/// an option.
pub fn take(value: &mut Dynamic) -> Option<Value> {
    if !is(value) {
        return None;
    }
    value.take().try_cast::<Value>()
}

/// Rust's `Option<T>` from a Rawr option whose `Some` holds a `T`; `None`
/// when `value` is not such an option.
#[must_use]
pub fn cast<T: Variant + Clone>(value: Dynamic) -> Option<Option<T>> {
    match value.try_cast::<Value>()? {
        None => Some(None),
        Some(inner) => inner.try_cast::<T>().map(Some),
    }
}

/// A copy of the option `value` holds; `None` when it is not an option.
pub(crate) fn read(value: &Dynamic) -> Option<Value> {
    value.read_lock::<Value>().map(|option| option.clone())
}

#[cfg(test)]
mod tests {
    use crate::Dynamic;

    #[test]
    fn options_round_trip_to_rust() {
        let value = super::from(Some(7_i64));
        assert!(super::is(&value));
        assert_eq!(super::cast::<i64>(value), Some(Some(7)));
        assert_eq!(super::cast::<i64>(super::none()), Some(None));
        assert_eq!(super::cast::<i64>(Dynamic::from(7_i64)), None);
        let mut nested = super::some(super::some(Dynamic::UNIT));
        let outer = super::take(&mut nested).unwrap().unwrap();
        assert!(super::read(&outer).unwrap().unwrap().is_unit());
    }
}
