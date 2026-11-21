//! `rawr dap`: a Debug Adapter Protocol server on stdio for editors, one
//! shell of a run's debugger session (D1053.2), as `rawr mcp` is.
//!
//! It translates and holds nothing of its own beyond the protocol's
//! handles: `launch` starts a `rawr run` as `rawr mcp`'s `start` does, and
//! `attach` reaches a run started elsewhere, through the same
//! [`crate::mcp::session`]. Breakpoints, steps, a pause, values and `eval`
//! are the program's inspector's ([`rawr::inspect`]); the session's events
//! become `stopped`, `continued`, `output` and `terminated`, and the
//! breakpoints another shell sets or clears, and those a save moves,
//! become `breakpoint` events. A save still reloads the program, and the
//! adapter says so as output.
//!
//! The protocol counts one thread: the one that runs the program's code,
//! which the inspector pauses. Frames are the inspector's, innermost first,
//! each named by its Rust path, and a value's children are its parts as
//! the inspector splits its `{:?}` text. A value the client sets is
//! assigned through the same `eval`, at the value's place in its frame.

mod assign;
mod breakpoints;
mod control;
mod follow;
mod handles;
mod lifecycle;
mod place;
mod serve;
mod values;
mod wire;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use serde_json::{json, Value};

use crate::mcp::process::Cancel;
use crate::mcp::session::{Refusal, Session, Sessions};

pub use serve::{serve, serve_on};

/// The thread the protocol names: the one that runs the program's code.
const THREAD: u64 = 1;

/// One client, and the program it debugs once it launched or attached.
struct Adapter {
    /// Where relative paths start.
    root: PathBuf,
    out: wire::Writer,
    sessions: Sessions,
    /// The program's session, set once by the first `launch` or `attach`.
    session: OnceLock<Arc<Session>>,
    handles: Mutex<handles::Handles>,
    /// The line the client counts from: 1, or 0.
    first_line: AtomicU64,
    /// One breakpoint request changes the program's breakpoints at a time.
    /// It holds the functions the client set breakpoints on, which its
    /// next `setFunctionBreakpoints` replaces.
    breaking: Mutex<Vec<String>>,
    /// The breakpoints the client knows, by their ids.
    known: Mutex<breakpoints::Known>,
    /// The client heard `stopped` and no `continued` since.
    stopped: AtomicBool,
    /// The program the adapter launched waits at its start for the
    /// client's `configurationDone`, which releases it (#2274).
    held: AtomicBool,
}

impl Adapter {
    fn new(root: PathBuf, out: wire::Writer) -> Self {
        Self {
            root,
            out,
            sessions: Sessions::default(),
            session: OnceLock::new(),
            handles: Mutex::new(handles::Handles::default()),
            first_line: AtomicU64::new(1),
            breaking: Mutex::new(Vec::new()),
            known: Mutex::new(breakpoints::Known::default()),
            stopped: AtomicBool::new(false),
            held: AtomicBool::new(false),
        }
    }

    fn handles(&self) -> MutexGuard<'_, handles::Handles> {
        self.handles.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn known(&self) -> MutexGuard<'_, breakpoints::Known> {
        self.known.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The program's session, once the client launched or attached.
    fn session(&self) -> Result<Arc<Session>, String> {
        self.session
            .get()
            .cloned()
            .ok_or_else(|| "no program yet: launch or attach first".to_owned())
    }

    /// The program's answer to `request`, or why it has none: refused by
    /// the session or by the program.
    fn ask(&self, request: &Value) -> Result<Value, String> {
        let session = self.session()?;
        let answer = session
            .ask(request, &Cancel::default())
            .map_err(|refusal| match refusal {
                Refusal::NotRunning => format!(
                    "the program isn't running (state: {})",
                    session.phase().name()
                ),
                Refusal::Error(error) => error,
                Refusal::Cancelled => "cancelled".to_owned(),
            })?;
        match answer["error"].as_str() {
            Some("not paused") => Err("the program isn't paused".to_owned()),
            Some(error) => Err(error.to_owned()),
            None => Ok(answer),
        }
    }

    /// A line the client gave, as the program counts lines: from 1.
    fn line_in(&self, line: u64) -> u64 {
        line + 1 - self.first_line.load(Ordering::Relaxed)
    }

    /// A line of the program, as the client counts lines.
    fn line_out(&self, line: u64) -> u64 {
        (line + self.first_line.load(Ordering::Relaxed)).saturating_sub(1)
    }

    /// Tell the client `text` as output in `category`: `console` for the
    /// adapter's own, `stdout` and `stderr` for the program's.
    fn output(&self, category: &str, text: &str) {
        self.out
            .event("output", json!({ "category": category, "output": text }));
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead as _, BufReader, Write as _};
    use std::path::PathBuf;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    use serde_json::{json, Value};

    use super::{wire, Adapter};
    use crate::mcp::session::{Phase, Session};

    /// `configurationDone` releases the program the adapter launched, held
    /// at its start, once; a second asks the program nothing (#2274).
    #[test]
    fn configuration_done_releases_a_launched_program_once() {
        let (session, other) = Session::running_at(Phase::Running);
        let adapter = Arc::new(Adapter::new(
            PathBuf::from("/p"),
            wire::Writer::new(std::io::sink()).unwrap(),
        ));
        assert!(
            adapter.session.set(session).is_ok(),
            "the adapter starts with no session"
        );
        adapter.held.store(true, Ordering::Release);
        let configuring = Arc::clone(&adapter);
        let configured = std::thread::spawn(move || configuring.configured());
        let mut lines = BufReader::new(other.try_clone().unwrap()).lines();
        let mut next = || serde_json::from_str::<Value>(&lines.next().unwrap().unwrap()).unwrap();
        assert_eq!(next()["method"], "hello");
        let asked = next();
        assert_eq!(asked["params"], json!({ "op": "release" }));
        let answer = json!({ "jsonrpc": "2.0", "id": asked["id"], "result": { "released": true } });
        writeln!(&other, "{answer}").unwrap();
        assert_eq!(configured.join().unwrap(), json!({}));
        // Nothing is held any more: no request goes to the program.
        assert_eq!(adapter.configured(), json!({}));
    }

