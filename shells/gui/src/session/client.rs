//! The shell's side of the session's JSON-RPC: requests numbered as they
//! go, each kept with what the shell asked it for, and the lines that come
//! back read as replies, events and output.

use std::collections::HashMap;

use serde_json::{json, Value};

/// The error a `program` request ends with when no program will answer it.
pub const NOT_RUNNING: i64 = -32001;

/// The requests a shell made that wait for their replies, each with the
/// `K` it was asked for.
#[derive(Debug)]
pub struct Client<K> {
    next: u64,
    pending: HashMap<u64, K>,
}

/// What one line from the session says.
#[derive(Debug, PartialEq)]
pub enum Incoming<K> {
    /// The reply to the request made for `kind`.
    Reply {
        kind: K,
        result: Result<Value, Error>,
    },
    /// One of the session's events, with its `seq`.
    Event(Value),
    /// A piece of the program's output.
    Output { stream: String, text: String },
}

/// A request's error, as the session gave it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    /// The JSON-RPC error code: [`NOT_RUNNING`] when no program will
    /// answer.
    pub code: i64,
    /// What went wrong.
    pub message: String,
}

impl<K> Default for Client<K> {
    fn default() -> Self {
        Self {
            next: 1,
            pending: HashMap::new(),
        }
    }
}

impl<K> Client<K> {
    /// The line that asks `method` with `params`, kept as `kind` until its
    /// reply comes.
    #[must_use]
    pub fn request(&mut self, method: &str, params: Value, kind: K) -> String {
        let id = self.next;
        self.next += 1;
        self.pending.insert(id, kind);
        format!(
            "{}\n",
            json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
        )
    }

    /// The line that asks the program `request` through the session.
    #[must_use]
    pub fn program(&mut self, request: Value, kind: K) -> String {
        self.request("program", request, kind)
    }

    /// What `line` from the session says: `None` for a line that is none of
    /// a reply to a request of this client's, an event or output.
    pub fn receive(&mut self, line: &str) -> Option<Incoming<K>> {
        let mut message: Value = serde_json::from_str(line).ok()?;
        if let Some(id) = message["id"].as_u64() {
            let kind = self.pending.remove(&id)?;
            let result = match message.get_mut("result") {
                Some(result) => Ok(result.take()),
                None => Err(Error {
                    code: message["error"]["code"].as_i64().unwrap_or_default(),
                    message: message["error"]["message"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                }),
            };
            return Some(Incoming::Reply { kind, result });
        }
        let params = message["params"].take();
        match message["method"].as_str()? {
            "event" => Some(Incoming::Event(params)),
            "output" => Some(Incoming::Output {
                stream: params["stream"].as_str().unwrap_or("stdout").to_owned(),
                text: params["text"].as_str()?.to_owned(),
            }),
            _ => None,
        }
    }

    /// Forget every request still waiting: the connection closed.
    pub fn abandon(&mut self) {
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::{Client, Error, Incoming, NOT_RUNNING};

    #[test]
    fn a_reply_comes_back_as_the_kind_it_was_asked_for() {
        let mut client = Client::default();
        let hello: Value =
            serde_json::from_str(&client.request("hello", json!({}), "hello")).unwrap();
        let step: Value =
            serde_json::from_str(&client.program(json!({ "op": "step" }), "step")).unwrap();
        assert_eq!(step["params"]["op"], "step");
        let refused = json!({
            "jsonrpc": "2.0", "id": step["id"],
            "error": { "code": NOT_RUNNING, "message": "the program isn't running" },
        });
        assert_eq!(
            client.receive(&refused.to_string()),
            Some(Incoming::Reply {
                kind: "step",
                result: Err(Error {
                    code: NOT_RUNNING,
                    message: "the program isn't running".to_owned()
                }),
            })
        );
        let greeted = json!({ "jsonrpc": "2.0", "id": hello["id"], "result": { "protocol": 1 } });
        assert!(matches!(
            client.receive(&greeted.to_string()),
            Some(Incoming::Reply {
                kind: "hello",
                result: Ok(_)
            })
        ));
        // A reply already taken is no one's.
        assert_eq!(client.receive(&greeted.to_string()), None);
        let told = json!({ "jsonrpc": "2.0", "method": "event", "params": { "event": "running", "seq": 1 } });
        assert_eq!(
            client.receive(&told.to_string()),
            Some(Incoming::Event(json!({ "event": "running", "seq": 1 })))
        );
    }
}
