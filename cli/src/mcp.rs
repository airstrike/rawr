//! `rawr mcp`: a Model Context Protocol server on stdio for coding agents
//! (D537). It is `rawr`'s own, whatever is installed.
//!
//! JSON-RPC 2.0, one message per line, with the standard library only
//! (D430.4). A reader thread forwards each line, and the main thread loops
//! over the lines, the calls that returned, the checks and the sessions,
//! owning the calls in flight, the tools last listed, and the check and the
//! session of each root. Each `tools/call` runs on a worker, so `ping`,
//! `notifications/cancelled` and the end of input are handled while a long
//! build runs; one writer thread writes every message. A call waits for
//! what it needs as long as that takes, on a channel of its own
//! ([`call`]), telling its progress where it has any; the client's cancel
//! is a message on that channel, and ends the call with its process
//! groups.
//!
//! Core's tools make, check and run programs; installed extensions add
//! tools their manifests declare ([`extension`]). The programs `start`
//! runs are the server's ([`session`]): the end of input ends them, and an
//! owner whose server died ends itself ([`crate::events`]). The server is
//! one shell of a run's debugger session (D1053.2): its tools also reach a
//! run started elsewhere, as in a terminal.

pub(crate) mod call;
mod check;
pub mod content;
mod extension;
pub(crate) mod options;
pub(crate) mod process;
pub(crate) mod session;
mod sessions;
pub(crate) mod tools;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;

use serde_json::{json, Value};

/// The protocol version answered when the client asks for one this server
/// doesn't know.
pub const PROTOCOL: &str = "2025-06-18";
const PROTOCOLS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// The server: where relative paths start, the writer and the main loop.
pub(crate) struct Server {
    root: PathBuf,
    out: mpsc::Sender<Value>,
    /// What the main loop hears from calls.
    heard: mpsc::Sender<Heard>,
    /// What Cargo answered the calls' discoveries, for as long as the
    /// server runs. Each call runs on its own thread and discovers its
    /// program there, so the calls share the answers behind one lock.
    answers: std::sync::Mutex<rawr::project::cargo::Answers>,
}

/// What the main loop hears.
enum Heard {
    /// A line of input.
    Line(String),
    /// The input ended.
    Ended,
    /// A call returned: its answer, unless it panicked or was cancelled.
    Returned { id: Value, answer: Option<Value> },
    /// A call asks for a check.
    Check(check::Request),
    /// A check is done.
    Checked(check::Report),
    /// A call asks about the sessions.
    Sessions(sessions::Request),
}

/// The main loop's state, and how it starts calls.
struct Loop<'scope, 'env> {
    server: &'scope Server,
    scope: &'scope std::thread::Scope<'scope, 'env>,
    /// The calls in flight, by their ids, for the client's cancels.
    calls: HashMap<String, call::Waker>,
    /// The tool names last listed, to tell the client when installs change
    /// them.
    listed: Vec<String>,
    checks: check::Checks,
    /// The programs the server reaches.
    sessions: sessions::Sessions,
}

impl Server {
    /// The calls' Cargo answers, for one discovery.
    pub(crate) fn answers(&self) -> std::sync::MutexGuard<'_, rawr::project::cargo::Answers> {
        self.answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn send(&self, message: Value) {
        let _ = self.out.send(message);
    }

    /// Tell the main loop `message`. Once the loop stopped reading, every
    /// call was cancelled, so nothing waits for an answer.
    fn ask(&self, message: Heard) {
        let _ = self.heard.send(message);
    }

    fn reply(&self, id: Value, result: Value) {
        self.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    }

    fn error(&self, id: Value, code: i64, message: &str) {
        self.send(
            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }),
        );
    }
}

/// Serve MCP on stdin and stdout until stdin closes. Relative paths in
/// calls start at `root`.
pub fn serve(root: PathBuf) -> std::io::Result<()> {
    serve_on(
        root,
        std::io::BufReader::new(std::io::stdin()),
        std::io::stdout(),
    )
}

