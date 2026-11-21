//! `textDocument/hover`: the value at a position where the program is
//! paused, through the inspector's hover read (D1102.1). Anything the read
//! refuses or can't read, and any hover while the program runs, answers
//! nothing, so rust-analyzer's hover stands alone.

use std::sync::Arc;

use serde_json::{json, Value};

use super::{document, Server, Stamp};
use crate::mcp::session::Session;

impl Server {
    /// Answer the hover request `id`.
    pub(super) fn hover(&mut self, id: Value, params: &Value) {
        let Some(asked) = self.hover_request(params) else {
            return self.respond(&id, Value::Null);
        };
        let encoding = self.encoding;
        let Asked {
            session,
            stamp,
            request,
            written,
        } = asked;
        self.ask(&session, request, move |server, answer| {
            let hovered = answer
                .filter(|_| server.holds(stamp))
                .and_then(|answer| shown(&answer, &written, encoding));
            server.respond(&id, hovered.unwrap_or(Value::Null));
        });
    }

    /// What a hover at `params`' position asks the paused program: `None`
    /// when no program of the document is paused.
    fn hover_request(&mut self, params: &Value) -> Option<Asked> {
        let document = self
            .documents
            .get(params["textDocument"]["uri"].as_str()?)?;
        let path = document.path.clone();
        let line = usize::try_from(params["position"]["line"].as_u64()?).ok()? + 1;
        let written = document::line(document.text(), line).to_owned();
        let column = self
            .encoding
            .column(&written, params["position"]["character"].as_u64()?);
        let index = self.program_of(&path)?;
        let link = self.programs[index].link.as_ref()?;
        link.paused.as_ref()?;
        let request = json!({
            "op": "hover",
            "file": path,
            "line": line,
            "column": column,
            "text": written,
            "frame": 0,
        });
        Some(Asked {
            session: Arc::clone(&link.session),
            stamp: link.stamp(),
            request,
            written,
        })
    }
}

/// A hover's read, as it is sent: the session it asks, when, the request,
/// and the text of the line hovered.
struct Asked {
    session: Arc<Session>,
    stamp: Stamp,
    request: Value,
    written: String,
}

/// The hover the read `answer` shows on the line `written`: the expression
/// and its value, over the expression's range. `None` when the read gave
/// no value.
fn shown(answer: &Value, written: &str, encoding: document::Encoding) -> Option<Value> {
    let value = answer["value"].as_str()?;
    let expression = answer["expression"].as_str().unwrap_or_default();
    let mut hover = json!({
        "contents": {
            "kind": "markdown",
            "value": format!("```rust\n{expression} = {value}\n```"),
        },
    });
    let range = &answer["range"];
    if let (Some(line), Some(start), Some(end)) = (
        range["line"].as_u64(),
        range["start"].as_u64(),
        range["end"].as_u64(),
    ) {
        let at = |column: u64| encoding.character(written, usize::try_from(column).unwrap_or(0));
        hover["range"] = json!({
            "start": { "line": line.saturating_sub(1), "character": at(start) },
            "end": { "line": line.saturating_sub(1), "character": at(end) },
        });
    }
    Some(hover)
}
