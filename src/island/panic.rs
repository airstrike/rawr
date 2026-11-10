//! A marked function's panic in a call from native code (#1657). The call
//! prints it as Rust's panic hook prints a panic of the code Cargo built,
//! at its place in the program's files, on the thread that called, and
//! the trampoline unwinds with its message, printing no place of its own.

use std::cell::RefCell;

use crate::{diagnostic, program};

/// The first byte of a reply whose trampoline unwinds with the message the
/// rest holds, the call having printed its panic.
const UNWIND: u8 = 2;

/// The first run-time failure of the functions one call from native code
/// invoked.
#[derive(Default)]
pub(super) struct Caught(RefCell<Option<diagnostic::Report>>);

impl Caught {
    /// `error`'s text, which the call's failure says. A run-time failure
    /// is kept as its panic, at its place in the program's files.
    pub(super) fn text(&self, error: &program::Error) -> String {
        if let program::Error::Evaluation { error: failure, .. } = error
            && let Some(report) = diagnostic::located(failure)
        {
            self.0.borrow_mut().get_or_insert(report);
        }
        error.to_string()
    }

    /// `reply`, or, for a failed reply of a call that caught a panic, that
    /// panic: printed here, and the trampoline unwinds with its message.
    pub(super) fn reply(self, reply: Vec<u8>) -> Vec<u8> {
        let report = match (reply.first(), self.0.into_inner()) {
            (Some(0), _) | (_, None) => return reply,
            (_, Some(report)) => report,
        };
        let thread = std::thread::current();
        eprint!(
            "{}",
            diagnostic::hooked(&report, thread.name().unwrap_or("<unnamed>"))
        );
        let message = diagnostic::panic_message(&report);
        let mut unwind = Vec::with_capacity(1 + message.len());
        unwind.push(UNWIND);
        unwind.extend_from_slice(message.as_bytes());
        unwind
    }
}
