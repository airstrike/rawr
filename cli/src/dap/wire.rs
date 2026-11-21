//! The Debug Adapter Protocol's framing: each message is a JSON object
//! after a `Content-Length` header and a blank line, both ways. The
//! Language Server Protocol frames its messages the same way, and
//! `rawr lsp` reads and writes them here too.

use std::io::{BufRead, Write};
use std::sync::mpsc;

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

/// Write `message` to `stream` behind its `Content-Length` header, and
/// flush it.
///
/// # Errors
///
/// When `stream` can't be written.
pub fn frame(stream: &mut impl Write, message: &Value) -> std::io::Result<()> {
    let body = message.to_string();
    let framed = format!("Content-Length: {}\r\n\r\n{body}", body.len());
    stream.write_all(framed.as_bytes())?;
    stream.flush()
}

/// Where the adapter writes: each message numbered in the order sent,
/// whichever thread sends it. One thread owns the stream and the count,
/// and writes the messages in turn.
pub struct Writer {
    out: mpsc::Sender<Out>,
}

enum Out {
    Message(Value),
    /// Answered once every message sent before it is written.
    Flush(mpsc::Sender<()>),
}

impl Writer {
    /// Start the thread that writes to `stream`.
    ///
    /// # Errors
    ///
    /// When the thread can't start.
    pub fn new(stream: impl Write + Send + 'static) -> std::io::Result<Self> {
        let (out, messages) = mpsc::channel();
        std::thread::Builder::new()
            .name("rawr-dap-writer".into())
            .spawn(move || write(stream, &messages))?;
        Ok(Self { out })
    }

    fn send(&self, message: Value) {
        let _ = self.out.send(Out::Message(message));
    }

    /// Wait for every message sent so far to be written, or for the client
    /// to go away.
    pub fn flush(&self) {
        let (written, flushed) = mpsc::channel();
        if self.out.send(Out::Flush(written)).is_ok() {
            let _ = flushed.recv();
        }
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

/// Write each message to `stream`, numbered next, for as long as a sender
/// is left. A client that went away hears nothing more, and a flush is
/// answered at once.
fn write(mut stream: impl Write, messages: &mpsc::Receiver<Out>) {
    let mut seq: u64 = 0;
    let mut open = true;
    for out in messages {
        match out {
            Out::Message(mut message) if open => {
                seq += 1;
                message["seq"] = json!(seq);
                open = frame(&mut stream, &message).is_ok();
            }
            Out::Message(_) => {}
            Out::Flush(written) => {
                let _ = written.send(());
            }
        }
    }
}
