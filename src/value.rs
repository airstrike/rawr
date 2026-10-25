//! [`Value`]: a script value where native generic code needs a Rust type
//! (D513.2).
//!
//! A generic Rust item that a script instantiates at one of its own types,
//! such as `mpsc::channel::<Job>()`, is built once in the runner at
//! `Value`. Rust checks the item's body against `Value`'s traits, and Rawr
//! checks each bound against the script type itself, so `Value` only
//! carries and dispatches. A value that comes back out is checked against
//! the script type it stands for.
//!
//! Its traits are a closed set: `Send` and `Sync` (with the crate's `sync`
//! feature), `'static`, `Unpin`, `Clone` and `Debug`. Bounds that pick
//! behaviour by type alone, such as `Default`, `Deserialize`, `Copy` or a
//! `TypeId` key, have no meaning here and are refused before the runner is
//! built.

use crate::Dynamic;

/// A script value standing for a script type in native generic code.
#[repr(transparent)]
pub struct Value(Dynamic);

impl Clone for Value {
    /// The script type's own `Clone` where the program wrote one, as
    /// `T::clone` calls it, and else a copy, as a derived `Clone` makes
    /// (review 8). A hand-written `clone` that fails panics, as in Rust.
    fn clone(&self) -> Self {
        #[cfg(all(feature = "std", not(feature = "no_function")))]
        if let Some(copy) = crate::future::clone_value(&self.0) {
            return match copy {
                Ok(copy) => Self(copy),
                Err(error) => panic!("{}", crate::future::message(&error)),
            };
        }
        Self(self.0.clone())
    }
}

impl Value {
    /// The script value.
    pub fn into_inner(self) -> Dynamic {
        self.0
    }

    /// The script value, borrowed.
    pub const fn get(&self) -> &Dynamic {
        &self.0
    }
}

impl From<Dynamic> for Value {
    fn from(value: Dynamic) -> Self {
        Self(value)
    }
}

impl From<Value> for Dynamic {
    fn from(value: Value) -> Self {
        value.0
    }
}

impl std::fmt::Debug for Value {
    /// The script type's derived `Debug` text, through the program the
    /// value belongs to, and else the value's own (review 8).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        #[cfg(all(feature = "std", not(feature = "no_function")))]
        if let Some(text) = crate::future::debug(&self.0) {
            return f.write_str(&text);
        }
        std::fmt::Debug::fmt(&self.0, f)
    }
}

#[cfg(test)]
mod tests {
    use super::Value;

    #[test]
    fn value_has_the_closed_trait_set() {
        fn traits<T: Clone + std::fmt::Debug + Unpin + 'static>() {}
        traits::<Value>();
        #[cfg(feature = "sync")]
        {
            fn shared<T: Send + Sync>() {}
            shared::<Value>();
        }
        assert_eq!(
            format!("{:?}", Value::from(crate::Dynamic::from(3_i64))),
            "3"
        );
    }
}
