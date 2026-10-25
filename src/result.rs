//! Rawr's `Result<T, E>` is Rust's prelude enum, a builtin generic until
//! Rawr has generics of its own, built as `Option<T>` is.
//!
//! At runtime a result is one `Result<Dynamic, Dynamic>`: `Ok` of the Rawr
//! value of `T`, or `Err` of the Rawr value of `E`. A host that takes or
//! returns Rust's `Result<T, E>` reads and writes it with these functions.

use crate::types::dynamic::Variant;
use crate::Dynamic;

/// The runtime representation of a Rawr `Result<T, E>`.
pub type Value = Result<Dynamic, Dynamic>;

/// `Ok(value)`.
pub fn ok(value: Dynamic) -> Dynamic {
    Dynamic::from(Value::Ok(value))
}

/// `Err(error)`.
pub fn err(error: Dynamic) -> Dynamic {
    Dynamic::from(Value::Err(error))
}

/// Rust's `Result<T, E>` as a Rawr result of the Rawr values of `T` and `E`.
pub fn from<T: Variant + Clone, E: Variant + Clone>(value: Result<T, E>) -> Dynamic {
    Dynamic::from(value.map(Dynamic::from).map_err(Dynamic::from))
}

/// Whether `value` is a Rawr result.
#[must_use]
pub fn is(value: &Dynamic) -> bool {
    value.is::<Value>()
}

/// The result `value` holds, leaving `()` behind; `None` when `value` is not
/// a result.
pub fn take(value: &mut Dynamic) -> Option<Value> {
    if !is(value) {
        return None;
    }
    value.take().try_cast::<Value>()
}

/// Rust's `Result<T, E>` from a Rawr result whose `Ok` holds a `T` and whose
/// `Err` holds an `E`; `None` when `value` is not such a result.
#[must_use]
pub fn cast<T: Variant + Clone, E: Variant + Clone>(value: Dynamic) -> Option<Result<T, E>> {
    match value.try_cast::<Value>()? {
        Ok(inner) => inner.try_cast::<T>().map(Ok),
        Err(inner) => inner.try_cast::<E>().map(Err),
    }
}

/// A copy of the result `value` holds; `None` when it is not a result.
pub(crate) fn read(value: &Dynamic) -> Option<Value> {
    value.read_lock::<Value>().map(|result| result.clone())
}

#[cfg(test)]
mod tests {
    use crate::Dynamic;

    #[test]
    fn results_round_trip_to_rust() {
        let value = super::from::<i64, bool>(Ok(7));
        assert!(super::is(&value));
        assert_eq!(super::cast::<i64, bool>(value), Some(Ok(7)));
        let error = super::from::<i64, bool>(Err(true));
        assert_eq!(super::cast::<i64, bool>(error), Some(Err(true)));
        assert_eq!(super::cast::<i64, bool>(Dynamic::from(7_i64)), None);
        assert_eq!(
            super::cast::<i64, bool>(super::err(Dynamic::from(1_i64))),
            None
        );
        let mut nested = super::ok(super::err(Dynamic::UNIT));
        let outer = super::take(&mut nested).unwrap().unwrap();
        assert!(super::read(&outer).unwrap().unwrap_err().is_unit());
        assert!(!super::is(&crate::option::none()));
    }
}
