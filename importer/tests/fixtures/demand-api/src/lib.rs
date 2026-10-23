//! A crate Rawr names only part of.

pub use inner_api;

/// A conversion target with a `From` impl for a type from another crate.
pub struct Element(pub i64);

impl From<inner_api::Deep> for Element {
    fn from(deep: inner_api::Deep) -> Self {
        Self(deep.0.into())
    }
}

impl From<widget::Button> for Element {
    fn from(button: widget::Button) -> Self {
        Self(button.label)
    }
}

pub mod widget {
    pub use self::slider::*;

    pub struct Button {
        pub label: i64,
    }

    impl Button {
        pub fn new(label: i64) -> Self {
            Self { label }
        }
    }

    impl Clone for Button {
        fn clone(&self) -> Self {
            Self { label: self.label }
        }
    }

    pub fn button(label: i64) -> Button {
        Button::new(label)
    }

    pub mod slider {
        pub struct Slider(pub i64);

        impl Slider {
            pub fn value(&self) -> i64 {
                self.0
            }
        }

        pub fn slider(value: i64) -> Slider {
            Slider(value)
        }
    }

    /// Never named by the tests: never extracted.
    pub struct Unnamed;

    impl Unnamed {
        pub fn unnamed() -> Self {
            Self
        }
    }
}

/// A type whose `to_string` comes from std's blanket
/// `impl<T: Display> ToString for T`.
pub struct Label(pub i64);

impl std::fmt::Display for Label {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "label {}", self.0)
    }
}
