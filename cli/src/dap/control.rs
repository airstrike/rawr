//! What moves the program: breakpoints, `continue`, `pause` and the steps.
//! Each goes to the program's inspector through the session, which waits
//! for a program that starts or restarts.

use std::sync::{Arc, PoisonError};

use serde_json::{json, Value};

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
        let _one = self.breaking.lock().unwrap_or_else(PoisonError::into_inner);
        self.ask(&json!({ "op": "clear", "file": file }))?;
        let source = json!({ "path": file });
        let set: Vec<Value> = arguments["breakpoints"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|breakpoint| breakpoint["line"].as_u64())
            .map(|line| {
                let line = self.line_in(line);
                match self.ask(&json!({ "op": "break", "file": file, "line": line })) {
                    Ok(answer) => json!({
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
                }
            })
            .collect();
        Ok(json!({ "breakpoints": set }))
    }

    /// `setFunctionBreakpoints`: the function breakpoints become those
    /// named, each pausing at its function's first statement.
    pub(super) fn set_functions(&self, arguments: &Value) -> Result<Value, String> {
        let _one = self.breaking.lock().unwrap_or_else(PoisonError::into_inner);
        let mut functions = self
            .functions
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for function in functions.drain(..) {
            self.ask(&json!({ "op": "clear", "function": function }))?;
        }
        let mut set = Vec::new();
        for name in arguments["breakpoints"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|breakpoint| breakpoint["name"].as_str())
        {
            set.push(
                match self.ask(&json!({ "op": "break", "function": name })) {
                    Ok(answer) => {
                        functions.push(name.to_owned());
                        let at = &answer["lines"][0];
                        let mut breakpoint = json!({ "verified": true });
                        if let (Some(file), Some(line)) = (at["file"].as_str(), at["line"].as_u64())
                        {
                            breakpoint["source"] = json!({ "path": file });
                            breakpoint["line"] = json!(self.line_out(line));
                        }
                        breakpoint
                    }
                    Err(message) => json!({ "verified": false, "message": message }),
                },
            );
        }
        Ok(json!({ "breakpoints": set }))
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
