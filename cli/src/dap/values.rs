//! What the client reads where the program is paused: its frames, each
//! frame's bindings, their parts, and code evaluated there.

use serde_json::{json, Value};

use super::handles::Handle;
use super::place::Place;
use super::Adapter;

impl Adapter {
    /// `stackTrace`: the program's frames where it is paused, innermost
    /// first, each named by its function's Rust path. A frame's id is its
    /// index from 1. A program that isn't paused has none.
    pub(super) fn stack(&mut self, request: Value) {
        let Some(session) = &self.session else {
            return self
                .out
                .respond(&request, Err(super::NO_PROGRAM.to_owned()));
        };
        if session.paused().is_none() {
            let none = json!({ "stackFrames": [], "totalFrames": 0 });
            return self.out.respond(&request, Ok(none));
        }
        self.ask(json!({ "frame": 0 }), move |adapter, answer| {
            let frames = answer.map(|answer| adapter.frames(&request["arguments"], answer));
            adapter.out.respond(&request, frames);
        });
    }

    /// The frames in `answer` that `arguments` asks for, as the protocol
    /// gives them.
    fn frames(&self, arguments: &Value, mut answer: Value) -> Value {
        let frames = match answer["frames"].take() {
            Value::Array(frames) => frames,
            _ => Vec::new(),
        };
        let total = frames.len();
        let start = arguments["startFrame"]
            .as_u64()
            .and_then(|start| usize::try_from(start).ok())
            .unwrap_or(0);
        let levels = arguments["levels"]
            .as_u64()
            .and_then(|levels| usize::try_from(levels).ok())
            .filter(|levels| *levels > 0)
            .unwrap_or(usize::MAX);
        let shown: Vec<Value> = frames
            .iter()
            .enumerate()
            .skip(start)
            .take(levels)
            .map(|(index, frame)| {
                let mut shown = json!({
                    "id": index + 1,
                    "name": frame["function"].as_str().unwrap_or("(top level)"),
                    "line": 0,
                    "column": 0,
                });
                if let (Some(file), Some(line)) = (frame["file"].as_str(), frame["line"].as_u64()) {
                    let name = std::path::Path::new(file)
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned());
                    shown["source"] = json!({ "name": name, "path": file });
                    shown["line"] = json!(self.line_out(line));
                    shown["column"] = json!(1);
                } else {
                    shown["presentationHint"] = json!("subtle");
                }
                shown
            })
            .collect();
        json!({ "stackFrames": shown, "totalFrames": total })
    }

    /// `scopes`: a frame's bindings, behind a reference.
    pub(super) fn scopes(&mut self, arguments: &Value) -> Value {
        let frame = arguments["frameId"].as_u64().unwrap_or(1).saturating_sub(1);
        let reference = self.handles.give(Handle::Frame(frame));
        json!({ "scopes": [{
            "name": "Locals",
            "presentationHint": "locals",
            "variablesReference": reference,
            "expensive": false,
        }] })
    }

    /// `variables`: a frame's bindings, or a value's parts, each with a
    /// reference to its own parts when it has any, and its place as the
    /// name the client evaluates it by.
    pub(super) fn variables(&mut self, request: Value) {
        let parts = match self.handle(&request["arguments"]) {
            Err(message) => return self.out.respond(&request, Err(message)),
            Ok(Handle::Frame(frame)) => {
                let bindings = json!({ "frame": frame, "children": true });
                return self.ask(bindings, move |adapter, answer| {
                    let variables = answer.map(|answer| adapter.bindings(frame, answer));
                    adapter.out.respond(&request, variables);
                });
            }
            Ok(Handle::Parts(parts)) => parts,
        };
        let variables: Vec<Value> = parts
            .parts
            .iter()
            .map(|part| {
                let name = part["name"].as_str().unwrap_or_default();
                self.handles.variable(
                    parts.frame,
                    parts.place_of(name),
                    name,
                    part["value"].as_str().unwrap_or_default(),
                    part["children"].clone(),
                )
            })
            .collect();
        self.out
            .respond(&request, Ok(json!({ "variables": variables })));
    }

    /// The bindings of frame `frame` that `answer` lists, as the protocol's
    /// `variables` answer.
    fn bindings(&mut self, frame: u64, mut answer: Value) -> Value {
        if let Some(why) = answer["unread"].as_str() {
            return json!({ "variables": [{
                "name": "(unread)",
                "value": why,
                "variablesReference": 0,
            }] });
        }
        let Value::Array(bindings) = answer["bindings"].take() else {
            return json!({ "variables": [] });
        };
        let variables: Vec<Value> = bindings
            .into_iter()
            .map(|mut binding| {
                let name = binding["name"].as_str().unwrap_or_default().to_owned();
                let value = binding["value"]
                    .as_str()
                    .or_else(|| binding["error"].as_str())
                    .unwrap_or_default()
                    .to_owned();
                self.handles.variable(
                    frame,
                    Place::binding(&name),
                    &name,
                    &value,
                    binding["children"].take(),
                )
            })
            .collect();
        json!({ "variables": variables })
    }

    /// What the `variablesReference` among `arguments` reads, while it
    /// holds.
    pub(super) fn handle(&self, arguments: &Value) -> Result<Handle, String> {
        let reference = arguments["variablesReference"].as_u64().unwrap_or(0);
        self.handles.get(reference).cloned().ok_or_else(|| {
            "that reference no longer holds: the program went on since it was given".to_owned()
        })
    }

    /// `evaluate`: the inspector's `eval`, as a block written before the
    /// statement the program is paused at (D1054), in the frame `frameId`
    /// names: a caller's at the statement making its call (#2309).
    pub(super) fn evaluate(&mut self, request: Value) {
        let arguments = &request["arguments"];
        let Some(code) = arguments["expression"].as_str() else {
            let refusal = Err("evaluate needs an expression".to_owned());
            return self.out.respond(&request, refusal);
        };
        let frame = arguments["frameId"].as_u64().unwrap_or(1).saturating_sub(1);
        let place = Place::expression(code);
        let code = code.to_owned();
        self.eval(frame, &code, place, move |adapter, variable| {
            let answer = variable.map(|variable| {
                // A console entry may write any place, so the client reads
                // the paused frame's variables again; the program holds the
                // one state.
                if request["arguments"]["context"] == "repl" {
                    adapter
                        .out
                        .event("invalidated", json!({ "areas": ["variables"] }));
                }
                json!({
                    "result": variable["value"],
                    "variablesReference": variable["variablesReference"],
                })
            });
            adapter.out.respond(&request, answer);
        });
    }

    /// `code` evaluated in frame `frame` of the paused program, then `then`
    /// with a protocol `Variable` for the value at `place`: the value the
    /// code ends with, and its parts behind a reference.
    pub(super) fn eval(
        &mut self,
        frame: u64,
        code: &str,
        place: Place,
        then: impl FnOnce(&mut Self, Result<Value, String>) + 'static,
    ) {
        let paused = match &self.session {
            None => Err(super::NO_PROGRAM.to_owned()),
            Some(session) => session.paused().map(|_| ()).ok_or_else(|| {
                "the program isn't paused: code is evaluated where it is paused".to_owned()
            }),
        };
        if let Err(message) = paused {
            return then(self, Err(message));
        }
        let eval = json!({
            "op": "eval",
            "code": code,
            "frame": frame,
            "children": true,
        });
        self.ask(eval, move |adapter, answer| {
            let variable = answer.map(|mut answer| {
                let value = answer["value"].as_str().unwrap_or_default().to_owned();
                adapter
                    .handles
                    .variable(frame, place, "", &value, answer["children"].take())
            });
            then(adapter, variable);
        });
    }
}
