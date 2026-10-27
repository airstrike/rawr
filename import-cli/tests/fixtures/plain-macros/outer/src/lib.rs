//! The program's only dependency. It re-exports `inner`'s macros, whose
//! `$crate` is `inner`, a crate the program does not name, and has a macro
//! of its own whose `$crate` path is a hidden item.

pub use inner::{list, scaled, total, List};

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
