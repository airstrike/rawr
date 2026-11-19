//! The session an owner hosts: the program's state as the owner's events
//! tell it, the connections that hear them, and the requests it forwards
//! to the program's inspector.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard, PoisonError};

use serde_json::{json, Value};

use super::handover;
use super::place::Place;
use super::state::{Phase, State};

/// The protocol's version, which `hello` says.
const PROTOCOL: u64 = 1;
/// Output kept for shells that attach later, oldest first out.
const OUTPUT: usize = 256 * 1024;

/// The error a `program` request ends with when no program will answer it.
pub const NOT_RUNNING: i64 = -32001;
/// The error a cancelled request ends with.
pub const CANCELLED: i64 = -32800;

/// The session this process hosts, while its owner holds it.
static OPEN: Mutex<Option<Arc<Shared>>> = Mutex::new(None);

/// A session an owner hosts, which leaves its place when dropped.
pub struct Host(Arc<Shared>);

struct Shared {
    root: PathBuf,
    place: Place,
    /// A shell started the run: it tells the run's output.
    handed: bool,
    inner: Mutex<Inner>,
    changed: Condvar,
    /// Held by the control request that reaches the program now.
    control: Mutex<()>,
}

#[derive(Default)]
struct Inner {
    state: State,
    output: VecDeque<Value>,
    output_bytes: usize,
    listeners: Vec<Listener>,
    inspector: Option<Served>,
    debugger: Debugger,
}

/// A connection that said `hello`, and hears what happens from then on.
struct Listener {
    connection: u64,
    send: mpsc::Sender<Arc<str>>,
}

/// The program's inspector, listening on `socket` while its connection
/// `connection` stays open.
struct Served {
    connection: u64,
    socket: PathBuf,
}

/// Whether the programs the owner starts register a debugger.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Debugger {
    /// No shell asked for one.
    #[default]
    Off,
    /// A shell asked for one: the owner restarts the program with it.
    Asked,
    /// Each program the owner starts has one.
    On,
}

/// Why a `program` request has no answer.
enum Refusal {
    NotRunning(Phase),
    Cancelled,
}

/// How one exchange with the inspector went.
enum Asked {
    Answered(Value),
    /// Nothing listens on the socket: the program that served it is gone.
    Refused,
    /// The connection ended before an answer: the program ended.
    Ended,
    Cancelled,
}

/// A `program` request's cancellation, which shuts its exchange with the
/// inspector.
#[derive(Default)]
struct Cancel {
    cancelled: AtomicBool,
    stream: Mutex<Option<UnixStream>>,
}

type Requests = Arc<Mutex<HashMap<String, Arc<Cancel>>>>;

/// Host the session of the program at `root` until the host drops: on the
/// listener a shell handed this run, or on a socket at the root's place.
/// `None` when another run of the program hosts it, which shells reach
/// instead, or, with a line on stderr, when the socket can't be made; the
/// program runs as it would without one.
pub fn open(root: &Path) -> Option<Host> {
    let opened = Place::new(root).and_then(|place| {
        let listener = match handover::take() {
            Some(listener) => listener,
            None => {
                place.make()?;
                match listen(&place.socket())? {
                    Some(listener) => listener,
                    None => return Ok(None),
                }
            }
        };
        Host::serve(root, place, listener, handover::handed()).map(Some)
    });
    match opened {
        Ok(Some(host)) => {
            *open_session() = Some(Arc::clone(&host.0));
            Some(host)
        }
        Ok(None) => None,
        Err(error) => {
            eprintln!("rawr: shells can't attach to this run: {error}");
            None
        }
    }
}

/// Record `event` in the session this process hosts, and tell it to every
/// connection.
pub fn record(event: &Value) {
    if let Some(shared) = current() {
        shared.record(event.clone());
    }
}

