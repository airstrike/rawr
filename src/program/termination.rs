//! std's `Termination` of what a program's `main` returns: the report it
//! writes on stderr and the status the process ends with.

use crate::Dynamic;

/// How a program ends once its `main` has returned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Termination {
    report: Option<String>,
    status: u8,
}

impl Termination {
    /// What `main`'s returned `value` ends the process with, as std's
    /// `Termination` impls report it: `()` succeeds, an `ExitCode` is its
    /// status, `Ok(t)` is `t`'s, and `Err(error)` reports `Error:
    /// {error:?}` and fails with 1.
    #[must_use]
    pub fn of(value: Dynamic) -> Self {
        if let Some(code) = value.read_lock::<crate::std_facade::ExitCode>() {
            return Self {
                report: None,
                status: code.status(),
            };
        }
        match value.try_cast::<Result<Dynamic, Dynamic>>() {
            Some(Ok(value)) => Self::of(value),
            Some(Err(error)) => Self {
                report: Some(format!("Error: {error:?}")),
                status: 1,
            },
            None => Self {
                report: None,
                status: 0,
            },
        }
    }

    /// What it writes on stderr, without its newline.
    #[must_use]
    pub fn report(&self) -> Option<&str> {
        self.report.as_deref()
    }

    /// The status the process ends with.
    #[must_use]
    pub const fn status(&self) -> u8 {
        self.status
    }
}
