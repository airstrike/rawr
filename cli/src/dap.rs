//! `rawr dap`: a Debug Adapter Protocol server on stdio for editors, one
//! shell of a run's debugger session (D1053.2), as `rawr mcp` is.
//!
//! It translates and holds nothing of its own beyond the protocol's
//! handles: `launch` starts a `rawr run` as `rawr mcp`'s `start` does, and
//! `attach` reaches a run started elsewhere, through the same
//! [`crate::mcp::session`]. Breakpoints, steps, a pause, values and `eval`
//! are the program's inspector's ([`rawr::inspect`]); the session's events
//! become `stopped`, `continued`, `output` and `terminated`, and the
//! breakpoints another shell sets or clears, and those a save moves,
//! become `breakpoint` events. A save still reloads the program, and the
//! adapter says so as output.
//!
//! One thread owns the adapter: the client's references, the breakpoints
//! it knows and the turn its breakpoint requests take. It hears the
//! client's requests and the session's answers on one inbox, and blocks
//! on neither: a request that needs the program asks the session and goes
//! on once the answer reaches the inbox, so a breakpoint set while the
//! program builds holds no other request.
//!
//! The protocol counts one thread: the one that runs the program's code,
//! which the inspector pauses. Frames are the inspector's, innermost first,
//! each named by its Rust path, and a value's children are its parts as
//! the inspector splits its `{:?}` text. A value the client sets is
//! assigned through the same `eval`, at the value's place in its frame.

mod assign;
mod breakpoints;
mod control;
mod follow;
mod handles;
mod lifecycle;
mod place;
mod serve;
mod values;
mod wire;

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{mpsc, Arc};

use serde_json::{json, Value};

use crate::mcp::session::{Ear, Refusal, Seen, Session, Told, Wait};

pub use serve::{serve, serve_on};

/// The thread the protocol names: the one that runs the program's code.
const THREAD: u64 = 1;

/// Why a request that needs the program has no answer before `launch` or
/// `attach`.
const NO_PROGRAM: &str = "no program yet: launch or attach first";

/// What the adapter's thread hears.
enum Heard {
    /// A request from the client.
    Request(Value),
    /// The client's input ended.
    Ended,
    /// The session told the adapter's request or wait `id`.
    Session { id: u64, told: Told },
}

/// What the adapter does once the session told one of its requests or
/// waits.
type Then = Box<dyn FnOnce(&mut Adapter, Told)>;

/// One client, and the program it debugs once it launched or attached.
struct Adapter {
    /// Where relative paths start.
    root: PathBuf,
    out: wire::Writer,
    /// The adapter's inbox, where the session answers it.
    heard: mpsc::Sender<Heard>,
    /// The program's session, once the client launched or attached.
    session: Option<Arc<Session>>,
    /// A `launch` or `attach` waits for a run's greeting.
    beginning: bool,
    handles: handles::Handles,
    /// The line the client counts from: 1, or 0.
    first_line: u64,
    /// The functions the client set breakpoints on, which its next
    /// `setFunctionBreakpoints` replaces.
    functions: Vec<String>,
    /// The breakpoints the client knows, by their ids.
    known: breakpoints::Known,
    /// One breakpoint request changes the program's breakpoints at a time:
    /// whether one has the turn, and those waiting for it.
    breaking: bool,
    turn: VecDeque<Value>,
    /// The client heard `stopped` and no `continued` since.
    stopped: bool,
    /// The program the adapter launched waits at its start for the
    /// client's `configurationDone`, which releases it (#2274).
    held: bool,
    /// How each request or wait on the session goes on, by its number.
    then: HashMap<u64, Then>,
    /// Where the session's events, output and breakpoints were told to.
    following: follow::Following,
    /// The client is gone and the adapter ended its program.
    over: bool,
}

impl Adapter {
    fn new(root: PathBuf, out: wire::Writer, heard: mpsc::Sender<Heard>) -> Self {
        Self {
            root,
            out,
            heard,
            session: None,
            beginning: false,
            handles: handles::Handles::default(),
            first_line: 1,
            functions: Vec::new(),
            known: breakpoints::Known::default(),
            breaking: false,
            turn: VecDeque::new(),
            stopped: false,
            held: false,
            then: HashMap::new(),
            following: follow::Following::default(),
            over: false,
        }
    }

    /// An ear on the adapter's inbox.
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

    /// Ask the program `request`, then go on with `then` and its answer,
    /// or why it has none: refused by the session or by the program.
    fn ask(
        &mut self,
        request: Value,
        then: impl FnOnce(&mut Self, Result<Value, String>) + 'static,
    ) {
        self.ask_seen(request, |adapter, answer| {
            then(adapter, answer.map(|(answer, _)| answer));
        });
    }

