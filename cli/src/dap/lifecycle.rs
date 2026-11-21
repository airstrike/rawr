//! How a client starts and ends: `initialize`, `launch` or `attach`, and
//! `terminate` or `disconnect`.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde_json::{json, Value};

use super::Adapter;
use crate::mcp::options::Options;
use crate::mcp::process::Cancel;
use crate::mcp::session::{Entry, Program, Session};
use crate::mcp::tools;

/// Why a second `launch` or `attach` is refused: a client debugs one
/// program.
const ONE_PROGRAM: &str =
    "the adapter already debugs a program: a client launches or attaches once";

impl Adapter {
    /// The adapter's capabilities, as `initialize` answers them.
    pub(super) fn initialize(&self, arguments: &Value) -> Value {
        if arguments["linesStartAt1"] == false {
            self.first_line.store(0, Ordering::Relaxed);
        }
        json!({
            "supportsConfigurationDoneRequest": true,
            "supportsFunctionBreakpoints": true,
            "supportsEvaluateForHovers": true,
            "supportsSetVariable": true,
            "supportsSetExpression": true,
            "supportsTerminateRequest": true,
        })
    }

    /// `launch`: start `rawr run` on the `program` path, with `args`, as
    /// `rawr mcp`'s `start` does, its program held at its start until
    /// `configurationDone`; a run started elsewhere that serves it is
    /// attached to instead, and runs on.
    pub(super) fn launch(self: &Arc<Self>, arguments: &Value) -> Result<Value, String> {
        if self.session.get().is_some() {
            return Err(ONE_PROGRAM.to_owned());
        }
        let cwd = tools::absolute(&self.root, arguments["cwd"].as_str());
        let place = tools::place(&cwd, arguments["program"].as_str())?;
        if let Some(name) = tools::terminal_ui(&place.root) {
            return Err(format!(
                "this program uses `{name}`, which draws a terminal UI, and the adapter can't host a terminal yet. Run it with `rawr run` in a terminal and attach to it."
            ));
        }
        let program: Vec<String> = arguments["args"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        let session = self
            .sessions
            .start(
                &place.root,
                Program {
                    path: &place.path,
                    options: &Options::default(),
                    arguments: &program,
                    stdin: None,
                },
                Entry::Hold,
                &Cancel::default(),
            )
            .map_err(|error| format!("cannot start rawr run: {error}"))?;
        let started = session.is_started();
        if let Err(refusal) = self.begin(Arc::clone(&session)) {
            // Another `launch` or `attach` won: the run this one started
            // goes.
            if started {
                session.stop();
            }
            return Err(refusal);
        }
        if started {
            self.held.store(true, Ordering::Release);
        } else {
            self.output(
                "console",
                "rawr: a `rawr run` started elsewhere serves this program, so the adapter attached to it\n",
            );
        }
        Ok(json!({}))
    }

    /// `attach`: reach the `rawr run` that serves the `program` path.
    pub(super) fn attach(self: &Arc<Self>, arguments: &Value) -> Result<Value, String> {
        if self.session.get().is_some() {
            return Err(ONE_PROGRAM.to_owned());
        }
        let cwd = tools::absolute(&self.root, arguments["cwd"].as_str());
        let place = tools::place(&cwd, arguments["program"].as_str())?;
        let session = self
            .sessions
            .get(&place.root, &Cancel::default())
            .ok_or_else(|| {
                format!(
                    "no `rawr run` serves {}: start one in a terminal, or launch it",
                    place.root.display()
                )
            })?;
        self.begin(session)?;
        Ok(json!({}))
    }

    /// Debug `session`'s program: tell its events from now on, and let the
    /// client set its breakpoints. Refused when the client already
    /// launched or attached.
    fn begin(self: &Arc<Self>, session: Arc<Session>) -> Result<(), String> {
        self.session
            .set(Arc::clone(&session))
            .map_err(|_| ONE_PROGRAM.to_owned())?;
        let following = Arc::clone(self);
        let _ = std::thread::Builder::new()
            .name("rawr-dap-follow".into())
            .spawn(move || following.follow(&session));
        self.out.event("initialized", json!({}));
        Ok(())
    }

    /// `configurationDone`: the client's breakpoints are in, so the program
    /// the adapter launched runs on from its start. A program that isn't
    /// running holds nothing.
    pub(super) fn configured(&self) -> Value {
        if self.held.swap(false, Ordering::AcqRel) {
            let _ = self.ask(&json!({ "op": "release" }));
        }
        json!({})
    }

    /// End the run the adapter started, once it exited, or detach from one
    /// started elsewhere, which keeps running.
    pub(super) fn end(&self) {
        let Ok(session) = self.session() else {
            return;
        };
        session.stop();
        session.wait_exit();
    }

    /// `disconnect`: end the launched run, which ends with the adapter in
    /// any case, or detach from one started elsewhere.
    pub(super) fn disconnect(&self, request: &Value) {
        self.end();
        self.out.respond(request, Ok(json!({})));
    }
}
