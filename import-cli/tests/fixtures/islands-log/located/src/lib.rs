//! A crate whose macros name std's built-in macros through its own
//! re-export, as tracing's `$crate::__macro_support::line!()` does.

#[doc(hidden)]
pub mod __support {
    pub use core::{column, concat, file, format_args, line, module_path};
}

/// Where the call is: its file, line, column and module, joined.
#[macro_export]
macro_rules! here {
    () => {
        $crate::__support::format_args!(
            "{}:{}:{} in {}",
            $crate::__support::file!(),
            $crate::__support::line!(),
            $crate::__support::column!(),
            $crate::__support::module_path!()
        )
        .to_string()
    };
}

/// The call's module and line as one literal, as tracing's
/// `concat!("event ", file!(), ":", line!())` names its callsite.
#[macro_export]
macro_rules! name {
    () => {
        $crate::__support::concat!(
            "event ",
            $crate::__support::module_path!(),
            " at line ",
            $crate::__support::line!()
        )
    };
}
