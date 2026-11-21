//! What a `rawr run` tells the shells attached to its session (D537.3–4,
//! D1053.2).
//!
//! The owners that know what happened emit one JSON object per event:
//! `building` (a stage and what the status line says), `running`, `queued`, `swapped`,
//! `restarting` (a reason), `failed` (structured diagnostics) and `ended`
//! (the exit status). The run's [`session`] numbers each and tells every
//! shell attached to it. A file named in [`EVENTS`] gets each as a line
//! too, for a reader that tails it. The human lines on stderr stay as they
//! are.
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
use std::sync::{Once, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};

#[cfg(unix)]
pub mod reload;
#[cfg(unix)]
pub mod session;

#[cfg(unix)]
pub use session::Handed;

/// The listener a shell hands a run: on Unix alone, so none here.
#[cfg(not(unix))]
#[derive(Debug)]
pub enum Handed {}

/// A file a `rawr run` appends its events to.
pub const EVENTS: &str = "RAWR_MCP_EVENTS";

/// The socket a program with a debugger answers on ([`rawr::inspect`]):
/// the process that runs the program serves it.
pub const INSPECT: &str = "RAWR_MCP_INSPECT";

/// Where the program's debugger tells where it paused and when it went on
/// (#535): the run's session socket, or a file. The process that runs the
/// program has it.
pub const DEBUG_EVENTS: &str = rawr::inspect::EVENTS;

/// Set in a program's environment beside [`INSPECT`]: the program serves
/// the socket and registers its debugger at a shell's first request for
/// it, not before (#2266, #2297).
pub const DEBUGGER_ON_REQUEST: &str = "RAWR_DEBUGGER_ON_REQUEST";

/// Set in a program's environment when no shell started its run: the
/// session socket where it tells what it prints ([`rawr::teller`]), as
/// it is written to the terminal.
pub const OUTPUT: &str = rawr::teller::SOCKET;

/// Set by a shell that starts a run and sets its breakpoints before the
/// program runs (#2274): the owner gives it to the first program it
/// starts, which waits at its first statement until the shell releases it
/// ([`rawr::inspect::HOLD`]).
pub const HOLD: &str = rawr::inspect::HOLD;

/// Set by a shell that starts a run and reads the run's stderr as rawr's
/// own lines (#2367): the owner tells the program's stderr through the
/// session, as `output`, in place of writing it to its own stderr.
pub const PROGRAM_STDERR: &str = "RAWR_SESSION_PROGRAM_STDERR";

/// How often a watched process looks at its parent.
const WATCH: Duration = Duration::from_millis(100);

/// The events file, opened to append: each line is one `write` at the
/// file's end, so lines from any thread never interleave.
fn file() -> Option<&'static File> {
    static FILE: OnceLock<Option<File>> = OnceLock::new();
    FILE.get_or_init(|| {
        let path = std::env::var_os(EVENTS)?;
        OpenOptions::new().create(true).append(true).open(path).ok()
    })
    .as_ref()
}

/// Whether a shell started this run or a file reads its events: a program
/// that fails runs again on the next save, as at a terminal.
#[must_use]
pub fn supervised() -> bool {
    #[cfg(unix)]
    let handed = session::handed();
    #[cfg(not(unix))]
    let handed = false;
    handed || std::env::var_os(EVENTS).is_some_and(|path| !path.is_empty())
}

/// Append `event` as one line to the events file, if any, record it in
/// the session, and then send it to the program's [`reload`] stream: a
/// connection that hears a line it wrote knows the session has it.
pub fn emit(event: &Value) {
    let mut line = event.to_string();
    line.push('\n');
    if let Some(mut file) = file() {
        let _ = file.write_all(line.as_bytes());
    }
    #[cfg(unix)]
    {
        session::record(event);
        reload::broadcast(&line);
    }
}

/// A cold start is building: the stage it is in, and what its status line
/// says of it, such as `building runner · 212 of 340 crates · iced_wgpu · 3m`.
pub fn building(stage: &str, status: &str) {
    emit(&json!({ "event": "building", "stage": stage, "status": status }));
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
    let first = outcome.diagnostics.first();
    let field = |name: &str| first.and_then(|first| first.get(name)).cloned();
    emit(&json!({
        "event": "failed",
        "message": field("message").unwrap_or(Value::Null),
        "file": field("file").unwrap_or(Value::Null),
        "line": field("line").unwrap_or(Value::Null),
        "column": field("column").unwrap_or(Value::Null),
        "diagnostics": outcome.diagnostics,
        "rendered": outcome.render(rawr::diagnostic::Color::Never),
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
