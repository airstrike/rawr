//! `rawr lsp`: a narrow language server on stdio that runs beside
//! rust-analyzer, one more shell of a run's debugger session (D1053.2,
//! D1102.3). rust-analyzer keeps types, completion and navigation; this
//! server says only what a run knows:
//! - `textDocument/hover`: the value at the position while the program is
//!   paused, through the inspector's hover read, which never runs a call
//!   (D1102.1). It answers nothing while the program runs.
//! - `textDocument/inlayHint`: the paused frame's bindings at the end of
//!   the lines above the pause that name them, each read at that name.
//! - `textDocument/publishDiagnostics`: why the last save didn't swap in,
//!   as rawr's own diagnostics (source `rawr`), until a save does.
//!
//! Each program the editor opens a file of is attached to its newest run
//! ([`crate::events::session::Runs`]). With no run live the server stays
//! up and answers nothing; it watches the user's session directory and
//! attaches once a run's socket appears there, and looks again at each
//! request.
//!
//! One thread owns the server: the client's documents, the programs and
//! their runs. It hears the client's messages, the sessions' answers and
//! the watcher on one inbox, and blocks on none of them. Each request to a
//! run is stamped with the run's moment, which every pause and resume
//! moves, so an answer that comes after the program went on is dropped
//! (D1096.3, D1097.4).

mod diagnostics;
mod document;
mod hints;
mod hover;
mod link;
mod serve;

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{mpsc, Arc};

use serde_json::{json, Value};

use crate::mcp::session::{Ear, Refusal, Session, Told, Wait};

pub use serve::{serve, serve_on};

/// What the server's thread hears.
enum Heard {
    /// A message from the client.
    Message(Value),
    /// The client's input ended.
    Ended,
    /// A session told the server's request or wait `id`.
    Session { id: u64, told: Told },
    /// A session socket appeared or went at one of these paths.
    Sockets(Vec<PathBuf>),
}

/// What the server does once a session told one of its requests or waits.
type Then = Box<dyn FnOnce(&mut Server, Told)>;

/// When a request to a run was sent: the run's link and its moment then.
/// An answer counts only while both still hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    link: u64,
    moment: u64,
}

/// One client, its documents, and the programs they belong to.
struct Server {
    out: Box<dyn Write + Send>,
    /// The server's inbox, where the sessions and the watcher tell it.
    heard: mpsc::Sender<Heard>,
    /// The path `rawr lsp` was given, which names the first program over
    /// the client's root.
    given: Option<PathBuf>,
    /// The client sent `initialize`, then `shutdown`.
    initialized: bool,
    shut: bool,
    /// The client sent `exit`, or its input ended.
    over: bool,
    encoding: document::Encoding,
    /// The client takes `workspace/inlayHint/refresh`.
    refreshes: bool,
    documents: HashMap<String, document::Document>,
    programs: Vec<link::Program>,
    /// Directories found to hold no program, so a file opened there isn't
    /// looked up again.
    unserved: HashSet<PathBuf>,
    /// How each request or wait on a session goes on, by its number.
    then: HashMap<u64, Then>,
    /// The links made so far, each numbered by the count.
    links: u64,
    /// The requests the server sent the client, each numbered by the count.
    asked: u64,
    /// The watcher on the user's session directory, while it watches.
    watcher: Option<link::Watcher>,
}

impl Server {
    fn new(out: Box<dyn Write + Send>, heard: mpsc::Sender<Heard>, given: Option<PathBuf>) -> Self {
        Self {
            out,
            heard,
            given,
            initialized: false,
            shut: false,
            over: false,
            encoding: document::Encoding::Utf16,
            refreshes: false,
            documents: HashMap::new(),
            programs: Vec::new(),
            unserved: HashSet::new(),
            then: HashMap::new(),
            links: 0,
            asked: 0,
            watcher: None,
        }
    }

    /// Write `message` to the client, as JSON-RPC 2.0. A client that went
    /// away ends the server.
    fn send(&mut self, mut message: Value) {
        message["jsonrpc"] = json!("2.0");
        if crate::dap::wire::frame(&mut self.out, &message).is_err() {
            self.over = true;
        }
    }

