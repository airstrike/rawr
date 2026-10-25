//! What a supervised `rawr run` tells the `rawr mcp` server that started it
//! (D537.3–4).
//!
//! The server names a file in [`EVENTS`]. The owners that know what
//! happened append one JSON object per line to it: `building` (a stage and
//! a percent), `running`, `swapped`, `restarting` (a reason), `failed`
//! (structured diagnostics) and `ended` (the exit status). The server reads
//! the file as it grows and numbers each event. A file needs no descriptor
//! passed across `exec` or kept from runners: Rust opens it close-on-exec,
//! and an `exec` into the binary that serves the program opens it again.
//! The human lines on stderr stay as they are.
//!
//! The server also names its own pid in [`PARENT`]. A process it started
//! directly watches that pid: when the server dies, even by SIGKILL, the
//! process ends what it owns and then its own process group.

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::sync::{Mutex, Once, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};

/// The file a supervised `rawr run` appends its events to.
pub const EVENTS: &str = "RAWR_MCP_EVENTS";

/// The pid of the `rawr mcp` server that started this process.
pub const PARENT: &str = "RAWR_MCP_PARENT";

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
/// and a program that ends runs again on the next save, as at a terminal.
#[must_use]
pub fn supervised() -> bool {
    std::env::var_os(EVENTS).is_some_and(|path| !path.is_empty())
}

/// Append `event` as one line, when supervised.
pub fn emit(event: &Value) {
    let Some(file) = file() else {
        return;
    };
    let mut line = event.to_string();
    line.push('\n');
    let mut file = file
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _ = file.write_all(line.as_bytes());
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

/// A save restarts the program, for `reason`.
pub fn restarting(reason: &str) {
    emit(&json!({ "event": "restarting", "reason": reason }));
}

/// A start or a save failed. The last good program, if any, keeps running.
pub fn failed(report: &rawr::diagnostic::Report) {
    if file().is_none() {
        return;
    }
    let base = std::env::current_dir().unwrap_or_default();
    emit(&json!({
        "event": "failed",
        "diagnostics": crate::diagnostic::shape(report, &base),
        "rendered": rawr::diagnostic::render(report),
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

/// When the server in [`PARENT`] started this process directly, watch it
/// from a thread: once it is gone, call `retire` to end what this process
/// owns in other process groups (a runner and its window), then end this
/// process's own group when it leads one, as the server started it.
pub fn watch_parent(retire: fn()) {
    static STARTED: Once = Once::new();
    let Some(expected) = std::env::var(PARENT)
        .ok()
        .and_then(|pid| pid.parse::<u32>().ok())
    else {
        return;
    };
    #[cfg(unix)]
    {
        // A process further down inherits the variable; only the server's
        // own child watches it.
        if std::os::unix::process::parent_id() != expected {
            return;
        }
        STARTED.call_once(|| {
            let _ = std::thread::Builder::new()
                .name("rawr-parent-watch".into())
                .spawn(move || {
                    while std::os::unix::process::parent_id() == expected {
                        std::thread::sleep(WATCH);
                    }
                    retire();
                    end_own_group();
                    std::process::exit(1);
                });
        });
    }
    #[cfg(not(unix))]
    {
        let _ = (expected, retire, &STARTED);
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
