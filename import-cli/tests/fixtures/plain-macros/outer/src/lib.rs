//! The program's only dependency. It re-exports `inner`'s macros, whose
//! `$crate` is `inner`, a crate the program does not name, and std's
//! `IntoIterator`, whose module is private, and has macros of its own: one
//! whose `$crate` path is a hidden item, and one whose block declares a
//! module.

pub use inner::{counter, list, measured, scaled, total, written, List, Measure};
pub use std::iter::IntoIterator;

#[doc(hidden)]
pub use inner::__private;

#[doc(hidden)]
pub fn __offset() -> i64 {
    100
}

/// The value plus a hundred.
#[macro_export]
macro_rules! offset {
    ($value:expr) => {
        $value + $crate::__offset()
    };
}

/// The value plus two hundred, through a hidden macro of this crate that
/// the program never names.
#[macro_export]
macro_rules! offset_twice {
    ($value:expr) => {
        $crate::__added!($value, 200)
    };
}

/// How many values it is given, counted in a type its block's module
/// declares.
#[macro_export]
macro_rules! tally {
    ($($value:expr),* $(,)?) => {{
        mod __tally {
            pub(super) type Count = u32;
        }
        let mut count: __tally::Count = 0;
        $(
            let _ = $value;
            count += 1;
        )*
        count
    }};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __added {
    ($value:expr, $by:expr) => {
        $value + $by
    };
}