/// [`serve`] over any reader and writer.
pub fn serve_on(
    root: PathBuf,
    input: impl std::io::BufRead + Send,
    output: impl std::io::Write + Send + 'static,
) -> std::io::Result<()> {
    let (out, messages) = mpsc::channel::<Value>();
    let writer = std::thread::Builder::new()
        .name("rawr-mcp-writer".into())
        .spawn(move || {
            let mut output = output;
            for message in messages {
                if writeln!(output, "{message}")
                    .and_then(|()| output.flush())
                    .is_err()
                {
                    break;
                }
            }
        })?;
    let (heard, hearing) = mpsc::channel::<Heard>();
    let server = Server {
        root,
        out,
        heard,
        answers: std::sync::Mutex::new(rawr::project::cargo::Answers::new()),
    };
    // Each call runs on a thread of this scope, and the scope ends once
    // every call has seen its cancel and returned.
    let served = std::thread::scope(|scope| {
        let reading = server.heard.clone();
        std::thread::Builder::new()
            .name("rawr-mcp-read".into())
            .spawn_scoped(scope, move || {
                for line in input.lines() {
                    let Ok(line) = line else {
                        break;
                    };
                    if reading.send(Heard::Line(line)).is_err() {
                        return;
                    }
                }
                let _ = reading.send(Heard::Ended);
            })?;
        let mut serving = Loop {
            server: &server,
            scope,
            calls: HashMap::new(),
            listed: Vec::new(),
            checks: check::Checks::default(),
            sessions: sessions::Sessions::default(),
        };
        for message in &hearing {
            match message {
                Heard::Line(line) => serving.line(&line),
                Heard::Returned { id, answer } => serving.returned(id, answer),
                Heard::Check(request) => serving.checks.ask(request, &server.heard),
                Heard::Checked(report) => serving.checks.finished(report),
                Heard::Sessions(request) => serving.sessions.heard(request),
                Heard::Ended => break,
            }
        }
        // The client went away: what the server started goes too.
        serving.shut_down();
        Ok::<(), std::io::Error>(())
    });
    // A call that started a session as the server ended told the loop
    // after it stopped reading: that session ends too.
    let mut late = sessions::Sessions::default();
    for message in hearing.try_iter() {
        if let Heard::Sessions(request) = message {
            late.heard(request);
        }
    }
    late.stop_all();
    // The writer ends with the server's sender.
    drop(server);
    let _ = writer.join();
    served
}

