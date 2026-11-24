//! iced's `color` module, macro and `border::color` under one name: the
//! module `tint` defines the type the root re-exports, the root exports
//! the macro `tint!`, and `outline::tint` is a function. Rust keeps the
//! three in their own namespaces.

/// A tint, as iced's `Color`, which lives in its module and is
/// re-exported at the root.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Tint {
    /// Its red.
    pub r: u8,
    /// Its green.
    pub g: u8,
    /// Its blue.
    pub b: u8,
}

impl Tint {
    /// The tint packed as `0xRRGGBB`.
    pub fn from_packed(hex: u32) -> Self {
        Tint {
            r: (hex >> 16) as u8,
            g: (hex >> 8) as u8,
            b: hex as u8,
        }
    }
}

/// A tint from a packed literal, as iced's `color!(0xff0000)`.
#[macro_export]
macro_rules! tint {
    ($hex:literal) => {
        $crate::Tint::from_packed($hex)
    };
}