    /// [`Self::ask`], with how far the session's events had come when the
    /// answer came, for an answer that says where the program is.
    fn ask_seen(
        &mut self,
        request: Value,
        then: impl FnOnce(&mut Self, Result<(Value, Seen), String>) + 'static,
    ) {
        let Some(session) = &self.session else {
            return then(self, Err(NO_PROGRAM.to_owned()));
        };
        let id = session.request(request, self.ear());
        self.then.insert(
            id,
            Box::new(move |adapter, told| {
                let answer = match told {
                    Told::Answer(answer) => adapter.answered(answer),
                    told => Err(format!("the session told no answer: {told:?}")),
                };
                then(adapter, answer);
            }),
        );
    }

    /// A session's answer as the client reads it.
    fn answered(&self, answer: Result<(Value, Seen), Refusal>) -> Result<(Value, Seen), String> {
        let (answer, seen) = answer.map_err(|refusal| match refusal {
            Refusal::NotRunning => format!(
                "the program isn't running (state: {})",
                self.session
                    .as_ref()
                    .map_or("not started", |session| session.phase().name())
            ),
            Refusal::Error(error) => error,
            Refusal::Cancelled => "cancelled".to_owned(),
        })?;
        match answer["error"].as_str() {
            Some("not paused") => Err("the program isn't paused".to_owned()),
            Some(error) => Err(error.to_owned()),
            None => Ok((answer, seen)),
        }
    }

    /// A line the client gave, as the program counts lines: from 1.
    const fn line_in(&self, line: u64) -> u64 {
        line + 1 - self.first_line
    }

    /// A line of the program, as the client counts lines.
    const fn line_out(&self, line: u64) -> u64 {
        (line + self.first_line).saturating_sub(1)
    }

    /// Tell the client `text` as output in `category`: `console` for the
    /// adapter's own, `stdout` and `stderr` for the program's.
    fn output(&self, category: &str, text: &str) {
        self.out
            .event("output", json!({ "category": category, "output": text }));
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead as _, BufReader, Write as _};
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;
    use std::sync::{mpsc, Arc};

    use serde_json::{json, Value};

    use super::{wire, Adapter, Heard};
    use crate::mcp::session::{Phase, Session};

    /// An adapter on a thread of its own, debugging `session`, `held` at
    /// its start or not, that writes to the stream returned: its inbox and
    /// its thread, which ends once it heard `Ended` and ended its program.
    fn serving(
        session: Arc<Session>,
        held: bool,
    ) -> (
        mpsc::Sender<Heard>,
        BufReader<UnixStream>,
        std::thread::JoinHandle<()>,
    ) {
        let (written, read) = UnixStream::pair().unwrap();
        let (heard, hearing) = mpsc::channel();
        let telling = heard.clone();
        let thread = std::thread::spawn(move || {
            let out = wire::Writer::new(written).unwrap();
            let mut adapter = Adapter::new(PathBuf::from("/p"), out, telling);
            adapter.begin(session);
            adapter.held = held;
            adapter.serve(&hearing);
        });
        (heard, BufReader::new(read), thread)
    }

    /// A request numbered `seq`.
    fn request(seq: u64, command: &str, arguments: Value) -> Heard {
        Heard::Request(json!({
            "type": "request",
            "seq": seq,
            "command": command,
            "arguments": arguments,
        }))
    }

    /// The response to the request numbered `seq`, past the events before
    /// it.
    fn response(heard: &mut BufReader<UnixStream>, seq: u64) -> Value {
        loop {
            let message = wire::read(heard).unwrap().unwrap();
            if message["type"] == "response" && message["request_seq"] == seq {
                return message;
            }
        }
    }

    /// The next line the session's peer reads, as JSON.
    fn next(lines: &mut impl Iterator<Item = std::io::Result<String>>) -> Value {
        serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap()
    }

    /// `configurationDone` releases the program the adapter launched, held
    /// at its start, once; a second asks the program nothing (#2274).
    #[test]
    fn configuration_done_releases_a_launched_program_once() {
        let (session, other) = Session::running_at(Phase::Running);
        let (heard, mut told, thread) = serving(session, true);
        let mut lines = BufReader::new(other.try_clone().unwrap()).lines();
        assert_eq!(next(&mut lines)["method"], "hello");
        heard
            .send(request(1, "configurationDone", json!({})))
            .unwrap();
        let asked = next(&mut lines);
        assert_eq!(asked["params"], json!({ "op": "release" }));
        let answer = json!({ "jsonrpc": "2.0", "id": asked["id"], "result": { "released": true } });
        writeln!(&other, "{answer}").unwrap();
        assert_eq!(response(&mut told, 1)["success"], true);
        // Nothing is held any more: the next request the program hears is
        // the one after the second `configurationDone`.
        heard
            .send(request(2, "configurationDone", json!({})))
            .unwrap();
        assert_eq!(response(&mut told, 2)["success"], true);
        heard.send(request(3, "pause", json!({}))).unwrap();
        assert_eq!(next(&mut lines)["params"], json!({ "op": "pause" }));
        heard.send(Heard::Ended).unwrap();
        thread.join().unwrap();
    }

