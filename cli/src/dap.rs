//! `rawr dap`: a Debug Adapter Protocol server on stdio for editors, one
//! shell of a run's debugger session (D1053.2), as `rawr mcp` is.
//!
//! It translates and holds nothing of its own beyond the protocol's
//! handles: `launch` starts a `rawr run` as `rawr mcp`'s `start` does, and
//! `attach` reaches a run started elsewhere, through the same
//! [`crate::mcp::session`]. Breakpoints, steps, a pause, values and `eval`
//! are the program's inspector's ([`rawr::inspect`]); the session's events
//! become `stopped`, `continued`, `output` and `terminated`. A save still
//! reloads the program, and the adapter says so as output.
//!
//! The protocol counts one thread: the one that runs the program's code,
//! which the inspector pauses. Frames are the inspector's, innermost first,
//! each named by its Rust path, and a value's children are its parts as
//! the inspector splits its `{:?}` text.

mod control;
mod follow;
mod handles;
mod lifecycle;
mod serve;
mod values;
mod wire;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

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
    session: Mutex<Option<Arc<Session>>>,
    handles: Mutex<handles::Handles>,
    /// The line the client counts from: 1, or 0.
    first_line: AtomicU64,
    /// The functions the client set breakpoints on, which its next
    /// `setFunctionBreakpoints` replaces.
    functions: Mutex<Vec<String>>,
    /// One breakpoint request changes the program's breakpoints at a time.
    breaking: Mutex<()>,
    /// The client heard `stopped` and no `continued` since.
    stopped: AtomicBool,
}

impl Adapter {
    fn new(root: PathBuf, out: wire::Writer) -> Self {
        Self {
            root,
            out,
            sessions: Sessions::default(),
            session: Mutex::new(None),
            handles: Mutex::new(handles::Handles::default()),
            first_line: AtomicU64::new(1),
            functions: Mutex::new(Vec::new()),
            breaking: Mutex::new(()),
            stopped: AtomicBool::new(false),
        }
    }

    fn handles(&self) -> MutexGuard<'_, handles::Handles> {
        self.handles.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The program's session, once the client launched or attached.
    fn session(&self) -> Result<Arc<Session>, String> {
        self.session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
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
