//! The program's only dependency. It re-exports `inner`'s macros, whose
//! `$crate` is `inner`, a crate the program does not name, and has a macro
//! of its own whose `$crate` path is a hidden item.

pub use inner::{list, measured, scaled, total, written, List, Measure};

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

#[doc(hidden)]
#[macro_export]
macro_rules! __added {
    ($value:expr, $by:expr) => {
        $value + $by
    };
}