impl<'scope> Loop<'scope, '_> {
    fn line(&mut self, line: &str) {
        if line.trim().is_empty() {
            return;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(message) => self.handle(message),
            Err(error) => self.server.send(json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": { "code": -32700, "message": error.to_string() },
            })),
        }
    }

    fn handle(&mut self, message: Value) {
        let server = self.server;
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            // A response to nothing the server asked.
            return;
        };
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let Some(id) = message.get("id").cloned() else {
            if method == "notifications/cancelled"
                && let Some(request) = params.get("requestId")
                && let Some(call) = self.calls.remove(&request.to_string())
            {
                call.cancel();
            }
            return;
        };
        match method {
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(Value::as_str);
                let version = asked
                    .filter(|asked| PROTOCOLS.contains(asked))
                    .unwrap_or(PROTOCOL);
                server.reply(
                    id,
                    json!({
                        "protocolVersion": version,
                        "capabilities": { "tools": { "listChanged": true } },
                        "serverInfo": { "name": "rawr", "version": rawr::VERSION },
                        "instructions": tools::instructions(&server.root),
                    }),
                );
            }
            "ping" => server.reply(id, json!({})),
            "tools/list" => {
                let listed = tools::list();
                self.listed = names(&listed);
                server.reply(id, json!({ "tools": listed }));
            }
            "tools/call" => self.call(id, params),
            method => server.error(id, -32601, &format!("unknown method `{method}`")),
        }
    }

    /// Run one `tools/call` on a thread of the scope, which says when it
    /// returned.
    fn call(&mut self, id: Value, params: Value) {
        let server = self.server;
        let call = call::Call::default();
        self.calls.insert(id.to_string(), call.waker());
        let heard = server.heard.clone();
        let spawned = std::thread::Builder::new()
            .name("rawr-mcp-call".into())
            .spawn_scoped(self.scope, move || {
                // A call that panics gets no answer and the server goes
                // on: its unwind ends here, so the scope's end doesn't
                // raise it again.
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    tools::call(server, &params, &call)
                }));
                // A cancelled request gets no answer.
                let answer = result.ok().filter(|_| !call.is_cancelled());
                let _ = heard.send(Heard::Returned { id, answer });
            });
        if let Err(error) = spawned {
            server.error(Value::Null, -32603, &error.to_string());
        }
    }

    /// A call returned: an install it saw is told before its answer.
    fn returned(&mut self, id: Value, answer: Option<Value>) {
        self.calls.remove(&id.to_string());
        self.tell_changes();
        if let Some(answer) = answer {
            self.server.reply(id, answer);
        }
    }

    /// Tell the client when an install or removal changed the tools since
    /// it last listed them.
    fn tell_changes(&mut self) {
        if self.listed.is_empty() {
            return;
        }
        let now = names(&tools::list());
        if now != self.listed {
            self.listed = now;
            self.server
                .send(json!({ "jsonrpc": "2.0", "method": "notifications/tools/list_changed" }));
        }
    }

    /// End every call and every program the server started.
    fn shut_down(&mut self) {
        for (_, call) in self.calls.drain() {
            call.cancel();
        }
        self.sessions.stop_all();
        self.checks.stop_all();
    }
}

fn names(tools: &[Value]) -> Vec<String> {
    tools
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
        .collect()
}

/// Relay progress for a request that asked for it.
pub(crate) struct Progress<'a> {
    server: &'a Server,
    token: Option<Value>,
}

impl Progress<'_> {
    fn send(&self, progress: f64, total: Option<f64>, message: &str) {
        let Some(token) = &self.token else {
            return;
        };
        let mut params =
            json!({ "progressToken": token, "progress": progress, "message": message });
        if let Some(total) = total {
            params["total"] = json!(total);
        }
        self.server.send(
            json!({ "jsonrpc": "2.0", "method": "notifications/progress", "params": params }),
        );
    }

    /// Relay what a build's status line says each time it changes. A build
    /// has no total, so each update is numbered, as progress must grow.
    fn building(&self) -> impl FnMut(&str) + '_ {
        let mut told = 0_u32;
        move |status| {
            told += 1;
            self.send(f64::from(told), None, status);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead as _, BufReader};
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;

    use serde_json::{json, Value};

    /// The loop answers each request it reads and ends with its input. A
    /// cancel of a call it doesn't know changes nothing.
    #[test]
    fn the_loop_answers_requests_and_ends_with_its_input() {
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":9}}"#,
            "\n",
            "not json\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"nothing"}"#,
            "\n",
        );
        let (written, read) = UnixStream::pair().unwrap();
        super::serve_on(PathBuf::from("/p"), input.as_bytes(), written).unwrap();
        let heard: Vec<Value> = BufReader::new(read)
            .lines()
            .map(|line| serde_json::from_str(&line.unwrap()).unwrap())
            .collect();
        assert_eq!(heard.len(), 3, "{heard:?}");
        assert_eq!(heard[0]["id"], 1);
        assert_eq!(heard[0]["result"], json!({}));
        assert_eq!(heard[1]["error"]["code"], -32700);
        assert_eq!(heard[2]["id"], 2);
        assert_eq!(heard[2]["error"]["code"], -32601);
    }
}
