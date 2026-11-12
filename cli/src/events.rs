//! What a supervised `rawr run` tells the `rawr mcp` server that started it
//! (D537.3–4).
//!
//! The server names a file in [`EVENTS`]. The owners that know what
//! happened append one JSON object per line to it: `building` (a stage and
//! a percent), `running`, `queued`, `swapped`, `restarting` (a reason), `failed`
//! (structured diagnostics) and `ended` (the exit status). The server reads
//! the file as it grows and numbers each event. A file needs no descriptor
//! passed across `exec` or kept from runners: Rust opens it close-on-exec,
//! and an `exec` into the binary that serves the program opens it again.
//! The human lines on stderr stay as they are.
//!
//! The same lines go to the program itself, on the socket [`reload`]
//! serves: `queued`, `swapped`, `failed` and `restarting` are what a
//! program with an editor of its own shows (#1166).
//!
//! A run watches the process that started it ([`watch_parent`]): when that
//! process dies, even by SIGKILL, the run ends what it owns and then its
//! own process group when it leads one, as a server starts it.

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::sync::{Mutex, Once, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};

#[cfg(unix)]
pub mod reload;

/// The file a supervised `rawr run` appends its events to.
pub const EVENTS: &str = "RAWR_MCP_EVENTS";

/// The socket a supervised program answers `inspect` on
/// ([`rawr::inspect`]): the process that runs the program serves it.
pub const INSPECT: &str = "RAWR_MCP_INSPECT";

/// The events file again, for the program's debugger: where it paused and
/// when it went on (#535). The process that runs the program has it, which
/// [`EVENTS`] leaves out so it doesn't report as an owner.
pub const DEBUG_EVENTS: &str = rawr::inspect::EVENTS;

/// How often a watched process looks at its parent.
const WATCH: Duration = Duration::from_millis(100);

fn file() -> Option<&'static Mutex<File>> {
    static FILE: OnceLock<Option<Mutex<File>>> = OnceLock::new();
    FILE.get_or_init(|| {
        let path = std::env::var_os(EVENTS)?;
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
            .map(Mutex::new)
    })
    .as_ref()
}

/// Whether a `rawr mcp` server supervises this run: it reads the events,
/// and a program that fails runs again on the next save, as at a terminal.
#[must_use]
pub fn supervised() -> bool {
    std::env::var_os(EVENTS).is_some_and(|path| !path.is_empty())
}

/// Append `event` as one line, when supervised, and then send it to the
/// program's [`reload`] stream: a connection that hears a line it wrote
/// knows the file has it.
pub fn emit(event: &Value) {
    let mut line = event.to_string();
    line.push('\n');
    if let Some(file) = file() {
        let mut file = file
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = file.write_all(line.as_bytes());
    }
    #[cfg(unix)]
    reload::broadcast(&line);
}

/// A cold start is building: the stage it is in and the percent done.
pub fn building(stage: &str, percent: u8) {
    emit(&json!({ "event": "building", "stage": stage, "percent": percent }));
}

/// The program is running its first generation, or again after a restart.
pub fn running() {
    emit(&json!({ "event": "running" }));
}

/// A save swapped into the running program, which kept its state.
pub fn swapped() {
    emit(&json!({ "event": "swapped" }));
}

/// A save went to the running program, which takes it at its next call
/// from native code into script code and then says `swapped` (D553.3).
pub fn queued() {
    emit(&json!({ "event": "queued" }));
}

/// A save takes the full preparation instead of compiling in the running
/// program, for `reason`.
pub fn preparing(reason: &str) {
    emit(&json!({ "event": "preparing", "reason": reason }));
}

/// A save restarts the program, for `reason`.
pub fn restarting(reason: &str) {
    emit(&json!({ "event": "restarting", "reason": reason }));
}

/// A start or a save failed. The last good program, if any, keeps running.
/// The first diagnostic's message and position are the event's own, for a
/// reader that shows one line.
pub fn failed(report: &rawr::diagnostic::Report) {
    #[cfg(unix)]
    let streamed = reload::path().is_some();
    #[cfg(not(unix))]
    let streamed = false;
    if file().is_none() && !streamed {
        return;
    }
    let base = std::env::current_dir().unwrap_or_default();
    let diagnostics = crate::diagnostic::shape(report, &base);
    let first = diagnostics.first();
    let field = |name: &str| first.and_then(|first| first.get(name)).cloned();
    emit(&json!({
        "event": "failed",
        "message": field("message").unwrap_or(Value::Null),
        "file": field("file").unwrap_or(Value::Null),
        "line": field("line").unwrap_or(Value::Null),
        "column": field("column").unwrap_or(Value::Null),
        "diagnostics": diagnostics,
        "rendered": rawr::diagnostic::render(report),
    }));
}

/// A start or a save failed as `cargo check` failed, and cargo's errors
/// are the whole report: [`failed`], from rustc's diagnostics.
pub fn failed_cargo(outcome: &crate::cargo::Outcome) {
    #[cfg(unix)]
    let streamed = reload::path().is_some();
    #[cfg(not(unix))]
    let streamed = false;
    if file().is_none() && !streamed {
        return;
    }
    let first = outcome.diagnostics.first();
    let field = |name: &str| first.and_then(|first| first.get(name)).cloned();
    emit(&json!({
        "event": "failed",
        "message": field("message").unwrap_or(Value::Null),
        "file": field("file").unwrap_or(Value::Null),
        "line": field("line").unwrap_or(Value::Null),
        "column": field("column").unwrap_or(Value::Null),
        "diagnostics": outcome.diagnostics,
        "rendered": outcome.render(),
    }));
}

/// The program ended with `status`.
pub fn ended(status: i32) {
    emit(&json!({ "event": "ended", "status": status }));
}

/// The program ended with `status`: its code, or 128 plus its signal.
pub fn exited(status: std::process::ExitStatus) {
    #[cfg(unix)]
    let signal = std::os::unix::process::ExitStatusExt::signal(&status);
    #[cfg(not(unix))]
    let signal: Option<i32> = None;
    ended(
        status
            .code()
            .or_else(|| signal.map(|signal| 128 + signal))
            .unwrap_or(1),
    );
}

/// Watch the process that started this one from a thread: once it is
/// gone, call `retire` to end what this process owns in other process
/// groups (a runner and its window), then end this process's own group when
/// it leads one, as a `rawr mcp` server starts it, and exit. A run outlives
/// no parent: a server, a shell or a test that dies takes its run with it.
pub fn watch_parent(retire: fn()) {
    static STARTED: Once = Once::new();
    #[cfg(unix)]
    STARTED.call_once(|| {
        let parent = std::os::unix::process::parent_id();
        let _ = std::thread::Builder::new()
            .name("rawr-parent-watch".into())
            .spawn(move || {
                // An orphan is the init process's child.
                while std::os::unix::process::parent_id() == parent {
                    std::thread::sleep(WATCH);
                }
                retire();
                end_own_group();
                std::process::exit(1);
            });
    });
    #[cfg(not(unix))]
    {
        let _ = (retire, &STARTED);
    }
}

/// Kill this process's group when this process leads it: everything the
/// server's child started there (a program, `cargo check`) goes with it.
#[cfg(unix)]
fn end_own_group() {
    use nix::sys::signal::{killpg, Signal};
    use nix::unistd::{getpgrp, getpid};
    if getpgrp() == getpid() {
        let _ = killpg(getpgrp(), Signal::SIGKILL);
    }
}