    /// A breakpoint another shell sets reaches the client as a `breakpoint`
    /// event, `new` with its line as the client counts lines, and its
    /// clear as `removed` by the same id (#2278).
    #[test]
    fn another_shells_breakpoint_is_told_as_new_then_removed() {
        let (written, read) = UnixStream::pair().unwrap();
        let (heard, _hearing) = mpsc::channel();
        let mut adapter = Adapter::new(
            PathBuf::from("/p"),
            wire::Writer::new(written).unwrap(),
            heard,
        );
        adapter.first_line = 0;
        let mut heard = BufReader::new(read);
        let spec = json!({ "file": "/p/src/main.rs", "line": 7, "text": "tick();" });
        adapter.follow_breakpoints(&json!({ "revision": 1, "breakpoints": [spec] }));
        let new = wire::read(&mut heard).unwrap().unwrap();
        assert_eq!(new["event"], "breakpoint", "{new}");
        assert_eq!(new["body"]["reason"], "new", "{new}");
        assert_eq!(new["body"]["breakpoint"]["line"], 6, "{new}");
        assert_eq!(
            new["body"]["breakpoint"]["source"]["path"],
            "/p/src/main.rs"
        );
        adapter.follow_breakpoints(&json!({ "revision": 2, "breakpoints": [] }));
        let removed = wire::read(&mut heard).unwrap().unwrap();
        assert_eq!(removed["body"]["reason"], "removed", "{removed}");
        assert_eq!(
            removed["body"]["breakpoint"]["id"],
            new["body"]["breakpoint"]["id"]
        );
    }

    /// A save that moves a breakpoint to another line, which the session's
    /// `breakpoints` event lists under the same inspector id, reaches the
    /// client as `changed` with the id it knows, so the client keeps its
    /// own breakpoint (#2299).
    #[test]
    fn a_breakpoint_a_save_moved_is_told_as_changed() {
        let (session, other) = Session::running_at(Phase::Running);
        let (heard, mut told, thread) = serving(session, false);
        let mut breakpoint = |revision: u64, line: u64| {
            let listed =
                json!({ "id": 3, "file": "/p/src/main.rs", "line": line, "text": "tick();" });
            let event =
                json!({ "event": "breakpoints", "revision": revision, "breakpoints": [listed] });
            let event = json!({ "jsonrpc": "2.0", "method": "event", "params": event });
            writeln!(&other, "{event}").unwrap();
            // The next `breakpoint` event, past the run's own output.
            loop {
                let message = wire::read(&mut told).unwrap().unwrap();
                if message["event"] == "breakpoint" {
                    return message["body"].clone();
                }
            }
        };
        let new = breakpoint(1, 7);
        assert_eq!(new["reason"], "new", "{new}");
        let changed = breakpoint(2, 9);
        assert_eq!(changed["reason"], "changed", "{changed}");
        assert_eq!(changed["breakpoint"]["id"], new["breakpoint"]["id"]);
        assert_eq!(changed["breakpoint"]["line"], 9, "{changed}");
        assert_eq!(changed["breakpoint"]["source"]["path"], "/p/src/main.rs");
        other.shutdown(std::net::Shutdown::Both).unwrap();
        heard.send(Heard::Ended).unwrap();
        thread.join().unwrap();
    }

    /// Rawr's own lines, the stderr of the run the adapter started, reach
    /// the client as `console`, and the program's stderr, which the
    /// session tells, as `stderr` (#2367).
    #[test]
    fn rawr_lines_are_console_and_the_programs_stderr_is_stderr() {
        let (session, other) = Session::running_at(Phase::Running);
        session.read_rawr(b"rawr > building runner  55%\n");
        let output = json!({
            "jsonrpc": "2.0",
            "method": "output",
            "params": { "stream": "stderr", "text": "the program's own\n" },
        });
        writeln!(&other, "{output}").unwrap();
        let (heard, mut told, thread) = serving(session, false);
        let mut outputs = Vec::new();
        while outputs.len() < 2 {
            let message = wire::read(&mut told).unwrap().unwrap();
            if message["event"] == "output" {
                outputs.push((
                    message["body"]["category"].as_str().unwrap().to_owned(),
                    message["body"]["output"].as_str().unwrap().to_owned(),
                ));
            }
        }
        outputs.sort();
        assert_eq!(
            outputs,
            [
                (
                    "console".to_owned(),
                    "rawr > building runner  55%\n".to_owned()
                ),
                ("stderr".to_owned(), "the program's own\n".to_owned()),
            ]
        );
        other.shutdown(std::net::Shutdown::Both).unwrap();
        heard.send(Heard::Ended).unwrap();
        thread.join().unwrap();
    }

