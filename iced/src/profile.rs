//! Exact imported-API evidence and runner-local lifecycle for iced.

mod definition;
mod evidence;
pub mod projection;
mod resolution;
#[doc(hidden)]
pub mod runner;
pub mod runtime;

#[cfg(feature = "import-baseline")]
#[doc(hidden)]
pub use definition::Fixture;
pub use definition::{Checked, Definition};
pub use runtime::Runtime;

/// Return the last private profile-selection failure stage for test diagnostics.
#[cfg(feature = "profile-selection-diagnostic")]
#[doc(hidden)]
#[must_use]
pub fn selection_diagnostic() -> Option<&'static str> {
    evidence::selection_diagnostic()
}
