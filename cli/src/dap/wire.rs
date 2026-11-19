//! The Debug Adapter Protocol's framing: each message is a JSON object
//! after a `Content-Length` header and a blank line, both ways.

use std::io::{BufRead, Write};
use std::sync::{Mutex, PoisonError};

use serde_json::{json, Value};

/// The next message on `input`: `None` at its end. A message whose body
/// isn't JSON is skipped.
///
/// # Errors
///
/// When `input` can't be read.
pub fn read(input: &mut impl BufRead) -> std::io::Result<Option<Value>> {
    loop {
        let mut length = None;
        loop {
            let mut header = String::new();
            if input.read_line(&mut header)? == 0 {
                return Ok(None);
            }
            let header = header.trim_end();
            if header.is_empty() {
                if length.is_some() {
                    break;
                }
                continue;
            }
            if let Some((name, value)) = header.split_once(':')
                && name.trim().eq_ignore_ascii_case("content-length")
            {
                length = value.trim().parse::<usize>().ok();
            }
        }
        let mut body = vec![0_u8; length.unwrap_or_default()];
        input.read_exact(&mut body)?;
        if let Ok(message) = serde_json::from_slice(&body) {
            return Ok(Some(message));
        }
    }
}

/// Where the adapter writes: each message numbered in the order written,
/// whichever thread writes it.
pub struct Writer {
    out: Mutex<Out>,
}

struct Out {
    stream: Box<dyn Write + Send>,
    seq: u64,
}

impl Writer {
    #[must_use]
    pub fn new(stream: impl Write + Send + 'static) -> Self {
        Self {
            out: Mutex::new(Out {
                stream: Box::new(stream),
                seq: 0,
            }),
        }
    }

    /// Write `message`, numbered next. A client that went away hears
    /// nothing more.
    fn send(&self, mut message: Value) {
        let mut out = self.out.lock().unwrap_or_else(PoisonError::into_inner);
        out.seq += 1;
        message["seq"] = json!(out.seq);
        let body = message.to_string();
        let framed = format!("Content-Length: {}\r\n\r\n{body}", body.len());
        let _ = out
            .stream
            .write_all(framed.as_bytes())
            .and_then(|()| out.stream.flush());
    }

    /// An event, `name` with `body`.
    pub fn event(&self, name: &str, body: Value) {
        self.send(json!({ "type": "event", "event": name, "body": body }));
    }

    /// The answer to `request`: its body, or why it failed.
    pub fn respond(&self, request: &Value, answer: Result<Value, String>) {
        let mut response = json!({
            "type": "response",
            "request_seq": request["seq"],
            "command": request["command"],
        });
        match answer {
            Ok(body) => {
                response["success"] = json!(true);
                response["body"] = body;
            }
            Err(message) => {
                response["success"] = json!(false);
                response["message"] = json!(message);
            }
        }
        self.send(response);
    }
}