    /// A breakpoint request waiting on a session whose connection closes
    /// is answered as failed, and the request that waited for its turn is
    /// answered after it: no request hangs on a peer that went away.
    #[test]
    fn a_peer_that_closes_mid_request_answers_it_and_the_next_in_turn() {
        let (session, other) = Session::running_at(Phase::Running);
        let (heard, mut told, thread) = serving(session, false);
        let mut lines = BufReader::new(other.try_clone().unwrap()).lines();
        assert_eq!(next(&mut lines)["method"], "hello");
        let source =
            json!({ "source": { "path": "/p/src/main.rs" }, "breakpoints": [{ "line": 3 }] });
        heard.send(request(1, "setBreakpoints", source)).unwrap();
        let functions = json!({ "breakpoints": [{ "name": "tick" }] });
        heard
            .send(request(2, "setFunctionBreakpoints", functions))
            .unwrap();
        assert_eq!(next(&mut lines)["params"]["op"], "clear");
        other.shutdown(std::net::Shutdown::Both).unwrap();
        let lines_set = response(&mut told, 1);
        assert_eq!(lines_set["success"], false, "{lines_set}");
        let functions_set = response(&mut told, 2);
        assert_eq!(functions_set["success"], true, "{functions_set}");
        assert_eq!(
            functions_set["body"]["breakpoints"][0]["verified"], false,
            "{functions_set}"
        );
        heard.send(Heard::Ended).unwrap();
        thread.join().unwrap();
    }

    /// A hover's `evaluate` asks the inspector's hover read, which refuses
    /// a call, and never its `eval`; the console's still asks `eval`.
    #[test]
    fn a_hover_reads_and_never_evaluates_a_call() {
        let (session, other) = Session::running_at(Phase::Running);
        let (heard, mut told, thread) = serving(session, false);
        let mut lines = BufReader::new(other.try_clone().unwrap()).lines();
        assert_eq!(next(&mut lines)["method"], "hello");
        let paused = json!({
            "event": "paused", "seq": 2, "file": "/p/src/main.rs", "line": 4,
            "reason": "breakpoint", "pauses": 1,
        });
        let event = json!({ "jsonrpc": "2.0", "method": "event", "params": paused });
        writeln!(&other, "{event}").unwrap();
        while wire::read(&mut told).unwrap().unwrap()["event"] != "stopped" {}
        let mut evaluate = |seq: u64, expression: &str, context: &str, result: Value| {
            let arguments = json!({ "expression": expression, "context": context, "frameId": 1 });
            heard.send(request(seq, "evaluate", arguments)).unwrap();
            let asked = next(&mut lines);
            let answer = json!({ "jsonrpc": "2.0", "id": asked["id"], "result": result });
            writeln!(&other, "{answer}").unwrap();
            (asked["params"].clone(), response(&mut told, seq))
        };
        let refusal = "a hover doesn't run calls: evaluate it in the console";
        let (asked, answered) = evaluate(
            1,
            "player.reset()",
            "hover",
            json!({ "expression": "player.reset()", "refused": refusal }),
        );
        assert_eq!(
            asked,
            json!({ "op": "hover", "code": "player.reset()", "frame": 0, "children": true })
        );
        assert_eq!(answered["success"], false, "{answered}");
        assert_eq!(answered["message"], refusal, "{answered}");
        let (asked, answered) = evaluate(
            2,
            "player.score",
            "hover",
            json!({ "expression": "player.score", "value": "3" }),
        );
        assert_eq!(asked["op"], "hover");
        assert_eq!(answered["body"]["result"], "3", "{answered}");
        let (asked, answered) = evaluate(3, "player.reset()", "repl", json!({ "value": "0" }));
        assert_eq!(asked["op"], "eval", "the console runs what it is given");
        assert_eq!(answered["body"]["result"], "0", "{answered}");
        other.shutdown(std::net::Shutdown::Both).unwrap();
        heard.send(Heard::Ended).unwrap();
        thread.join().unwrap();
    }

    /// A `disconnect` while a request waits on the program is answered,
    /// and the adapter ends, without the program's answer.
    #[test]
    fn a_disconnect_mid_request_ends_the_adapter() {
        let (session, other) = Session::running_at(Phase::Running);
        let (heard, mut told, thread) = serving(session, false);
        let mut lines = BufReader::new(other.try_clone().unwrap()).lines();
        assert_eq!(next(&mut lines)["method"], "hello");
        heard.send(request(1, "pause", json!({}))).unwrap();
        assert_eq!(next(&mut lines)["params"]["op"], "pause");
        heard.send(request(2, "disconnect", json!({}))).unwrap();
        assert_eq!(response(&mut told, 2)["success"], true);
        thread.join().unwrap();
    }
}