/// Ready `command`, which starts the program, for the session: never the
/// listener this run was handed, and the debugger's sockets once a shell
/// asked for one.
pub fn prepare(command: &mut Command) {
    command.env_remove(handover::LISTENER);
    let Some(shared) = current() else {
        return;
    };
    let mut inner = shared.inner();
    if inner.debugger == Debugger::Off {
        return;
    }
    // The program this starts has the debugger the shell asked for.
    inner.debugger = Debugger::On;
    command
        .env(crate::events::INSPECT, shared.place.inspector())
        .env(crate::events::DEBUG_EVENTS, shared.place.socket());
}

/// Whether a shell asked for the program's debugger while it ran without
/// one: the owner then restarts it, and [`prepare`] gives the new program
/// one.
pub fn take_restart() -> bool {
    let Some(shared) = current() else {
        return false;
    };
    let mut inner = shared.inner();
    let asked = inner.debugger == Debugger::Asked;
    if asked {
        inner.debugger = Debugger::On;
    }
    asked
}

fn open_session() -> MutexGuard<'static, Option<Arc<Shared>>> {
    OPEN.lock().unwrap_or_else(PoisonError::into_inner)
}

fn current() -> Option<Arc<Shared>> {
    open_session().clone()
}

/// A listener on `socket`, in place of a stale one: `None` when a run
/// hosts a session there.
fn listen(socket: &Path) -> std::io::Result<Option<UnixListener>> {
    match UnixListener::bind(socket) {
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            if UnixStream::connect(socket).is_ok() {
                return Ok(None);
            }
            std::fs::remove_file(socket)?;
            UnixListener::bind(socket).map(Some)
        }
        bound => bound.map(Some),
    }
}

impl Host {
    /// Serve the session of the program at `root` on `listener`, at
    /// `place`; `handed` when a shell started the run.
    fn serve(
        root: &Path,
        place: Place,
        listener: UnixListener,
        handed: bool,
    ) -> std::io::Result<Self> {
        place.clear();
        let shared = Arc::new(Shared {
            root: root.to_path_buf(),
            place,
            handed,
            inner: Mutex::new(Inner {
                debugger: if handed { Debugger::On } else { Debugger::Off },
                ..Inner::default()
            }),
            changed: Condvar::new(),
            control: Mutex::new(()),
        });
        let accepting = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("rawr-session-accept".into())
            .spawn(move || accepting.accept(&listener))?;
        Ok(Self(shared))
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let mut open = open_session();
        if open
            .as_ref()
            .is_some_and(|shared| Arc::ptr_eq(shared, &self.0))
        {
            *open = None;
        }
        drop(open);
        self.0.place.remove();
    }
}

impl Shared {
    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn accept(self: &Arc<Self>, listener: &UnixListener) {
        let mut next = 0;
        for stream in listener.incoming() {
            let Ok(stream) = stream else {
                continue;
            };
            next += 1;
            let connection = next;
            let shared = Arc::clone(self);
            let _ = std::thread::Builder::new()
                .name("rawr-session-read".into())
                .spawn(move || shared.converse(connection, &stream));
        }
    }

