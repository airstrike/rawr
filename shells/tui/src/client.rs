//! A JSON-RPC client on a run's debugger session: one JSON message per
//! line each way. Requests go out numbered; what the session says comes in
//! on a thread of its own, as [`Message`]s on the shell's channel.

use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::UnixStream;
use std::sync::mpsc;

use serde_json::{json, Value};

/// The error a `program` request ends with when no program will answer it.
pub const NOT_RUNNING: i64 = -32001;

/// What the session says.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// The answer to request `id`.
    Answer { id: u64, reply: Reply },
    /// Something happened to the program, numbered by the session.
    Event(Value),
    /// A piece of the run's output, from the shell that holds it.
    Output { stream: String, text: String },
    /// The session's connection closed.
    Closed,
}

/// How the session answered a request.
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    Result(Value),
    Error { code: i64, message: String },
}

/// The writing half of a connection to the session.
#[derive(Debug)]
pub struct Client {
    stream: UnixStream,
    next: u64,
}

impl Client {
    /// A client on `stream`, whose messages reach `heard` as `wrap` makes
    /// them, until it closes.
    ///
    /// # Errors
    ///
    /// When the stream can't be shared with the thread that reads it.
    pub fn new<T: Send + 'static>(
        stream: UnixStream,
        heard: mpsc::Sender<T>,
        wrap: fn(Message) -> T,
    ) -> std::io::Result<Self> {
        let reading = stream.try_clone()?;
        std::thread::Builder::new()
            .name("rawr-tui-session".into())
            .spawn(move || listen(reading, &heard, wrap))?;
        Ok(Self { stream, next: 0 })
    }

    /// Send `method` with `params`, answered by the id returned.
    pub fn request(&mut self, method: &str, params: Value) -> u64 {
        let id = self.next;
        self.next += 1;
        self.write(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        id
    }

    /// Send the notification `method` with `params`.
    pub fn notify(&mut self, method: &str, params: Value) {
        self.write(&json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    fn write(&mut self, message: &Value) {
        // A closed session is heard as [`Message::Closed`] by the reader.
        let _ = writeln!(self.stream, "{message}");
    }
}

/// Read the session's lines until it closes.
fn listen<T>(stream: UnixStream, heard: &mpsc::Sender<T>, wrap: fn(Message) -> T) {
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else {
            break;
        };
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(message) = parse(message)
            && heard.send(wrap(message)).is_err()
        {
            return;
        }
    }
    let _ = heard.send(wrap(Message::Closed));
}

/// One line from the session, as a message: `None` for one a shell
/// doesn't take.
fn parse(mut message: Value) -> Option<Message> {
    match (message["id"].as_u64(), message["method"].as_str()) {
        (Some(id), None) => {
            let reply = match message.get_mut("result") {
                Some(result) => Reply::Result(result.take()),
                None => Reply::Error {
                    code: message["error"]["code"].as_i64().unwrap_or_default(),
                    message: message["error"]["message"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                },
            };
            Some(Message::Answer { id, reply })
        }
        (None, Some("event")) => Some(Message::Event(message["params"].take())),
        (None, Some("output")) => Some(Message::Output {
            stream: message["params"]["stream"]
                .as_str()
                .unwrap_or("stdout")
                .to_owned(),
            text: message["params"]["text"].as_str()?.to_owned(),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{parse, Message, Reply};

    #[test]
    fn a_line_becomes_an_answer_an_event_or_output() {
        assert_eq!(
            parse(json!({ "jsonrpc": "2.0", "id": 3, "result": { "paused": null } })),
            Some(Message::Answer {
                id: 3,
                reply: Reply::Result(json!({ "paused": null }))
            })
        );
        assert_eq!(
            parse(json!({ "id": 4, "error": { "code": -32001, "message": "no" } })),
            Some(Message::Answer {
                id: 4,
                reply: Reply::Error {
                    code: -32001,
                    message: "no".to_owned()
                }
            })
        );
        assert_eq!(
            parse(json!({ "method": "event", "params": { "event": "running", "seq": 1 } })),
            Some(Message::Event(json!({ "event": "running", "seq": 1 })))
        );
        assert_eq!(
            parse(json!({ "method": "output", "params": { "stream": "stderr", "text": "x" } })),
            Some(Message::Output {
                stream: "stderr".to_owned(),
                text: "x".to_owned()
            })
        );
        assert_eq!(parse(json!({ "method": "serve" })), None);
    }
}
