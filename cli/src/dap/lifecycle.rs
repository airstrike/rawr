//! How a client starts and ends: `initialize`, `launch` or `attach`, and
//! `terminate` or `disconnect`.

use std::sync::Arc;

use serde_json::{json, Value};

use super::Adapter;
use crate::mcp::session::{Entry, Session, Wait};
use crate::mcp::tools;

/// Why a second `launch` or `attach` is refused: a client debugs one
/// program.
const ONE_PROGRAM: &str =
    "the adapter already debugs a program: a client launches or attaches once";

impl Adapter {
    /// The adapter's capabilities, as `initialize` answers them.
    pub(super) fn initialize(&mut self, arguments: &Value) -> Value {
        if arguments["linesStartAt1"] == false {
            self.first_line = 0;
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

    /// Where a `launch` or `attach` request's `program` is, unless the
    /// client already launched or attached.
    fn program(&self, arguments: &Value) -> Result<tools::Place, String> {
        if self.session.is_some() || self.beginning {
            return Err(ONE_PROGRAM.to_owned());
        }
        let cwd = tools::absolute(&self.root, arguments["cwd"].as_str());
        tools::place(&cwd, arguments["program"].as_str())
    }

    /// `launch`: start `rawr run` on the `program` path, with `args`, as
    /// `rawr mcp`'s `start` does, its program held at its start until
    /// `configurationDone`; a run started elsewhere that serves it is
    /// attached to instead, and runs on.
    pub(super) fn launch(&mut self, request: Value) {
        let arguments = &request["arguments"];
        let place = match self.program(arguments) {
            Ok(place) => place,
            Err(refusal) => return self.out.respond(&request, Err(refusal)),
        };
        if let Some(name) = tools::terminal_ui(&place.root) {
            return self.out.respond(&request, Err(format!(
                "this program uses `{name}`, which draws a terminal UI, and the adapter can't host a terminal yet. Run it with `rawr run` in a terminal and attach to it."
            )));
        }
        let program: Vec<String> = arguments["args"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        self.beginning = true;
        let Some(reached) = Session::reach(&place.root) else {
            return self.spawn(&request, &place, &program);
        };
        let greeting = Arc::clone(&reached);
        self.park(&greeting, Wait::Greeting, move |adapter, _| {
            if !reached.reached() {
                reached.stop();
                return adapter.spawn(&request, &place, &program);
            }
            adapter.begin(reached);
            adapter.output(
                "console",
                "rawr: a `rawr run` started elsewhere serves this program, so the adapter attached to it\n",
            );
            adapter.out.respond(&request, Ok(json!({})));
        });
    }

    /// Start the `rawr run` a `launch` asked for, its program held.
    fn spawn(&mut self, request: &Value, place: &tools::Place, program: &[String]) {
        match Session::spawn(&place.root, &place.path, program, None, Entry::Hold) {
            Ok(session) => {
                self.begin(session);
                self.held = true;
                self.out.respond(request, Ok(json!({})));
            }
            Err(error) => {
                self.beginning = false;
                self.out
                    .respond(request, Err(format!("cannot start rawr run: {error}")));
            }
        }
    }

    /// `attach`: reach the `rawr run` that serves the `program` path.
    pub(super) fn attach(&mut self, request: Value) {
        let place = match self.program(&request["arguments"]) {
            Ok(place) => place,
            Err(refusal) => return self.out.respond(&request, Err(refusal)),
        };
        let none = format!(
            "no `rawr run` serves {}: start one in a terminal, or launch it",
            place.root.display()
        );
        let Some(reached) = Session::reach(&place.root) else {
            return self.out.respond(&request, Err(none));
        };
        self.beginning = true;
        let greeting = Arc::clone(&reached);
        self.park(&greeting, Wait::Greeting, move |adapter, _| {
            if reached.reached() {
                adapter.begin(reached);
                adapter.out.respond(&request, Ok(json!({})));
            } else {
                reached.stop();
                adapter.beginning = false;
                adapter.out.respond(&request, Err(none));
            }
        });
    }

    /// Debug `session`'s program: tell its events from now on, and let the
    /// client set its breakpoints.
    pub(super) fn begin(&mut self, session: Arc<Session>) {
        self.beginning = false;
        self.following = super::follow::Following::of(&session);
        self.session = Some(session);
        self.out.event("initialized", json!({}));
        self.follow();
    }

    /// `configurationDone`: the client's breakpoints are in, so the program
    /// the adapter launched runs on from its start, answered once it does.
    /// A program that isn't running holds nothing.
    pub(super) fn configured(&mut self, request: Value) {
        if !std::mem::take(&mut self.held) {
            return self.out.respond(&request, Ok(json!({})));
        }
        self.ask(json!({ "op": "release" }), move |adapter, _| {
            adapter.out.respond(&request, Ok(json!({})));
        });
    }

    /// End the run the adapter started, then go on with `then` once it
    /// exited, or detach from one started elsewhere, which keeps running.
    pub(super) fn end(&mut self, then: impl FnOnce(&mut Self) + 'static) {
        let Some(session) = self.session.clone() else {
            return then(self);
        };
        session.stop();
        if !session.is_started() {
            return then(self);
        }
        self.park(&session, Wait::Exit, move |adapter, _| then(adapter));
    }
}
