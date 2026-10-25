//! Tool results, shared by core's tools and the extensions' `mcp call`.
//!
//! A result has `structuredContent` (the data) and a first text block an
//! agent can act on without reading the data, then any images.
//!
//! An extension's side of a call ([`Call`]): core runs
//! `rawr-<name> mcp call <tool>` with a JSON object on stdin holding the
//! `arguments`, the project `root` and its `target_dir`, both resolved once
//! by core, and the `pid` of the program core started for that root, if
//! any. The child writes JSON lines to stdout: any number of
//! `{"progress": n, "total": t, "message": m}` records, which core relays
//! as `notifications/progress`, then one `{"result": <tool result>}`.
//! Logs go to stderr.

use std::io::{Read as _, Write as _};
use std::path::PathBuf;

use serde_json::{json, Value};

/// A text block.
#[must_use]
pub fn text(text: impl Into<String>) -> Value {
    json!({ "type": "text", "text": text.into() })
}

/// A PNG image block.
#[must_use]
pub fn png(bytes: &[u8]) -> Value {
    json!({ "type": "image", "data": base64(bytes), "mimeType": "image/png" })
}

/// A successful result: `structured` as data, `summary` as text, then
/// `images`.
#[must_use]
pub fn success(structured: Value, summary: impl Into<String>, images: Vec<Value>) -> Value {
    let mut content = vec![text(summary)];
    content.extend(images);
    json!({ "content": content, "structuredContent": structured, "isError": false })
}

/// A failed result: `message` says what went wrong and what to do.
#[must_use]
pub fn failure(message: impl Into<String>) -> Value {
    let message = message.into();
    json!({
        "content": [text(message.clone())],
        "structuredContent": { "error": message },
        "isError": true,
    })
}

/// A failed result that carries data too.
#[must_use]
pub fn failure_with(structured: Value, summary: impl Into<String>, images: Vec<Value>) -> Value {
    let mut result = success(structured, summary, images);
    result["isError"] = Value::Bool(true);
    result
}

/// Standard base64 with padding.
#[must_use]
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let triple = chunk
            .iter()
            .enumerate()
            .fold(0_u32, |triple, (index, byte)| {
                triple | u32::from(*byte) << (16 - 8 * index)
            });
        for index in 0..4 {
            if index <= chunk.len() {
                encoded.push(char::from(
                    ALPHABET[(triple >> (18 - 6 * index) & 63) as usize],
                ));
            } else {
                encoded.push('=');
            }
        }
    }
    encoded
}

/// One extension tool call, as core hands it over on stdin.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    /// The tool's arguments as the agent gave them.
    pub arguments: Value,
    /// The project root, absolute.
    pub root: PathBuf,
    /// The project's Cargo target directory, absolute.
    pub target_dir: PathBuf,
    /// The program core started for this root, if any.
    pub pid: Option<u32>,
}

impl Call {
    /// The call core wrote to stdin.
    pub fn read() -> std::io::Result<Self> {
        let mut text = String::new();
        std::io::stdin().read_to_string(&mut text)?;
        Self::parse(&text).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "expected {\"arguments\", \"root\", \"target_dir\"} on stdin",
            )
        })
    }

    /// Parse a call; `None` when a field is missing.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(text).ok()?;
        Some(Self {
            arguments: value.get("arguments").cloned().unwrap_or(json!({})),
            root: PathBuf::from(value.get("root")?.as_str()?),
            target_dir: PathBuf::from(value.get("target_dir")?.as_str()?),
            pid: value
                .get("pid")
                .and_then(Value::as_u64)
                .and_then(|pid| u32::try_from(pid).ok()),
        })
    }

    /// What core writes to the child's stdin.
    #[must_use]
    pub fn encode(&self) -> String {
        json!({
            "arguments": self.arguments,
            "root": self.root,
            "target_dir": self.target_dir,
            "pid": self.pid,
        })
        .to_string()
    }

    /// The string argument `key`, if given.
    #[must_use]
    pub fn text(&self, key: &str) -> Option<&str> {
        self.arguments.get(key).and_then(Value::as_str)
    }
}

/// Report progress from an extension's call: `progress` of `total`.
pub fn progress(progress: f64, total: Option<f64>, message: &str) {
    line(&json!({ "progress": progress, "total": total, "message": message }));
}

/// Write the call's one result.
pub fn finish(result: &Value) {
    line(&json!({ "result": result }));
}

fn line(value: &Value) {
    let mut stdout = std::io::stdout().lock();
    let _ = writeln!(stdout, "{value}");
    let _ = stdout.flush();
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn results_carry_data_text_and_images() {
        let result = super::success(json!({ "n": 1 }), "one", vec![super::png(b"Man")]);
        assert_eq!(result["structuredContent"]["n"], 1);
        assert_eq!(result["content"][0]["text"], "one");
        assert_eq!(result["content"][1]["data"], "TWFu");
        assert_eq!(result["isError"], false);
        assert_eq!(super::failure("no")["isError"], true);
        assert_eq!(super::base64(b"Ma"), "TWE=");
        assert_eq!(super::base64(b"M"), "TQ==");
    }

    #[test]
    fn a_call_round_trips() {
        let call = super::Call {
            arguments: json!({ "text": "Increment" }),
            root: "/p".into(),
            target_dir: "/p/target".into(),
            pid: Some(7),
        };
        assert_eq!(super::Call::parse(&call.encode()), Some(call.clone()));
        assert_eq!(call.text("text"), Some("Increment"));
        assert_eq!(super::Call::parse("{\"arguments\": {}}"), None);
    }
}
