//! A crate whose macros name std's built-in macros through its own
//! re-export, as tracing's `$crate::__macro_support::line!()` does.

/// Functions that format what they are lent, as tracing's `ValueSet`
/// formats its `&dyn Value`s and log's `Record` its `fmt::Arguments`.
pub mod sink {
    use std::fmt;

    pub fn debug(value: &dyn fmt::Debug) -> String {
        format!("{value:?}")
    }

    pub fn display(value: &dyn fmt::Display) -> String {
        format!("[{value}]")
    }

    pub fn both(name: &str, value: &dyn fmt::Display, detail: &dyn fmt::Debug) -> String {
        format!("{name}={value} ({detail:?})")
    }

    pub fn hex(value: &dyn fmt::LowerHex) -> String {
        format!("{value:#x}")
    }

    pub fn arguments(arguments: fmt::Arguments<'_>) -> String {
        format!("<{arguments}>")
    }

    /// Whether it is asked to format at all, as a disabled event is not.
    pub fn unread(_value: &dyn fmt::Debug) -> &'static str {
        "unread"
    }
}

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

/// Its caller's expression and where it is, through a nested call of
/// [`here!`], as tracing's `event!` reaches `callsite!` and `valueset!`.
#[macro_export]
macro_rules! traced {
    ($value:expr) => {
        $crate::__support::format_args!("{} at {}", $value, $crate::here!()).to_string()
    };
}
