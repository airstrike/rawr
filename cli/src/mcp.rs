//! `rawr mcp`: a Model Context Protocol server on stdio for coding agents
//! (D537). It is `rawr`'s own, whatever is installed.
//!
//! JSON-RPC 2.0, one message per line, with the standard library only
//! (D430.4). The main thread reads; each `tools/call` runs on a worker, so
//! `ping`, `notifications/cancelled` and the end of input are handled while
//! a long build runs; one writer thread writes every message. A call waits
//! for what it needs as long as that takes, telling its progress where it
//! has any; the client's cancel ends it, with its process groups.
//!
//! Core's tools make, check and run programs; installed extensions add
//! tools their manifests declare ([`extension`]). The programs `start`
//! runs are the server's ([`session`]): the end of input ends them, and an
//! owner whose server died ends itself ([`crate::events`]). The server is
//! one shell of a run's debugger session (D1053.2): its tools also reach a
//! run started elsewhere, as in a terminal.

mod check;
pub mod content;
mod extension;
pub(crate) mod process;
pub(crate) mod session;
pub(crate) mod tools;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex, PoisonError};

use serde_json::{json, Value};

/// The protocol version answered when the client asks for one this server
/// doesn't know.
pub const PROTOCOL: &str = "2025-06-18";
const PROTOCOLS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// The server: where relative paths start, the writer, the calls in
/// flight and the programs it reaches.
pub(crate) struct Server {
    root: PathBuf,
    out: mpsc::Sender<Value>,
    calls: Mutex<HashMap<String, Arc<process::Cancel>>>,
    sessions: session::Sessions,
    checks: check::Checks,
    /// The tool names last listed, to tell the client when installs change
    /// them.
    listed: Mutex<Vec<String>>,
}

impl Server {
    fn send(&self, message: Value) {
        let _ = self.out.send(message);
    }

    fn reply(&self, id: Value, result: Value) {
        self.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    }

    fn error(&self, id: Value, code: i64, message: &str) {
        self.send(
            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }),
        );
    }

    /// End every call and every program the server started.
    fn shut_down(&self) {
        for (_, call) in self
            .calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain()
        {
            call.cancel();
        }
        self.sessions.stop_all();
        self.checks.stop_all();
    }
}

/// Serve MCP on stdin and stdout until stdin closes. Relative paths in
/// calls start at `root`.
pub fn serve(root: PathBuf) -> std::io::Result<()> {
    serve_on(root, std::io::stdin().lock(), std::io::stdout())
}

/// [`serve`] over any reader and writer.
pub fn serve_on(
    root: PathBuf,
    input: impl std::io::BufRead,
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
    let server = Server {
        root,
        out,
        calls: Mutex::new(HashMap::new()),
        sessions: session::Sessions::default(),
        checks: check::Checks::default(),
        listed: Mutex::new(Vec::new()),
    };
    // Each call runs on a thread of this scope, and the scope ends once
    // every call has seen its cancel and returned.
    std::thread::scope(|calls| {
        for line in input.lines() {
            let Ok(line) = line else {
                break;
            };
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Value>(&line) {
                Ok(message) => handle(&server, calls, message),
                Err(error) => server.send(json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": { "code": -32700, "message": error.to_string() },
                })),
            }
        }
        // The client went away: what the server started goes too.
        server.shut_down();
    });
    // The writer ends with the server's sender.
    drop(server);
    let _ = writer.join();
    Ok(())
}

fn handle<'scope>(
    server: &'scope Server,
    calls: &'scope std::thread::Scope<'scope, '_>,
    message: Value,
) {
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        // A response to nothing the server asked.
        return;
    };
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let Some(id) = message.get("id").cloned() else {
        if method == "notifications/cancelled"
            && let Some(request) = params.get("requestId")
        {
            let call = server
                .calls
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&request.to_string());
            if let Some(call) = call {
                call.cancel();
            }
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
            *server.listed.lock().unwrap_or_else(PoisonError::into_inner) = names(&listed);
            server.reply(id, json!({ "tools": listed }));
        }
        "tools/call" => {
            let cancel = Arc::new(process::Cancel::default());
            let key = id.to_string();
            server
                .calls
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(key.clone(), Arc::clone(&cancel));
            let spawned = std::thread::Builder::new()
                .name("rawr-mcp-call".into())
                .spawn_scoped(calls, move || {
                    // A call that panics gets no answer and the server goes
                    // on: its unwind ends here, so the scope's end doesn't
                    // raise it again.
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        tools::call(server, &params, &cancel)
                    }));
                    server
                        .calls
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .remove(&key);
                    // An install the call saw is told before its answer.
                    tell_changes(server);
                    // A cancelled request gets no answer.
                    if let Ok(result) = result
                        && !cancel.is_cancelled()
                    {
                        server.reply(id, result);
                    }
                });
            if let Err(error) = spawned {
                server.error(Value::Null, -32603, &error.to_string());
            }
        }
        method => server.error(id, -32601, &format!("unknown method `{method}`")),
    }
}

fn names(tools: &[Value]) -> Vec<String> {
    tools
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
        .collect()
}

/// Tell the client when an install or removal changed the tools since it
/// last listed them.
fn tell_changes(server: &Server) {
    let mut listed = server.listed.lock().unwrap_or_else(PoisonError::into_inner);
    if listed.is_empty() {
        return;
    }
    let now = names(&tools::list());
    if now != *listed {
        *listed = now;
        server.send(json!({ "jsonrpc": "2.0", "method": "notifications/tools/list_changed" }));
    }
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
}
