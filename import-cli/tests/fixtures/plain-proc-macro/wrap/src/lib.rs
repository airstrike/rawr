//! A library that re-exports a proc macro crate's macro, as
//! `rust_decimal` re-exports `dec!`.

pub use numerals::double;

#[doc(hidden)]
pub use numerals as __numerals;

/// `seven!()`: 7.
#[macro_export]
macro_rules! seven {
    () => {
        7
    };
}

/// `relayed!()`: `seven!()` through `numerals::verbatim!`, each named
/// from `$crate`, as a crate's macro that calls its proc macro with its
/// own paths does.
#[macro_export]
macro_rules! relayed {
    () => {
        $crate::__numerals::verbatim!($crate::seven!())
    };
}
