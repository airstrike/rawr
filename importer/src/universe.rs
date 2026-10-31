//! The universe runner: one runner built ahead of time that serves every
//! program whose native calls it covers, as `?play` runs snippets (#434,
//! #436).
//!
//! It names each script type by its index in the program's definition
//! order (`__RawrScript0`, `__RawrScript1`, ..), so the runner names no
//! script type and two programs that differ only in their type names run
//! on one binary. A project's runner names script types as the program
//! does, so `type_name` and iced's window title are the program's (D594).
//!
//! The setting is one per process. `rawr-import` takes it from
//! `RAWR_UNIVERSE_RUNNER=1` when it builds the runner; the generated runner
//! sets it itself before it compiles a program, so both compile the
//! program alike.

use std::sync::atomic::{AtomicBool, Ordering};

static UNIVERSE: AtomicBool = AtomicBool::new(false);

/// The variable that makes `rawr-import` build a universe runner.
pub const VARIABLE: &str = "RAWR_UNIVERSE_RUNNER";

/// Make this process compile programs as a universe runner does.
pub fn set() {
    UNIVERSE.store(true, Ordering::Relaxed);
}

/// Whether this process compiles programs as a universe runner does.
#[must_use]
pub fn enabled() -> bool {
    UNIVERSE.load(Ordering::Relaxed)
}
