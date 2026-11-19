//! What the client reads where the program is paused: its frames, each
//! frame's bindings, their parts, and code evaluated there.

use serde_json::{json, Value};

use super::handles::Handle;
use super::Adapter;

impl Adapter {
    /// `stackTrace`: the program's frames where it is paused, innermost
    /// first, each named by its function's Rust path. A frame's id is its
    /// index from 1. A program that isn't paused has none.
    pub(super) fn stack(&self, arguments: &Value) -> Result<Value, String> {
        if self.session()?.paused().is_none() {
            return Ok(json!({ "stackFrames": [], "totalFrames": 0 }));
        }
        let mut answer = self.ask(&json!({ "frame": 0 }))?;
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
        Ok(json!({ "stackFrames": shown, "totalFrames": total }))
    }

    /// `scopes`: a frame's bindings, behind a reference.
    pub(super) fn scopes(&self, arguments: &Value) -> Value {
        let frame = arguments["frameId"].as_u64().unwrap_or(1).saturating_sub(1);
        let reference = self.handles().give(Handle::Frame(frame));
        json!({ "scopes": [{
            "name": "Locals",
            "presentationHint": "locals",
            "variablesReference": reference,
            "expensive": false,
        }] })
    }

    /// `variables`: a frame's bindings, or a value's parts, each with a
    /// reference to its own parts when it has any.
    pub(super) fn variables(&self, arguments: &Value) -> Result<Value, String> {
        let reference = arguments["variablesReference"].as_u64().unwrap_or(0);
        let handle = self
            .handles()
            .get(reference)
            .cloned()
            .ok_or("that reference no longer holds: the program went on since it was given")?;
        let variables: Vec<Value> = match handle {
            Handle::Frame(frame) => {
                let mut answer = self.ask(&json!({ "frame": frame, "children": true }))?;
                if let Some(why) = answer["unread"].as_str() {
                    return Ok(json!({ "variables": [{
                        "name": "(unread)",
                        "value": why,
                        "variablesReference": 0,
                    }] }));
                }
                let Value::Array(bindings) = answer["bindings"].take() else {
                    return Ok(json!({ "variables": [] }));
                };
                let mut handles = self.handles();
                bindings
                    .into_iter()
                    .map(|mut binding| {
                        let name = binding["name"].as_str().unwrap_or_default().to_owned();
                        let value = binding["value"]
                            .as_str()
                            .or_else(|| binding["error"].as_str())
                            .unwrap_or_default()
                            .to_owned();
                        let mut variable =
                            handles.variable(&name, &value, binding["children"].take());
                        variable["evaluateName"] = json!(name);
                        variable
                    })
                    .collect()
            }
            Handle::Parts(parts) => {
                let mut handles = self.handles();
                parts
                    .into_iter()
                    .map(|mut part| {
                        let children = part["children"].take();
                        handles.variable(
                            part["name"].as_str().unwrap_or_default(),
                            part["value"].as_str().unwrap_or_default(),
                            children,
                        )
                    })
                    .collect()
            }
        };
        Ok(json!({ "variables": variables }))
    }

    /// `evaluate`: the inspector's `eval`, as a block written before the
    /// statement the program is paused at (D1054), in the frame `frameId`
    /// names: a caller's at the statement making its call (#2309).
    pub(super) fn evaluate(&self, arguments: &Value) -> Result<Value, String> {
        let code = arguments["expression"]
            .as_str()
            .ok_or("evaluate needs an expression")?;
        if self.session()?.paused().is_none() {
            return Err(
                "the program isn't paused: code is evaluated where it is paused".to_owned(),
            );
        }
        let frame = arguments["frameId"].as_u64().unwrap_or(1).saturating_sub(1);
        let mut answer = self.ask(&json!({
            "op": "eval",
            "code": code,
            "frame": frame,
            "children": true,
        }))?;
        let value = answer["value"].as_str().unwrap_or_default().to_owned();
        let variable = self
            .handles()
            .variable("", &value, answer["children"].take());
        Ok(json!({
            "result": value,
            "variablesReference": variable["variablesReference"],
        }))
    }
}