    /// A breakpoint another shell sets reaches the client as a `breakpoint`
    /// event, `new` with its line as the client counts lines, and its
    /// clear as `removed` by the same id (#2278).
    #[test]
    fn another_shells_breakpoint_is_told_as_new_then_removed() {
        let (written, read) = std::os::unix::net::UnixStream::pair().unwrap();
        let adapter = Adapter::new(PathBuf::from("/p"), wire::Writer::new(written).unwrap());
        adapter.first_line.store(0, Ordering::Relaxed);
        let mut heard = BufReader::new(read);
        let spec = json!({ "file": "/p/src/main.rs", "line": 7, "text": "tick();" });
        adapter.follow_breakpoints(&json!({ "revision": 1, "breakpoints": [spec] }));
        let new = wire::read(&mut heard).unwrap().unwrap();
        assert_eq!(new["event"], "breakpoint", "{new}");
        assert_eq!(new["body"]["reason"], "new", "{new}");
        assert_eq!(new["body"]["breakpoint"]["line"], 6, "{new}");
        assert_eq!(
            new["body"]["breakpoint"]["source"]["path"],
            "/p/src/main.rs"
        );
        adapter.follow_breakpoints(&json!({ "revision": 2, "breakpoints": [] }));
        let removed = wire::read(&mut heard).unwrap().unwrap();
        assert_eq!(removed["body"]["reason"], "removed", "{removed}");
        assert_eq!(
            removed["body"]["breakpoint"]["id"],
            new["body"]["breakpoint"]["id"]
        );
    }

    /// A save that moves a breakpoint to another line, which the session's
    /// `breakpoints` event lists under the same inspector id, reaches the
    /// client as `changed` with the id it knows, so the client keeps its
    /// own breakpoint (#2299).
    #[test]
    fn a_breakpoint_a_save_moved_is_told_as_changed() {
        let (session, other) = Session::running_at(Phase::Running);
        let (written, read) = std::os::unix::net::UnixStream::pair().unwrap();
        let adapter = Arc::new(Adapter::new(
            PathBuf::from("/p"),
            wire::Writer::new(written).unwrap(),
        ));
        let following = Arc::clone(&adapter);
        let followed = std::thread::spawn(move || following.follow(&session));
        let mut heard = BufReader::new(read);
        let mut breakpoint = |revision: u64, line: u64| {
            let listed =
                json!({ "id": 3, "file": "/p/src/main.rs", "line": line, "text": "tick();" });
            let event =
                json!({ "event": "breakpoints", "revision": revision, "breakpoints": [listed] });
            let told = json!({ "jsonrpc": "2.0", "method": "event", "params": event });
            writeln!(&other, "{told}").unwrap();
            // The next `breakpoint` event, past the run's own output.
            loop {
                let message = wire::read(&mut heard).unwrap().unwrap();
                if message["event"] == "breakpoint" {
                    return message["body"].clone();
                }
            }
        };
        let new = breakpoint(1, 7);
        assert_eq!(new["reason"], "new", "{new}");
        let changed = breakpoint(2, 9);
        assert_eq!(changed["reason"], "changed", "{changed}");
        assert_eq!(changed["breakpoint"]["id"], new["breakpoint"]["id"]);
        assert_eq!(changed["breakpoint"]["line"], 9, "{changed}");
        assert_eq!(changed["breakpoint"]["source"]["path"], "/p/src/main.rs");
        other.shutdown(std::net::Shutdown::Both).unwrap();
        followed.join().unwrap();
    }

    /// Rawr's own lines, the stderr of the run the adapter started, reach
    /// the client as `console`, and the program's stderr, which the
    /// session tells, as `stderr` (#2367).
    #[test]
    fn rawr_lines_are_console_and_the_programs_stderr_is_stderr() {
        let (session, other) = Session::running_at(Phase::Running);
        let (written, read) = std::os::unix::net::UnixStream::pair().unwrap();
        let adapter = Arc::new(Adapter::new(
            PathBuf::from("/p"),
            wire::Writer::new(written).unwrap(),
        ));
        session.read_rawr(b"rawr > building runner  55%\n");
        let told = json!({
            "jsonrpc": "2.0",
            "method": "output",
            "params": { "stream": "stderr", "text": "the program's own\n" },
        });
        writeln!(&other, "{told}").unwrap();
        let following = Arc::clone(&adapter);
        let followed = std::thread::spawn(move || following.follow(&session));
        let mut heard = BufReader::new(read);
        let mut outputs = Vec::new();
        while outputs.len() < 2 {
            let message = wire::read(&mut heard).unwrap().unwrap();
            if message["event"] == "output" {
                outputs.push((
                    message["body"]["category"].as_str().unwrap().to_owned(),
                    message["body"]["output"].as_str().unwrap().to_owned(),
                ));
            }
        }
        outputs.sort();
        assert_eq!(
            outputs,
            [
                (
                    "console".to_owned(),
                    "rawr > building runner  55%\n".to_owned()
                ),
                ("stderr".to_owned(), "the program's own\n".to_owned()),
            ]
        );
        other.shutdown(std::net::Shutdown::Both).unwrap();
        followed.join().unwrap();
    }
}
