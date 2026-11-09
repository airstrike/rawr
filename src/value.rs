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
//! feature), `'static`, `Unpin`, `Clone`, `Debug` and `Display`, the last
//! two rendered by the program when native code formats the value, as a
//! `&dyn Debug` or `&dyn Display` it was lent. Bounds that pick
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
    /// value belongs to, and else the value's own (review 8): an `Option`,
    /// a `Result` or an array as std writes one, each value in it by this
    /// `Debug`, as a `&dyn Debug` of `Some(message)` is written.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(text) = crate::future::debug(&self.0) {
            return f.write_str(&text);
        }
        if let Some(option) = self.0.read_lock::<Option<Dynamic>>() {
            return match &*option {
                Some(value) => f.debug_tuple("Some").field(&Self(value.clone())).finish(),
                None => f.write_str("None"),
            };
        }
        if let Some(result) = self.0.read_lock::<Result<Dynamic, Dynamic>>() {
            let (variant, value) = match &*result {
                Ok(value) => ("Ok", value),
                Err(value) => ("Err", value),
            };
            return f.debug_tuple(variant).field(&Self(value.clone())).finish();
        }
        if let Ok(values) = self.0.as_array_ref() {
            return f
                .debug_list()
                .entries(values.iter().cloned().map(Self))
                .finish();
        }
        std::fmt::Debug::fmt(&self.0, f)
    }
}

impl std::fmt::Display for Value {
    /// The script type's own `Display` text, through the program the value
    /// belongs to, and else the value's own, as a number's or a string's:
    /// rendered only when native code formats it, as a `&dyn Display` it
    /// was lent.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        #[cfg(feature = "sync")]
        if let Some(text) = crate::future::display(&self.0) {
            return f.write_str(&text.map_err(|_| std::fmt::Error)?);
        }
        std::fmt::Display::fmt(&self.0, f)
    }
}

#[cfg(test)]
mod tests {
    use super::Value;

    #[test]
    fn value_has_the_closed_trait_set() {
        fn traits<T: Clone + std::fmt::Debug + std::fmt::Display + Unpin + 'static>() {}
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
        assert_eq!(format!("{}", Value::from(crate::Dynamic::from(3_i64))), "3");
        let some = crate::Dynamic::from(Some(crate::Dynamic::from(crate::ImmutableString::from(
            "a",
        ))));
        let none = crate::Dynamic::from(None::<crate::Dynamic>);
        let list = crate::Dynamic::from_array(vec![some, none]);
        assert_eq!(format!("{:?}", Value::from(list)), r#"[Some("a"), None]"#);
    }
}