    /// Read `stream`'s messages until it closes, then cancel what it asked.
    fn converse(self: &Arc<Self>, connection: u64, stream: &UnixStream) {
        let Ok(writing) = stream.try_clone() else {
            return;
        };
        let (send, lines) = mpsc::channel::<Arc<str>>();
        let spawned = std::thread::Builder::new()
            .name("rawr-session-write".into())
            .spawn(move || write(writing, &lines));
        if spawned.is_err() {
            return;
        }
        let requests = Requests::default();
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else {
                break;
            };
            if let Ok(message) = serde_json::from_str::<Value>(&line) {
                self.take(connection, message, &send, &requests);
            }
        }
        let asked: Vec<Arc<Cancel>> = requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain()
            .map(|(_, cancel)| cancel)
            .collect();
        for cancel in asked {
            cancel.cancel(self);
        }
        self.left(connection);
    }

    /// One message from connection `connection`.
    fn take(
        self: &Arc<Self>,
        connection: u64,
        mut message: Value,
        send: &mpsc::Sender<Arc<str>>,
        requests: &Requests,
    ) {
        let params = message["params"].take();
        let id = message.get("id").cloned();
        match (message["method"].as_str(), id) {
            (Some("hello"), Some(id)) => self.greet(connection, &id, send),
            (Some("program"), Some(id)) => {
                let key = id.to_string();
                let cancel = Arc::new(Cancel::default());
                requests
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(key.clone(), Arc::clone(&cancel));
                let shared = Arc::clone(self);
                let send = send.clone();
                let requests = Arc::clone(requests);
                let _ = std::thread::Builder::new()
                    .name("rawr-session-program".into())
                    .spawn(move || {
                        let reply = match shared.forward(&params, &cancel) {
                            Ok(answer) => json!({ "jsonrpc": "2.0", "id": id, "result": answer }),
                            Err(Refusal::NotRunning(phase)) => json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "error": {
                                    "code": NOT_RUNNING,
                                    "message": "the program isn't running",
                                    "data": { "state": phase.name() },
                                },
                            }),
                            Err(Refusal::Cancelled) => json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "error": { "code": CANCELLED, "message": "cancelled" },
                            }),
                        };
                        requests
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .remove(&key);
                        let _ = send.send(line(&reply));
                    });
            }
            (Some("cancel"), None) => {
                let cancel = requests
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get(&params["id"].to_string())
                    .cloned();
                if let Some(cancel) = cancel {
                    cancel.cancel(self);
                }
            }
            (Some("serve"), None) => {
                if let Some(socket) = params["socket"].as_str() {
                    self.inner().inspector = Some(Served {
                        connection,
                        socket: PathBuf::from(socket),
                    });
                    self.changed.notify_all();
                }
            }
            (Some("emit"), None) if params.is_object() => self.record(params),
            (Some("output"), None) => self.output(connection, params),
            (Some(method), Some(id)) => {
                let _ = send.send(line(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": -32601, "message": format!("unknown method `{method}`") },
                })));
            }
            _ => {}
        }
    }

    /// Answer `hello`: the events so far and the output kept, and from now
    /// on every event and piece of output.
    fn greet(&self, connection: u64, id: &Value, send: &mpsc::Sender<Arc<str>>) {
        let mut inner = self.inner();
        let events: Vec<Value> = inner
            .state
            .events()
            .iter()
            .map(|(seq, event)| numbered(*seq, event))
            .collect();
        let mut capabilities = vec![
            "events",
            "inspect",
            "eval",
            "break",
            "clear",
            "step",
            "continue",
            "pause",
            "input",
            "screenshot",
            "menu",
        ];
        if self.handed {
            capabilities.push("output");
        }
        let result = json!({
            "protocol": PROTOCOL,
            "capabilities": capabilities,
            "root": self.root,
            "pid": std::process::id(),
            "events": events,
            "output": inner.output,
        });
        let _ = send.send(line(
            &json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        ));
        inner.listeners.push(Listener {
            connection,
            send: send.clone(),
        });
    }

    fn record(&self, event: Value) {
        let mut inner = self.inner();
        let seq = inner.state.next_number();
        let told = line(&json!({
            "jsonrpc": "2.0",
            "method": "event",
            "params": numbered(seq, &event),
        }));
        inner.state.record(seq, event);
        inner
            .listeners
            .retain(|listener| listener.send.send(Arc::clone(&told)).is_ok());
        drop(inner);
        self.changed.notify_all();
    }

    /// Keep output the shell on `from` read, and tell the others.
    fn output(&self, from: u64, piece: Value) {
        let Some(bytes) = piece["text"].as_str().map(str::len) else {
            return;
        };
        let told = line(&json!({ "jsonrpc": "2.0", "method": "output", "params": piece }));
        let mut inner = self.inner();
        inner.output_bytes += bytes;
        inner.output.push_back(piece);
        while inner.output_bytes > OUTPUT {
            let Some(dropped) = inner.output.pop_front() else {
                break;
            };
            inner.output_bytes -= dropped["text"].as_str().map_or(0, str::len);
        }
        for listener in &inner.listeners {
            if listener.connection != from {
                let _ = listener.send.send(Arc::clone(&told));
            }
        }
    }

    /// Connection `connection` closed: it hears nothing more, and an
    /// inspector it served is gone.
    fn left(&self, connection: u64) {
        let mut inner = self.inner();
        inner
            .listeners
            .retain(|listener| listener.connection != connection);
        if inner
            .inspector
            .as_ref()
            .is_some_and(|served| served.connection == connection)
        {
            inner.inspector = None;
        }
        drop(inner);
        self.changed.notify_all();
    }

    /// The answer to `request` from the program's inspector. A program
    /// that ends while it holds the request leaves it to the next one.
    fn forward(&self, request: &Value, cancel: &Cancel) -> Result<Value, Refusal> {
        let control = matches!(
            request["op"].as_str(),
            Some("break" | "clear" | "step" | "continue" | "eval" | "input")
        );
        let _turn = control.then(|| self.control.lock().unwrap_or_else(PoisonError::into_inner));
        let mut gone = None;
        loop {
            let (connection, socket) = self.reachable(cancel, gone)?;
            match ask(&socket, request, cancel) {
                Asked::Answered(answer) => return Ok(answer),
                Asked::Refused | Asked::Ended => gone = Some(connection),
                Asked::Cancelled => return Err(Refusal::Cancelled),
            }
        }
    }

    /// The inspector a request goes to once the program serves one: not
    /// while it starts or restarts, and not the one on connection `gone`.
    /// A program without a debugger is asked for one.
    fn reachable(&self, cancel: &Cancel, gone: Option<u64>) -> Result<(u64, PathBuf), Refusal> {
        let mut inner = self.inner();
        loop {
            if cancel.cancelled.load(Ordering::Acquire) {
                return Err(Refusal::Cancelled);
            }
            if inner.debugger == Debugger::Off {
                inner.debugger = Debugger::Asked;
            }
            let phase = inner.state.phase();
            if let Some(served) = &inner.inspector
                && !phase.is_pending()
                && gone != Some(served.connection)
            {
                return Ok((served.connection, served.socket.clone()));
            }
            if !phase.is_live() && !phase.is_pending() {
                return Err(Refusal::NotRunning(phase.clone()));
            }
            inner = self
                .changed
                .wait(inner)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

impl Cancel {
    /// Hold `stream` for a cancel to shut: `false` when cancelled already.
    fn hold(&self, stream: &UnixStream) -> bool {
        let mut held = self.stream.lock().unwrap_or_else(PoisonError::into_inner);
        if self.cancelled.load(Ordering::Acquire) {
            return false;
        }
        *held = stream.try_clone().ok();
        true
    }

    fn cancel(&self, shared: &Shared) {
        self.cancelled.store(true, Ordering::Release);
        let held = self
            .stream
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(stream) = held {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        // A wait in `reachable` looks at the flag under this lock.
        drop(shared.inner());
        shared.changed.notify_all();
    }
}

/// One request and its answer on the inspector at `socket`: the program
/// answers every request it reads, so the end before an answer is the end
/// of the process that read it, as a restart retires it (#1964).
fn ask(socket: &Path, request: &Value, cancel: &Cancel) -> Asked {
    let Ok(stream) = UnixStream::connect(socket) else {
        return Asked::Refused;
    };
    if !cancel.hold(&stream) {
        return Asked::Cancelled;
    }
    let ended = || {
        if cancel.cancelled.load(Ordering::Acquire) {
            Asked::Cancelled
        } else {
            Asked::Ended
        }
    };
    if writeln!(&stream, "{request}").is_err() {
        return ended();
    }
    let mut answer = String::new();
    match BufReader::new(&stream).read_line(&mut answer) {
        Ok(_) if answer.ends_with('\n') => {
            serde_json::from_str(&answer).map_or_else(|_| ended(), Asked::Answered)
        }
        _ => ended(),
    }
}

/// Write each line to the connection until it goes away.
fn write(mut stream: UnixStream, lines: &mpsc::Receiver<Arc<str>>) {
    for line in lines {
        if stream.write_all(line.as_bytes()).is_err() {
            return;
        }
    }
}

/// `event` with its number.
fn numbered(seq: u64, event: &Value) -> Value {
    let mut event = event.clone();
    event["seq"] = json!(seq);
    event
}

fn line(message: &Value) -> Arc<str> {
    Arc::from(format!("{message}\n"))
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead as _, BufReader, Write as _};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::Path;
    use std::sync::mpsc;

    use serde_json::{json, Value};

    use super::{Host, Place, CANCELLED};

    /// A connection to a session, reading its messages on a thread.
    struct Shell {
        stream: UnixStream,
        heard: mpsc::Receiver<Value>,
    }

    impl Shell {
        fn connect(socket: &Path) -> Self {
            let stream = UnixStream::connect(socket).unwrap();
            let reading = stream.try_clone().unwrap();
            let (send, heard) = mpsc::channel();
            std::thread::spawn(move || {
                for line in BufReader::new(reading).lines() {
                    let Ok(line) = line else { break };
                    if send.send(serde_json::from_str(&line).unwrap()).is_err() {
                        break;
                    }
                }
            });
            Self { stream, heard }
        }

        fn send(&self, message: &Value) {
            writeln!(&self.stream, "{message}").unwrap();
        }

        fn next(&self) -> Value {
            self.heard.recv().expect("the session answers")
        }

        /// The events from `hello`'s answer.
        fn hello(&self) -> Vec<Value> {
            self.send(&json!({ "jsonrpc": "2.0", "id": 0, "method": "hello" }));
            let answer = self.next();
            assert_eq!(answer["id"], 0, "{answer}");
            answer["result"]["events"].as_array().unwrap().clone()
        }
    }

    fn host(name: &str, handed: bool) -> (Host, Place) {
        let root = format!("/rawr-session-test/{name}/{}", std::process::id());
        let place = Place::new(Path::new(&root)).unwrap();
        place.make().unwrap();
        let _ = std::fs::remove_file(place.socket());
        let listener = UnixListener::bind(place.socket()).unwrap();
        let host = Host::serve(Path::new(&root), place.clone(), listener, handed).unwrap();
        (host, place)
    }

    /// The program's inspector says where it serves on a connection of its
    /// own, held open while it does.
    fn serve(place: &Place) -> UnixStream {
        let told = UnixStream::connect(place.socket()).unwrap();
        let serve = json!({ "jsonrpc": "2.0", "method": "serve", "params": { "socket": place.inspector() } });
        writeln!(&told, "{serve}").unwrap();
        told
    }

    /// A program that ends while it holds a request, as a restart retires
    /// it, leaves the request to the program that serves next (#1964).
    #[test]
    fn a_request_the_ending_program_held_goes_to_the_next_one() {
        let (host, place) = host("retire", true);
        host.0.record(json!({ "event": "running" }));
        let inspector = UnixListener::bind(place.inspector()).unwrap();
        let ending = serve(&place);
        let shell = Shell::connect(&place.socket());
        shell.hello();
        let request = json!({ "op": "screenshot" });
        shell.send(&json!({ "jsonrpc": "2.0", "id": 1, "method": "program", "params": request }));
        // The retiring program reads the request and ends.
        let (held, _) = inspector.accept().unwrap();
        let mut line = String::new();
        BufReader::new(&held).read_line(&mut line).unwrap();
        drop(held);
        drop(ending);
        // The next one serves and answers it.
        let _next = serve(&place);
        let (next, _) = inspector.accept().unwrap();
        let mut again = String::new();
        BufReader::new(&next).read_line(&mut again).unwrap();
        writeln!(&next, "{}", json!({ "answered": again.trim() })).unwrap();
        let answer = shell.next();
        assert_eq!(
            answer["result"]["answered"],
            request.to_string(),
            "{answer}"
        );
        assert_eq!(line.trim(), request.to_string());
        drop(host);
    }

    #[test]
    fn two_shells_hear_every_event_each_through_its_own_cursor() {
        let (host, place) = host("cursors", false);
        let first = Shell::connect(&place.socket());
        assert_eq!(first.hello(), Vec::<Value>::new());
        host.0.record(json!({ "event": "running" }));
        assert_eq!(
            first.next()["params"],
            json!({ "event": "running", "seq": 1 })
        );

        // A shell that attaches later hears what happened so far, then
        // what happens next, numbered on.
        let second = Shell::connect(&place.socket());
        assert_eq!(second.hello(), [json!({ "event": "running", "seq": 1 })]);
        host.0.record(json!({ "event": "swapped" }));
        for shell in [&first, &second] {
            assert_eq!(
                shell.next()["params"],
                json!({ "event": "swapped", "seq": 2 })
            );
        }
    }

    #[test]
    fn a_pause_one_shell_asks_for_reaches_the_other_and_a_cancel_ends_only_its_request() {
        let (host, place) = host("pause", true);
        host.0.record(json!({ "event": "running" }));
        let inspector = UnixListener::bind(place.inspector()).unwrap();
        // The program's inspector tells the session where it paused, as
        // `rawr::inspect` does.
        let told = serve(&place);
        let (holding, held) = mpsc::channel();
        let program = std::thread::spawn(move || {
            let (step, _) = inspector.accept().unwrap();
            let mut request = String::new();
            BufReader::new(&step).read_line(&mut request).unwrap();
            let at = json!({ "file": "/p/src/main.rs", "line": 3, "reason": "step" });
            let mut paused = at.clone();
            paused["event"] = json!("paused");
            writeln!(
                &told,
                "{}",
                json!({ "jsonrpc": "2.0", "method": "emit", "params": paused })
            )
            .unwrap();
            writeln!(&step, "{}", json!({ "paused": at })).unwrap();
            // A read the program never answers, until its tool goes away.
            let (held, _) = inspector.accept().unwrap();
            let mut reader = BufReader::new(&held);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            holding.send(()).unwrap();
            line.clear();
            reader.read_line(&mut line).unwrap()
        });

        let stepping = Shell::connect(&place.socket());
        let watching = Shell::connect(&place.socket());
        stepping.hello();
        watching.hello();
        stepping.send(&json!({
            "jsonrpc": "2.0", "id": 1, "method": "program", "params": { "op": "step" }
        }));
        let mut answered = None;
        let mut heard = None;
        while answered.is_none() || heard.is_none() {
            let message = stepping.next();
            if message["id"] == 1 {
                answered = Some(message["result"]["paused"]["line"].clone());
            } else if message["method"] == "event" {
                heard = Some(message["params"]["event"].clone());
            }
        }
        assert_eq!((answered, heard), (Some(json!(3)), Some(json!("paused"))));
        let other = watching.next();
        assert_eq!(other["method"], "event", "{other}");
        assert_eq!(other["params"]["event"], "paused", "{other}");
        assert_eq!(other["params"]["seq"], 2, "{other}");

        // The watching shell cancels its own read; the stepping shell's
        // connection hears nothing of it.
        watching.send(&json!({
            "jsonrpc": "2.0", "id": 7, "method": "program", "params": { "name": "count" }
        }));
        held.recv().unwrap();
        watching.send(&json!({ "jsonrpc": "2.0", "method": "cancel", "params": { "id": 7 } }));
        let cancelled = watching.next();
        assert_eq!(cancelled["id"], 7, "{cancelled}");
        assert_eq!(cancelled["error"]["code"], CANCELLED, "{cancelled}");
        // The program saw its tool go away.
        assert_eq!(program.join().unwrap(), 0);
        drop(host);
    }
}