    /// The answer to the client's request `id`.
    fn respond(&mut self, id: &Value, result: Value) {
        self.send(json!({ "id": id, "result": result }));
    }

    /// A notification to the client.
    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({ "method": method, "params": params }));
    }

    /// An ear on the server's inbox.
    fn ear(&self) -> Ear {
        let heard = self.heard.clone();
        Ear::new(move |id, told| heard.send(Heard::Session { id, told }).is_ok())
    }

    /// Wait for `wait` of `session`, then go on with `then`.
    fn park(
        &mut self,
        session: &Session,
        wait: Wait,
        then: impl FnOnce(&mut Self, Told) + 'static,
    ) {
        let id = session.park(wait, self.ear());
        self.then.insert(id, Box::new(then));
    }

    /// Ask the program of `session` `request`, then go on with `then` and
    /// its answer: `None` when the session or the program gave none.
    fn ask(
        &mut self,
        session: &Arc<Session>,
        request: Value,
        then: impl FnOnce(&mut Self, Option<Value>) + 'static,
    ) {
        let id = session.request(request, self.ear());
        self.then.insert(
            id,
            Box::new(move |server, told| {
                let answer = match told {
                    Told::Answer(Ok((answer, _))) if answer["error"].is_null() => Some(answer),
                    Told::Answer(
                        Ok(_) | Err(Refusal::NotRunning | Refusal::Error(_) | Refusal::Cancelled),
                    )
                    | Told::Building(_)
                    | Told::Done
                    | Told::Next(_) => None,
                };
                then(server, answer);
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead as _, BufReader, Write as _};
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;
    use std::sync::{mpsc, Arc};

    use serde_json::{json, Value};

    use super::{Heard, Server};
    use crate::dap::wire;
    use crate::mcp::session::{Phase, Session};

    const URI: &str = "file:///p/src/main.rs";

    const TEXT: &str = "// rawr\n\
                        fn main() {\n\
                        \x20   let player = Player { score: 3 };\n\
                        \x20   println!(\"{}\", player.score);\n\
                        }\n";

    /// A server on a thread of its own, its program at `/p` attached to
    /// `session`, with `/p/src/main.rs` open, that writes to the stream
    /// returned: its inbox and its thread, which ends once it heard
    /// `Ended`.
    fn serving(
        session: Arc<Session>,
    ) -> (
        mpsc::Sender<Heard>,
        BufReader<UnixStream>,
        std::thread::JoinHandle<()>,
    ) {
        let (written, read) = UnixStream::pair().unwrap();
        let (heard, hearing) = mpsc::channel();
        let telling = heard.clone();
        let thread = std::thread::spawn(move || {
            let mut server = Server::new(Box::new(written), telling, None);
            server.initialized = true;
            server.refreshes = true;
            server.linked(PathBuf::from("/p"), session);
            server.serve(&hearing);
        });
        let opened = json!({
            "method": "textDocument/didOpen",
            "params": { "textDocument": { "uri": URI, "languageId": "rust", "version": 1, "text": TEXT } },
        });
        heard.send(Heard::Message(opened)).unwrap();
        (heard, BufReader::new(read), thread)
    }

    /// A hover request numbered `id` at `line` and `character`, from 0.
    fn hover(id: u64, line: u64, character: u64) -> Heard {
        Heard::Message(json!({
            "id": id,
            "method": "textDocument/hover",
            "params": {
                "textDocument": { "uri": URI },
                "position": { "line": line, "character": character },
            },
        }))
    }

    /// The next message the server writes whose `key` is `value`, past the
    /// others.
    fn written(told: &mut BufReader<UnixStream>, key: &str, value: &Value) -> Value {
        loop {
            let message = wire::read(told).unwrap().unwrap();
            if message[key] == *value {
                return message;
            }
        }
    }

    /// The next line the session's peer reads, as JSON.
    fn next(lines: &mut impl Iterator<Item = std::io::Result<String>>) -> Value {
        serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap()
    }

    /// The program's `answer` to the request `asked`, from the session.
    fn answer(other: &UnixStream, asked: &Value, answer: Value) {
        let answer = json!({ "jsonrpc": "2.0", "id": asked["id"], "result": answer });
        writeln!(&*other, "{answer}").unwrap();
    }

    /// The session's event `event`.
    fn event(other: &UnixStream, event: Value) {
        let event = json!({ "jsonrpc": "2.0", "method": "event", "params": event });
        writeln!(&*other, "{event}").unwrap();
    }

    /// The program paused at line 4, as event `seq`.
    fn paused(seq: u64) -> Value {
        json!({
            "event": "paused", "seq": seq, "file": "/p/src/main.rs", "line": 4,
            "reason": "breakpoint", "pauses": seq,
        })
    }

    /// At a pause, a binding's value is a hint at the end of the line that
    /// names it, read at the name, and a hover reads the place under the
    /// pointer, over its range.
    #[test]
    fn a_paused_frames_values_are_hints_and_hovers() {
        let (session, other) = Session::running_at(Phase::Running);
        let (heard, mut told, thread) = serving(session);
        let mut lines = BufReader::new(other.try_clone().unwrap()).lines();
        assert_eq!(next(&mut lines)["method"], "hello");
        event(&other, paused(2));
        let bindings = next(&mut lines);
        assert_eq!(bindings["params"], json!({ "frame": 0 }));
        let player = json!({ "name": "player", "value": "Player { score: 3 }" });
        answer(&other, &bindings, json!({ "bindings": [player] }));
        let read = next(&mut lines);
        assert_eq!(
            read["params"],
            json!({
                "op": "hover", "file": "/p/src/main.rs", "line": 4, "column": 20,
                "text": "    println!(\"{}\", player.score);", "frame": 0,
            })
        );
        let player = json!({ "expression": "player", "value": "Player { score: 3 }" });
        answer(&other, &read, player);
        written(&mut told, "method", &json!("workspace/inlayHint/refresh"));
        let range = json!({
            "start": { "line": 0, "character": 0 },
            "end": { "line": 9, "character": 0 },
        });
        let hints = json!({
            "id": 1,
            "method": "textDocument/inlayHint",
            "params": { "textDocument": { "uri": URI }, "range": range },
        });
        heard.send(Heard::Message(hints)).unwrap();
        let hints = written(&mut told, "id", &json!(1));
        assert_eq!(
            hints["result"],
            json!([{
                "position": { "line": 3, "character": 33 },
                "label": "player = Player { score: 3 }",
                "paddingLeft": true,
            }])
        );

        heard.send(hover(2, 3, 26)).unwrap();
        let read = next(&mut lines);
        assert_eq!(read["params"]["column"], 27, "{read}");
        let range = json!({ "line": 4, "start": 20, "end": 32 });
        let score = json!({ "expression": "player.score", "value": "3", "range": range });
        answer(&other, &read, score);
        let hovered = written(&mut told, "id", &json!(2));
        assert_eq!(
            hovered["result"],
            json!({
                "contents": { "kind": "markdown", "value": "```rust\nplayer.score = 3\n```" },
                "range": {
                    "start": { "line": 3, "character": 19 },
                    "end": { "line": 3, "character": 31 },
                },
            })
        );
        heard.send(Heard::Ended).unwrap();
        thread.join().unwrap();
    }

    /// While the program runs a hover answers nothing and asks the program
    /// nothing; an answer that comes after the program went on is dropped
    /// (D1096.3).
    #[test]
    fn a_hover_while_running_or_answered_after_a_resume_shows_nothing() {
        let (session, other) = Session::running_at(Phase::Running);
        let (heard, mut told, thread) = serving(session);
        let mut lines = BufReader::new(other.try_clone().unwrap()).lines();
        assert_eq!(next(&mut lines)["method"], "hello");
        heard.send(hover(1, 3, 26)).unwrap();
        assert_eq!(written(&mut told, "id", &json!(1))["result"], Value::Null);

        event(&other, paused(2));
        let bindings = next(&mut lines);
        assert_eq!(
            bindings["params"],
            json!({ "frame": 0 }),
            "the hover while running asked the program nothing"
        );
        answer(&other, &bindings, json!({ "bindings": [] }));
        written(&mut told, "method", &json!("workspace/inlayHint/refresh"));
        heard.send(hover(2, 3, 26)).unwrap();
        let read = next(&mut lines);
        assert_eq!(read["params"]["op"], "hover", "{read}");
        event(&other, json!({ "event": "resumed", "seq": 3 }));
        let score = json!({ "expression": "player.score", "value": "3" });
        answer(&other, &read, score);
        assert_eq!(written(&mut told, "id", &json!(2))["result"], Value::Null);
        heard.send(Heard::Ended).unwrap();
        thread.join().unwrap();
    }

    /// A hover asked before the run's session greeted the server waits for
    /// the greeting, which says where the program is paused, then reads
    /// there: an editor hovers as soon as it opens a file.
    #[test]
    fn a_hover_before_the_greeting_waits_for_it() {
        let (session, other) = Session::greeting();
        let (out, read) = UnixStream::pair().unwrap();
        let (heard, hearing) = mpsc::channel();
        let opened = json!({
            "method": "textDocument/didOpen",
            "params": { "textDocument": { "uri": URI, "languageId": "rust", "version": 1, "text": TEXT } },
        });
        heard.send(Heard::Message(opened)).unwrap();
        heard.send(hover(1, 3, 26)).unwrap();
        let telling = heard.clone();
        let thread = std::thread::spawn(move || {
            let mut server = Server::new(Box::new(out), telling, None);
            server.initialized = true;
            server.attached(PathBuf::from("/p"), session);
            server.serve(&hearing);
        });
        let mut told = BufReader::new(read);
        let mut lines = BufReader::new(other.try_clone().unwrap()).lines();
        let hello = next(&mut lines);
        assert_eq!(hello["method"], "hello");
        let events = json!([{ "event": "running", "seq": 1 }, paused(2)]);
        answer(
            &other,
            &hello,
            json!({ "events": events, "breakpoints": null }),
        );
        let bindings = next(&mut lines);
        assert_eq!(bindings["params"], json!({ "frame": 0 }));
        answer(&other, &bindings, json!({ "bindings": [] }));
        let read = next(&mut lines);
        assert_eq!(read["params"]["op"], "hover", "{read}");
        let score = json!({ "expression": "player.score", "value": "3" });
        answer(&other, &read, score);
        let hovered = written(&mut told, "id", &json!(1));
        assert_eq!(
            hovered["result"]["contents"]["value"], "```rust\nplayer.score = 3\n```",
            "{hovered}"
        );
        heard.send(Heard::Ended).unwrap();
        thread.join().unwrap();
    }

    /// A hover whose session closes before it answers shows nothing, and
    /// so does the next, with no run to ask.
    #[test]
    fn a_hover_on_a_closed_session_shows_nothing() {
        let (session, other) = Session::running_at(Phase::Running);
        let (heard, mut told, thread) = serving(session);
        let mut lines = BufReader::new(other.try_clone().unwrap()).lines();
        assert_eq!(next(&mut lines)["method"], "hello");
        event(&other, paused(2));
        let bindings = next(&mut lines);
        answer(&other, &bindings, json!({ "bindings": [] }));
        written(&mut told, "method", &json!("workspace/inlayHint/refresh"));
        heard.send(hover(1, 3, 26)).unwrap();
        assert_eq!(next(&mut lines)["params"]["op"], "hover");
        other.shutdown(std::net::Shutdown::Both).unwrap();
        assert_eq!(written(&mut told, "id", &json!(1))["result"], Value::Null);
        heard.send(hover(2, 3, 26)).unwrap();
        assert_eq!(written(&mut told, "id", &json!(2))["result"], Value::Null);
        heard.send(Heard::Ended).unwrap();
        thread.join().unwrap();
    }
}
