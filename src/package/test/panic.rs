//! A test's panics, as libtest's test threads print and catch them. The
//! harness runs each test on its own thread, so a panic there is printed
//! on a thread named after the test, with the test's output, and a panic
//! that unwinds out of the test, from the program's code or from native
//! code it called, fails that test alone.

use std::any::Any;
use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, PanicHookInfo};
use std::sync::Once;

use crate::diagnostic;

thread_local! {
    /// The test running on this thread, while the harness runs one.
    static RUNNING: RefCell<Option<Running>> = const { RefCell::new(None) };
}

/// A test the harness is running on this thread.
struct Running {
    name: Box<str>,
    output: Output,
}

/// Where a running test's panics are printed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Output {
    /// With what the test printed, which the harness shows when it fails.
    Captured,
    /// On stderr as it runs, with `--nocapture`.
    Printed,
}

/// Call `test`, the function of the test `name`, on this thread, its
/// panics printed to `output` on a thread named after the test. `Err` is
/// the message of a panic that unwound out of it.
pub(super) fn catch<T>(name: &str, output: Output, test: impl FnOnce() -> T) -> Result<T, String> {
    hook();
    RUNNING.with_borrow_mut(|running| {
        *running = Some(Running {
            name: name.into(),
            output,
        });
    });
    let caught = std::panic::catch_unwind(AssertUnwindSafe(test));
    RUNNING.with_borrow_mut(|running| *running = None);
    caught.map_err(|payload| message(&*payload).to_owned())
}

/// Print a panic on this thread, as Rust's hook prints it: `text` is its
/// text for the name of the thread it is on. A panic of the test running
/// on this thread is named after the test and printed to its output, as
/// libtest's test thread prints it; any other is on stderr.
pub fn print(text: impl FnOnce(&str) -> String) {
    match running() {
        Some(running) => running.print(text),
        None => {
            let thread = std::thread::current();
            eprint!("{}", text(thread.name().unwrap_or("<unnamed>")));
        }
    }
}

/// The test running on this thread, if the harness runs one.
fn running() -> Option<Running> {
    RUNNING
        .try_with(|running| {
            running.try_borrow().ok()?.as_ref().map(|running| Running {
                name: running.name.clone(),
                output: running.output,
            })
        })
        .ok()
        .flatten()
}

impl Running {
    /// Print `text`, a panic's for the name of its thread, as this test's
    /// panic.
    fn print(&self, text: impl FnOnce(&str) -> String) {
        let text = text(&self.name);
        match self.output {
            Output::Captured => super::harness::capture_text(&text),
            Output::Printed => eprint!("{text}"),
        }
    }
}

/// Install, once, rawr's panic hook for native code: it prints a running
/// test's panics as [`print`] does, and a panic at an adapter's call of a
/// crate's `#[track_caller]` function at the program's call, as rustc
/// places it (`std_facade::panic::panicked_at`). Any other panic goes to
/// the hook that was installed before.
pub(crate) fn hook() {
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| match running() {
            Some(running) => running.print(|thread| hooked(info, thread)),
            None if placed(info).is_some() => print(|thread| hooked(info, thread)),
            None => previous(info),
        }));
    });
}

/// The program's call that `info`'s panic is at, when it is at an
/// adapter's call, which stands for that call.
fn placed(info: &PanicHookInfo<'_>) -> Option<std::panic::Location<'static>> {
    info.location()
        .and_then(crate::std_facade::panic::panicked_at)
}

/// `info`'s panic, on the thread named `thread`, as Rust's hook prints it:
/// where it panicked, its message, and the note on backtraces after the
/// process's first panic.
fn hooked(info: &PanicHookInfo<'_>, thread: &str) -> String {
    let location = placed(info)
        .map(|location| location.to_string())
        .or_else(|| info.location().map(ToString::to_string));
    format!(
        "{}{}",
        diagnostic::hook_text(thread, location.as_deref(), message(info.payload())),
        diagnostic::note()
    )
}

/// A panic's message, from its payload: the text `panic!` gave, or what
/// Rust's hook says of a payload that isn't text.
fn message(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("Box<dyn Any>")
}
