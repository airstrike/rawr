//! What moves the program: breakpoints, `continue`, `pause` and the steps.
//! Each goes to the program's inspector through the session, which waits
//! for a program that starts or restarts.

use std::sync::{Arc, MutexGuard, PoisonError};

use serde_json::{json, Value};

use super::breakpoints::{Change, Known, Point};
use super::Adapter;
use crate::mcp::tools;

impl Adapter {
    /// `setBreakpoints`: the source's breakpoints become the lines given,
    /// each verified where the program has a statement on it.
    pub(super) fn set_lines(&self, arguments: &Value) -> Result<Value, String> {
        let file = arguments["source"]["path"]
            .as_str()
            .ok_or("setBreakpoints needs the source's path")?;
        let file = tools::absolute(&self.root, Some(file));
        let lines: Vec<u64> = arguments["breakpoints"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|breakpoint| breakpoint["line"].as_u64())
            .map(|line| self.line_in(line))
            .collect();
        let point = |line: u64| Point::Line {
            file: file.to_string_lossy().into_owned(),
            line,
        };
        let _one = self.breaking.lock().unwrap_or_else(PoisonError::into_inner);
        self.known().ask(lines.iter().copied().map(point).collect());
        let mut last = None;
        let mut answers = Vec::new();
        let cleared = self.ask(&json!({ "op": "clear", "file": file }));
        if let Ok(answer) = &cleared {
            last = Some(listed(answer));
            for line in &lines {
                let answer = self.ask(&json!({ "op": "break", "file": file, "line": line }));
                if let Ok(answer) = &answer {
                    last = Some(listed(answer));
                }
                answers.push((*line, answer));
            }
        }
        let known = self.told(last.as_ref());
        cleared?;
        let source = json!({ "path": file });
        let set: Vec<Value> = answers
            .into_iter()
            .map(|(line, answer)| match answer {
                Ok(answer) => json!({
                    "id": known.id(&point(line)),
                    "verified": true,
                    "line": self.line_out(answer["lines"][0]["line"].as_u64().unwrap_or(line)),
                    "source": source,
                }),
                Err(message) => json!({
                    "verified": false,
                    "line": self.line_out(line),
                    "message": message,
                    "source": source,
                }),
            })
            .collect();
        Ok(json!({ "breakpoints": set }))
    }

    /// `setFunctionBreakpoints`: the function breakpoints become those
    /// named, each pausing at its function's first statement.
    pub(super) fn set_functions(&self, arguments: &Value) -> Result<Value, String> {
        let names: Vec<&str> = arguments["breakpoints"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|breakpoint| breakpoint["name"].as_str())
            .collect();
        let _one = self.breaking.lock().unwrap_or_else(PoisonError::into_inner);
        let mut functions = self
            .functions
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.known().ask(
            functions
                .iter()
                .map(String::as_str)
                .chain(names.iter().copied())
                .map(|name| Point::Function(name.to_owned()))
                .collect(),
        );
        let mut last = None;
        let mut cleared = Ok(());
        for function in functions.drain(..) {
            match self.ask(&json!({ "op": "clear", "function": function })) {
                Ok(answer) => last = Some(listed(&answer)),
                Err(message) => {
                    cleared = Err(message);
                    break;
                }
            }
        }
        let mut answers = Vec::new();
        if cleared.is_ok() {
            for name in &names {
                let answer = self.ask(&json!({ "op": "break", "function": name }));
                if let Ok(answer) = &answer {
                    functions.push((*name).to_owned());
                    last = Some(listed(answer));
                }
                answers.push((*name, answer));
            }
        }
        let known = self.told(last.as_ref());
        cleared?;
        let set: Vec<Value> = answers
            .into_iter()
            .map(|(name, answer)| match answer {
                Ok(answer) => {
                    let at = &answer["lines"][0];
                    let mut breakpoint = json!({
                        "id": known.id(&Point::Function(name.to_owned())),
                        "verified": true,
                    });
                    if let (Some(file), Some(line)) = (at["file"].as_str(), at["line"].as_u64()) {
                        breakpoint["source"] = json!({ "path": file });
                        breakpoint["line"] = json!(self.line_out(line));
                    }
                    breakpoint
                }
                Err(message) => json!({ "verified": false, "message": message }),
            })
            .collect();
        Ok(json!({ "breakpoints": set }))
    }

    /// The client's breakpoint request ended, its last answer the list
    /// `last`: tell the client what other shells changed meanwhile, and
    /// give the breakpoints it knows, its own now among them.
    fn told(&self, last: Option<&Value>) -> MutexGuard<'_, Known> {
        let mut known = self.known();
        let changes = known.answered(last);
        self.tell_breakpoints(&changes);
        known
    }

    /// Tell the client what the session's newest breakpoints, `told`,
    /// changed of those it knows: what another shell set or cleared.
    pub(super) fn follow_breakpoints(&self, told: &Value) {
        let mut known = self.known();
        let changes = known.follow(told);
        self.tell_breakpoints(&changes);
    }

    fn tell_breakpoints(&self, changes: &[Change]) {
        for change in changes {
            self.out
                .event("breakpoint", change.body(|line| self.line_out(line)));
        }
    }

    /// `continue`: the paused program runs on.
    pub(super) fn resume(&self) -> Result<Value, String> {
        self.ask(&json!({ "op": "continue" }))?;
        self.session()?.observe_pause(None);
        Ok(json!({ "allThreadsContinued": true }))
    }

    /// `pause`: the running program pauses at the next statement it runs,
    /// told as `stopped`.
    pub(super) fn pause(&self) -> Result<Value, String> {
        self.ask(&json!({ "op": "pause" }))?;
        Ok(json!({}))
    }

    /// `next`, `stepIn` and `stepOut`, a step of `kind`: answered once the
    /// program goes, before the pause it ends in is told as `stopped`.
    pub(super) fn step(self: &Arc<Self>, request: &Value, kind: &str) {
        let paused = self.session().and_then(|session| {
            session
                .paused()
                .ok_or_else(|| "the program isn't paused".to_owned())
        });
        if let Err(message) = paused {
            self.out.respond(request, Err(message));
            return;
        }
        self.out.respond(request, Ok(json!({})));
        match self.ask(&json!({ "op": "step", "kind": kind })) {
            Ok(answer) => {
                if let Ok(session) = self.session() {
                    session.observe_pause(
                        answer["paused"]
                            .as_object()
                            .map(|_| answer["paused"].clone()),
                    );
                }
            }
            Err(message) => self.output("stderr", &format!("rawr: the step failed: {message}\n")),
        }
    }
}

/// The breakpoints a `break` or `clear` answer says there are now, as the
/// session tells them: `{"revision", "breakpoints"}`.
fn listed(answer: &Value) -> Value {
    let breakpoints = answer.get("breakpoints").unwrap_or(&answer["left"]);
    json!({ "revision": answer["revision"], "breakpoints": breakpoints })
}
