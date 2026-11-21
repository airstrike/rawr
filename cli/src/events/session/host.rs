//! The session an owner hosts: the program's state as the owner's events
//! tell it, the connections that hear them, and the requests it forwards
//! to the program's inspector.
//!
//! One thread owns the session ([`Session`]): the state, the output kept,
//! the connections that hear, the requests in flight and the control turn.
//! Each connection's reader, each request's thread, the owner's events and
//! its copy of the program's output send to its inbox, and it takes each
//! message in the order sent. A `hello` that waits for the output the
//! program kept, and a request that waits for an inspector or for its
//! turn, park a reply on the owner. The owner looks at every parked reply
//! after each message, so none waits on something already true, and one
//! ends when what it waits for comes, when its request is cancelled or
//! when the connection that asked closes.

use std::collections::VecDeque;
use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::OnceLock;

use serde_json::{json, Value};

use super::handover;
use super::place::Place;
use super::state::{Phase, State};

/// The protocol's version, which `hello` says.
const PROTOCOL: u64 = 1;
/// Output kept for shells that attach later, oldest first out.
pub(super) const OUTPUT: usize = rawr::teller::OUTPUT;

/// The error a `program` request ends with when no program will answer it.
pub const NOT_RUNNING: i64 = -32001;
/// The error a cancelled request ends with.
pub const CANCELLED: i64 = -32800;

/// The inbox of the session this process hosts for its run, once [`open`]
/// made it. The owner's events, its copy of the program's output and its
/// program's start reach the session through it from free functions any
/// thread calls. A process hosts one session, so it is set once.
static OPEN: OnceLock<Inbox> = OnceLock::new();

/// A session an owner hosts, which leaves its place when dropped.
pub struct Host(Inbox);

/// The inbox of a session's owner thread, and what stays the same while
/// the session serves.
#[derive(Clone)]
struct Inbox {
    send: mpsc::Sender<Message>,
    place: Place,
    /// A shell started the run: it tells the run's output.
    handed: bool,
    /// The shell that started the run reads its stderr as rawr's own
    /// lines, so the program's stderr reaches it through the session
    /// instead ([`PROGRAM_STDERR`](crate::events::PROGRAM_STDERR)).
    program_stderr: bool,
}

/// What a run's environment says of its session as it opens.
#[derive(Clone, Copy, Debug, Default)]
struct Opening {
    /// A shell started the run.
    handed: bool,
    /// The run's first program waits at its start for the shell that
    /// started the run (#2274).
    hold: bool,
    /// The program's stderr reaches the shell through the session (#2367).
    program_stderr: bool,
}

/// What the session's owner thread hears.
enum Message {
    /// A shell on `connection` said `hello` as request `id`. The owner
    /// answers on `send`, then drops `greeted`, which the connection's
    /// reader waits for before it reads on.
    Hello {
        connection: u64,
        id: Value,
        send: mpsc::Sender<Arc<str>>,
        greeted: mpsc::Sender<()>,
    },
    /// Connection `connection` is the program's, which tells its output.
    Tell {
        connection: u64,
        send: mpsc::Sender<Arc<str>>,
    },
    /// The program on `connection` told the output it kept.
    Told { connection: u64 },
    /// The program's inspector listens on `socket` while `connection`
    /// stays open.
    Serve { connection: u64, socket: PathBuf },
    /// An event, which takes the next number and stays in the history.
    Record(Value),
    /// An event that says what is so now: the `breakpoints` list.
    Now(Value),
    /// Output the shell on `from` read, for the other connections.
    Output { from: u64, piece: Value },
    /// Output the owner copied from the program's streams.
    Copied(Value),
    /// The owner starts a program: the reply says what its command takes.
    Start(mpsc::Sender<Start>),
    /// Request `ticket`, whose JSON id is `id`, is in flight: a `control`
    /// one takes the turn.
    Asked {
        ticket: Ticket,
        id: String,
        control: bool,
    },
    /// Request `ticket` waits for an inspector other than the one on
    /// connection `gone`. The reply comes once one serves, and the request
    /// has the turn if it is a control request.
    Reach {
        ticket: Ticket,
        gone: Option<u64>,
        debugs: bool,
        reply: mpsc::Sender<Reached>,
    },
    /// Request `ticket` talks to the inspector on `stream`, which a cancel
    /// shuts.
    Exchange { ticket: Ticket, stream: UnixStream },
    /// Request `ticket` is over.
    Finished(Ticket),
    /// The shell on `connection` cancelled its request `id`.
    Cancel { connection: u64, id: String },
    /// Connection `connection` closed.
    Left(u64),
    /// A test waits for the session to reach a state.
    #[cfg(test)]
    Wait(Wait),
}

/// A test's wait: the owner replies once `met` holds of the session.
#[cfg(test)]
struct Wait {
    met: Box<dyn Fn(&Session) -> bool + Send>,
    reply: mpsc::Sender<()>,
}

/// What a program the owner starts takes in its command.
#[derive(Clone, Copy, Debug, Default)]
struct Start {
    /// It waits at its start: it is the run's first.
    hold: bool,
    /// It registers its debugger at a shell's first request for one.
    on_request: bool,
}

/// One `program` request: the connection that asked it and its number
/// there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Ticket {
    connection: u64,
    number: u64,
}

/// The answer to a [`Message::Reach`].
type Reached = Result<Served, Refusal>;

/// The session, as its owner thread holds it.
struct Session {
    root: PathBuf,
    place: Place,
    handed: bool,
    state: State,
    output: VecDeque<Value>,
    output_bytes: usize,
    listeners: Vec<Listener>,
    /// The program's connection that tells its output, in a run no shell
    /// started.
    teller: Option<Teller>,
    /// A shell said `hello`: the program tells each piece of its output
    /// as it writes it.
    listening: bool,
    /// The run's output reaches the session: a shell started the run, or
    /// its program or owner tells it.
    tells: bool,
    /// The program's own connection told its output: its streams are the
    /// terminal, which it writes to itself, so only what the engine prints
    /// reaches the session and a dependency's own writes don't (#2411).
    at_terminal: bool,
    inspector: Option<Served>,
    debugger: Debugger,
    /// The next program the owner starts waits at its start for the shell
    /// that started the run (#2274): only the first, as a program started
    /// again has the breakpoints from its start.
    hold: bool,
    /// The newest `breakpoints` list the inspector told. `hello` gives it
    /// when it is newer than the list in the inspector's file.
    breakpoints: Option<Value>,
    /// The `hello`s waiting for the output the program kept.
    greetings: Vec<Greeting>,
    /// The `program` requests in flight, oldest first.
    requests: Vec<Request>,
    /// The control request that reaches the program now.
    turn: Option<Ticket>,
    #[cfg(test)]
    waits: Vec<Wait>,
}

/// A connection that said `hello`, and hears what happens from then on.
struct Listener {
    connection: u64,
    send: mpsc::Sender<Arc<str>>,
}

/// A `hello` the owner answers once the program told what it kept.
struct Greeting {
    connection: u64,
    id: Value,
    send: mpsc::Sender<Arc<str>>,
    greeted: mpsc::Sender<()>,
}

/// The program's connection that tells its output (`rawr::teller::open`): it
/// keeps the last of it until the session asks for it with `listen`, then
/// says `told` and tells each piece as it writes it.
struct Teller {
    connection: u64,
    send: mpsc::Sender<Arc<str>>,
    told: bool,
}

/// The program's inspector, listening on `socket` while its connection
/// `connection` stays open.
#[derive(Clone, Debug)]
struct Served {
    connection: u64,
    socket: PathBuf,
}

/// A `program` request in flight, as the owner keeps it.
struct Request {
    ticket: Ticket,
    /// Its JSON id, as text, which a `cancel` names.
    id: String,
    /// It takes the turn: control requests reach the program one at a
    /// time.
    control: bool,
    cancelled: bool,
    /// Its socket to the inspector now, which a cancel shuts.
    exchange: Option<UnixStream>,
    /// Its reply while it waits for an inspector or for its turn.
    parked: Option<Parked>,
}

/// A request's [`Message::Reach`] the owner hasn't answered yet.
struct Parked {
    gone: Option<u64>,
    debugs: bool,
    reply: mpsc::Sender<Reached>,
}

/// When the programs the owner starts register a debugger. Each serves
/// its inspector from its start either way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Debugger {
    /// At a shell's first request that needs one, while the program runs:
    /// it keeps its state.
    #[default]
    OnRequest,
    /// From its start: a shell asked for one already.
    On,
}

/// Why a `program` request has no answer.
#[derive(Debug)]
enum Refusal {
    NotRunning(Phase),
    Cancelled,
}

/// Host the session of the program at `root` until the host drops: on the
/// listener a shell handed this run, `handed`, or on a socket at this
/// process's place for the root, beside any other run's.
/// `None` when this process hosts that place's session already, or, with a
/// line on stderr, when the socket can't be made; the program runs as it
/// would without one.
pub fn open(root: &Path, handed: Option<handover::Handed>) -> Option<Host> {
    let set = |name| std::env::var_os(name).is_some_and(|value| !value.is_empty());
    let started = handover::handed();
    let opening = Opening {
        handed: started,
        hold: set(crate::events::HOLD),
        program_stderr: started && set(crate::events::PROGRAM_STDERR),
    };
    let opened = (|| {
        let (place, listener) = match handed {
            Some(handed) => {
                let listener = handed.into_listener();
                (Place::bound(&listener)?, listener)
            }
            None => {
                let place = Place::new(root)?;
                place.make()?;
                match listen(&place.socket())? {
                    Some(listener) => (place, listener),
                    None => return Ok(None),
                }
            }
        };
        Host::serve(root, place, listener, opening).map(Some)
    })();
    match opened {
        Ok(Some(host)) => {
            // A second session in one process hears only its connections.
            let _ = OPEN.set(host.0.clone());
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
/// connection. It reaches the owner's inbox before this returns, so a
/// message sent after it sees it.
pub fn record(event: &Value) {
    if let Some(inbox) = OPEN.get() {
        let _ = inbox.send.send(Message::Record(event.clone()));
    }
}

/// Tell `bytes` the program wrote to `stream` to every shell attached to the
/// session this process hosts, and keep them for shells that attach later:
/// an owner that holds the program's output calls it as it copies it to
/// the terminal. A shell that started the run holds that output itself,
/// and tells it, except the program's stderr when the shell reads the
/// run's stderr as rawr's own: that the session takes in place of the
/// owner's stderr, and this answers `true`.
pub fn output(stream: rawr::Output, bytes: &[u8]) -> bool {
    OPEN.get().is_some_and(|inbox| inbox.copied(stream, bytes))
}

/// Whether the session takes the program's stderr in place of this
/// process's own, as [`output`] does: an owner whose program inherits its
/// stderr then gives the program a pipe of its own and copies it.
#[must_use]
pub fn takes_stderr() -> bool {
    OPEN.get().is_some_and(|inbox| inbox.program_stderr)
}

/// Ready `command`, which starts the program, for the session: never the
/// listener this run was handed, the debugger's sockets, where the program
/// tells its output when no shell holds it, and unless a shell asked for
/// the debugger already,
/// [`DEBUGGER_ON_REQUEST`](crate::events::DEBUGGER_ON_REQUEST). The
/// program then serves the inspector and registers the debugger at a
/// shell's first request for it, keeping its state (#2266, #2297); one
/// that started with it keeps it on.
pub fn prepare(command: &mut Command) {
    command
        .env_remove(handover::LISTENER)
        .env_remove(crate::events::HOLD)
        .env_remove(crate::events::PROGRAM_STDERR)
        .env_remove(crate::events::OUTPUT);
    if let Some(inbox) = OPEN.get() {
        inbox.ready(command);
    }
}

/// A listener on `socket`, in place of a stale one: `None` when a session
/// listens there.
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
    /// `place`, as `opening` says.
    fn serve(
        root: &Path,
        place: Place,
        listener: UnixListener,
        opening: Opening,
    ) -> std::io::Result<Self> {
        place.clear();
        let (send, inbox) = mpsc::channel();
        let session = Session::new(root, place.clone(), opening);
        std::thread::Builder::new()
            .name("rawr-session".into())
            .spawn(move || session.own(&inbox))?;
        let accepting = send.clone();
        std::thread::Builder::new()
            .name("rawr-session-accept".into())
            .spawn(move || accept(&accepting, &listener))?;
        Ok(Self(Inbox {
            send,
            place,
            handed: opening.handed,
            program_stderr: opening.program_stderr,
        }))
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.0.place.remove();
    }
}

impl Inbox {
    /// Tell `bytes` the owner copied from the program's `stream`, unless a
    /// shell that started the run holds that output. Whether the session
    /// took them in place of the owner's own stream: the program's stderr,
    /// for a shell that reads the run's stderr as rawr's own.
    fn copied(&self, stream: rawr::Output, bytes: &[u8]) -> bool {
        let takes = self.program_stderr && matches!(stream, rawr::Output::Stderr);
        if self.handed && !takes {
            return false;
        }
        let stream = match stream {
            rawr::Output::Stderr => "stderr",
            _ => "stdout",
        };
        let text = String::from_utf8_lossy(bytes);
        let _ = self
            .send
            .send(Message::Copied(json!({ "stream": stream, "text": text })));
        takes
    }

    /// Ready `command` for the session, as [`prepare`] does. The owner's
    /// reply says whether the program holds at its start and when it
    /// registers its debugger.
    fn ready(&self, command: &mut Command) {
        if !self.handed {
            command.env(crate::events::OUTPUT, self.place.socket());
        }
        let (reply, started) = mpsc::channel();
        let start = match self.send.send(Message::Start(reply)) {
            Ok(()) => started.recv().unwrap_or_default(),
            Err(_) => Start::default(),
        };
        if start.hold {
            command.env(crate::events::HOLD, "1");
        }
        if start.on_request {
            command.env(crate::events::DEBUGGER_ON_REQUEST, "1");
        }
        command
            .env(crate::events::INSPECT, self.place.inspector())
            .env(crate::events::DEBUG_EVENTS, self.place.socket());
    }
}

/// Read each connection `listener` accepts on a thread of its own.
fn accept(inbox: &mpsc::Sender<Message>, listener: &UnixListener) {
    let mut next = 0;
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            continue;
        };
        next += 1;
        let connection = Connection {
            inbox: inbox.clone(),
            number: next,
            asked: 0,
        };
        let _ = std::thread::Builder::new()
            .name("rawr-session-read".into())
            .spawn(move || connection.converse(&stream));
    }
}

/// One connection's reader: it turns each line into a message to the
/// owner.
struct Connection {
    inbox: mpsc::Sender<Message>,
    number: u64,
    /// The `program` requests it read so far, which number its tickets.
    asked: u64,
}

impl Connection {
    /// Read `stream`'s messages until it closes, then tell the owner, which
    /// cancels what it asked.
    fn converse(mut self, stream: &UnixStream) {
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
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else {
                break;
            };
            if let Ok(message) = serde_json::from_str::<Value>(&line) {
                self.take(message, &send);
            }
        }
        let _ = self.inbox.send(Message::Left(self.number));
    }

    /// One message from the connection, whose lines go to `send`.
    fn take(&mut self, mut message: Value, send: &mpsc::Sender<Arc<str>>) {
        let connection = self.number;
        let params = message["params"].take();
        let id = message.get("id").cloned();
        let told = match (message["method"].as_str(), id) {
            (Some("hello"), Some(id)) => {
                let (greeted, greeting) = mpsc::channel();
                let hello = Message::Hello {
                    connection,
                    id,
                    send: send.clone(),
                    greeted,
                };
                // What the connection says next waits for the answer, as
                // the shell reads it.
                if self.inbox.send(hello).is_ok() {
                    let _ = greeting.recv();
                }
                return;
            }
            (Some("program"), Some(id)) => {
                self.ask(id, params, send);
                return;
            }
            (Some("cancel"), None) => Message::Cancel {
                connection,
                id: params["id"].to_string(),
            },
            (Some("serve"), None) => {
                let Some(socket) = params["socket"].as_str() else {
                    return;
                };
                Message::Serve {
                    connection,
                    socket: PathBuf::from(socket),
                }
            }
            (Some("emit"), None) if params["event"] == "breakpoints" => Message::Now(params),
            (Some("emit"), None) if params.is_object() => Message::Record(params),
            (Some("output"), None) => Message::Output {
                from: connection,
                piece: params,
            },
            (Some("tell"), None) => Message::Tell {
                connection,
                send: send.clone(),
            },
            (Some("told"), None) => Message::Told { connection },
            (Some(method), Some(id)) => {
                let _ = send.send(line(&json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": -32601, "message": format!("unknown method `{method}`") },
                })));
                return;
            }
            _ => return,
        };
        let _ = self.inbox.send(told);
    }

    /// Forward `request`, the `program` request `id`, on a thread of its
    /// own, which answers on `send`. The owner hears of it first, so a
    /// `cancel` read after it finds it.
    fn ask(&mut self, id: Value, request: Value, send: &mpsc::Sender<Arc<str>>) {
        self.asked += 1;
        let ticket = Ticket {
            connection: self.number,
            number: self.asked,
        };
        let control = matches!(
            request["op"].as_str(),
            Some("break" | "clear" | "step" | "eval" | "input")
        );
        let asked = Message::Asked {
            ticket,
            id: id.to_string(),
            control,
        };
        if self.inbox.send(asked).is_err() {
            return;
        }
        // The request ends when this drops, on the thread or with the
        // closure a failed spawn drops.
        let over = Over {
            inbox: self.inbox.clone(),
            ticket,
        };
        let send = send.clone();
        let _ = std::thread::Builder::new()
            .name("rawr-session-program".into())
            .spawn(move || {
                let reply = match forward(&over.inbox, ticket, &request) {
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
                let _ = send.send(line(&reply));
                drop(over);
            });
    }
}

/// Tells the owner a request is over when dropped, so its turn passes on
/// even when its thread panics or never starts.
struct Over {
    inbox: mpsc::Sender<Message>,
    ticket: Ticket,
}

impl Drop for Over {
    fn drop(&mut self) {
        let _ = self.inbox.send(Message::Finished(self.ticket));
    }
}

/// The answer to `request` from the program's inspector. A program that
/// ends while it holds the request leaves it to the next one. Control
/// requests take turns; `pause` and `continue` go at once, as they end a
/// step that waits for a pause the program may never reach.
fn forward(
    inbox: &mpsc::Sender<Message>,
    ticket: Ticket,
    request: &Value,
) -> Result<Value, Refusal> {
    // Window input, screenshots and menus need no debugger.
    let debugs = !matches!(
        request["op"].as_str(),
        Some("input" | "screenshot" | "menu")
    );
    let mut gone = None;
    loop {
        let (reply, reached) = mpsc::channel();
        let reach = Message::Reach {
            ticket,
            gone,
            debugs,
            reply,
        };
        if inbox.send(reach).is_err() {
            return Err(Refusal::Cancelled);
        }
        // The owner answers every parked reach before it drops it.
        let served = reached.recv().unwrap_or(Err(Refusal::Cancelled))?;
        match ask(inbox, ticket, &served.socket, request) {
            Some(answer) => return Ok(answer),
            None => gone = Some(served.connection),
        }
    }
}

/// One request and its answer on the inspector at `socket`. `None` when
/// the exchange ends before an answer: nothing listens, the process that
/// read it ended, as a restart retires it (#1964), or a cancel shut it.
/// The next reach tells these apart.
fn ask(
    inbox: &mpsc::Sender<Message>,
    ticket: Ticket,
    socket: &Path,
    request: &Value,
) -> Option<Value> {
    let stream = UnixStream::connect(socket).ok()?;
    if let Ok(held) = stream.try_clone() {
        let _ = inbox.send(Message::Exchange {
            ticket,
            stream: held,
        });
    }
    writeln!(&stream, "{request}").ok()?;
    let mut answer = String::new();
    BufReader::new(&stream).read_line(&mut answer).ok()?;
    if !answer.ends_with('\n') {
        return None;
    }
    serde_json::from_str(&answer).ok()
}

impl Session {
    fn new(root: &Path, place: Place, opening: Opening) -> Self {
        Self {
            root: root.to_path_buf(),
            place,
            handed: opening.handed,
            state: State::default(),
            output: VecDeque::new(),
            output_bytes: 0,
            listeners: Vec::new(),
            teller: None,
            listening: false,
            tells: false,
            at_terminal: false,
            inspector: None,
            debugger: if opening.handed {
                Debugger::On
            } else {
                Debugger::OnRequest
            },
            hold: opening.hold,
            breakpoints: None,
            greetings: Vec::new(),
            requests: Vec::new(),
            turn: None,
            #[cfg(test)]
            waits: Vec::new(),
        }
    }

    /// Take each message until every sender is gone, answering what each
    /// one lets go.
    fn own(mut self, inbox: &mpsc::Receiver<Message>) {
        for message in inbox {
            self.take(message);
            self.settle();
        }
    }

    fn take(&mut self, message: Message) {
        match message {
            Message::Hello {
                connection,
                id,
                send,
                greeted,
            } => {
                if !std::mem::replace(&mut self.listening, true)
                    && let Some(teller) = &self.teller
                {
                    let _ = teller.send.send(asking());
                }
                self.greetings.push(Greeting {
                    connection,
                    id,
                    send,
                    greeted,
                });
            }
            Message::Tell { connection, send } => self.teller(connection, send),
            Message::Told { connection } => {
                if let Some(teller) = self.teller.as_mut()
                    && teller.connection == connection
                {
                    teller.told = true;
                }
            }
            Message::Serve { connection, socket } => {
                self.inspector = Some(Served { connection, socket });
            }
            Message::Record(event) => self.record(event),
            Message::Now(event) => self.now(event),
            Message::Output { from, piece } => self.output(from, piece),
            Message::Copied(piece) => {
                self.tells = true;
                self.output(0, piece);
            }
            Message::Start(reply) => {
                let _ = reply.send(Start {
                    hold: std::mem::take(&mut self.hold),
                    on_request: self.debugger == Debugger::OnRequest,
                });
            }
            Message::Asked {
                ticket,
                id,
                control,
            } => self.requests.push(Request {
                ticket,
                id,
                control,
                cancelled: false,
                exchange: None,
                parked: None,
            }),
            Message::Reach {
                ticket,
                gone,
                debugs,
                reply,
            } => match self.request(ticket) {
                Some(request) => {
                    request.exchange = None;
                    request.parked = Some(Parked {
                        gone,
                        debugs,
                        reply,
                    });
                }
                None => {
                    let _ = reply.send(Err(Refusal::Cancelled));
                }
            },
            Message::Exchange { ticket, stream } => match self.request(ticket) {
                Some(request) if !request.cancelled => request.exchange = Some(stream),
                _ => {
                    let _ = stream.shutdown(std::net::Shutdown::Both);
                }
            },
            Message::Finished(ticket) => {
                self.requests.retain(|request| request.ticket != ticket);
                if self.turn == Some(ticket) {
                    self.turn = None;
                }
            }
            Message::Cancel { connection, id } => {
                let asked = self
                    .requests
                    .iter_mut()
                    .filter(|request| request.ticket.connection == connection && request.id == id);
                for request in asked {
                    request.cancel();
                }
            }
            Message::Left(connection) => self.left(connection),
            #[cfg(test)]
            Message::Wait(wait) => self.waits.push(wait),
        }
    }

    fn request(&mut self, ticket: Ticket) -> Option<&mut Request> {
        self.requests
            .iter_mut()
            .find(|request| request.ticket == ticket)
    }

    /// Answer each parked reply whose wait is over: the `hello`s once the
    /// program told what it kept, then each request in the order asked.
    fn settle(&mut self) {
        if self.teller.as_ref().is_none_or(|teller| teller.told) {
            for greeting in std::mem::take(&mut self.greetings) {
                self.greet(greeting);
            }
        }
        for index in 0..self.requests.len() {
            if let Some(reached) = self.reached(index)
                && let Some(parked) = self.requests[index].parked.take()
            {
                let _ = parked.reply.send(reached);
            }
        }
        #[cfg(test)]
        {
            let waits = std::mem::take(&mut self.waits);
            for wait in waits {
                if (wait.met)(self) {
                    let _ = wait.reply.send(());
                } else {
                    self.waits.push(wait);
                }
            }
        }
    }

    /// The answer to request `index`'s parked reach, once it has one: the
    /// inspector, once the program serves one, not while it starts or
    /// restarts, not the one on the connection that reach named gone, and
    /// for a control request once the turn is its own. A program
    /// registers its debugger at the request that debugs, and the programs
    /// started after it have one from their start.
    fn reached(&mut self, index: usize) -> Option<Reached> {
        let request = &self.requests[index];
        let parked = request.parked.as_ref()?;
        if request.cancelled {
            return Some(Err(Refusal::Cancelled));
        }
        let (ticket, control, gone, debugs) =
            (request.ticket, request.control, parked.gone, parked.debugs);
        if control && self.turn.is_some_and(|turn| turn != ticket) {
            return None;
        }
        let phase = self.state.phase();
        if let Some(served) = &self.inspector
            && !phase.is_pending()
            && gone != Some(served.connection)
        {
            let served = served.clone();
            if debugs && self.debugger == Debugger::OnRequest {
                self.debugger = Debugger::On;
            }
            if control {
                self.turn = Some(ticket);
            }
            return Some(Ok(served));
        }
        if !phase.is_live() && !phase.is_pending() {
            return Some(Err(Refusal::NotRunning(phase.clone())));
        }
        None
    }

    /// Answer `hello`: the events so far and the output kept, and from now
    /// on every event and piece of output. The answer and the connection's
    /// place among the listeners come in one step, so each event reaches
    /// it once, in the answer or after it.
    fn greet(&mut self, greeting: Greeting) {
        let events: Vec<Value> = self
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
        if self.handed || self.tells {
            capabilities.push("output");
        }
        let mut result = json!({
            "protocol": PROTOCOL,
            "capabilities": capabilities,
            "root": self.root,
            "pid": std::process::id(),
            "events": events,
            "output": self.output,
            "breakpoints": self.breakpoints(),
        });
        if let Some(coverage) = self.coverage() {
            result["coverage"] = json!(coverage);
        }
        let _ = greeting.send.send(line(
            &json!({ "jsonrpc": "2.0", "id": greeting.id, "result": result }),
        ));
        self.listeners.push(Listener {
            connection: greeting.connection,
            send: greeting.send,
        });
        // The connection's reader goes on once this closes.
        drop(greeting.greeted);
    }

    /// How much of the run's output `hello` gives, once any reaches the
    /// session: `all` when a shell that started the run tells it off
    /// pipes, or the owner copies it off pipes, and `engine` when the
    /// program writes to the terminal itself and tells only what the
    /// engine prints (#2411).
    fn coverage(&self) -> Option<&'static str> {
        if self.handed {
            Some("all")
        } else if self.at_terminal {
            Some("engine")
        } else {
            self.tells.then_some("all")
        }
    }

    fn record(&mut self, event: Value) {
        let seq = self.state.next_number();
        let told = line(&json!({
            "jsonrpc": "2.0",
            "method": "event",
            "params": numbered(seq, &event),
        }));
        self.state.record(seq, event);
        self.listeners
            .retain(|listener| listener.send.send(Arc::clone(&told)).is_ok());
    }

    /// Tell every connection `event`, which says what is so now rather
    /// than what happened: it takes no number and stays in no history, as
    /// `hello` says the same. The newest list is kept for `hello`.
    fn now(&mut self, event: Value) {
        let told = line(&json!({ "jsonrpc": "2.0", "method": "event", "params": event }));
        if self
            .breakpoints
            .as_ref()
            .is_none_or(|kept| revision(kept) <= revision(&event))
        {
            self.breakpoints = Some(event);
        }
        self.listeners
            .retain(|listener| listener.send.send(Arc::clone(&told)).is_ok());
    }

    /// The breakpoints as told in their `breakpoints` event: the newer of
    /// the list in the inspector's file and the last list it told, so a
    /// file read before a write never takes back a list a shell already
    /// heard. None before it kept any.
    fn breakpoints(&self) -> Value {
        let kept = std::fs::read(self.place.breakpoints())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({ "revision": 0, "breakpoints": [] }));
        let mut newest = match &self.breakpoints {
            Some(told) if revision(told) > revision(&kept) => told.clone(),
            _ => kept,
        };
        newest["event"] = json!("breakpoints");
        newest
    }

    /// Keep output the shell on `from` read, and tell the others.
    fn output(&mut self, from: u64, piece: Value) {
        let Some(bytes) = piece["text"].as_str().map(str::len) else {
            return;
        };
        let told = line(&json!({ "jsonrpc": "2.0", "method": "output", "params": piece }));
        self.output_bytes += bytes;
        self.output.push_back(piece);
        while self.output_bytes > OUTPUT {
            let Some(dropped) = self.output.pop_front() else {
                break;
            };
            self.output_bytes -= dropped["text"].as_str().map_or(0, str::len);
        }
        for listener in &self.listeners {
            if listener.connection != from {
                let _ = listener.send.send(Arc::clone(&told));
            }
        }
    }

    /// Connection `connection` is the program's, which tells its output. A
    /// program started after a shell said `hello` tells it at once.
    fn teller(&mut self, connection: u64, send: mpsc::Sender<Arc<str>>) {
        self.tells = true;
        self.at_terminal = true;
        if self.listening {
            let _ = send.send(asking());
        }
        self.teller = Some(Teller {
            connection,
            send,
            told: false,
        });
    }

    /// Connection `connection` closed: it hears nothing more, an inspector
    /// it served, or output it told, is gone, and its requests are
    /// cancelled.
    fn left(&mut self, connection: u64) {
        self.listeners
            .retain(|listener| listener.connection != connection);
        self.greetings
            .retain(|greeting| greeting.connection != connection);
        if self
            .inspector
            .as_ref()
            .is_some_and(|served| served.connection == connection)
        {
            self.inspector = None;
        }
        if self
            .teller
            .as_ref()
            .is_some_and(|teller| teller.connection == connection)
        {
            self.teller = None;
        }
        let asked = self
            .requests
            .iter_mut()
            .filter(|request| request.ticket.connection == connection);
        for request in asked {
            request.cancel();
        }
    }
}

impl Request {
    /// End the request: its exchange with the inspector shuts, and its
    /// next reach is answered as cancelled.
    fn cancel(&mut self) {
        self.cancelled = true;
        if let Some(stream) = self.exchange.take() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }
}

/// A `breakpoints` list's revision, which counts its changes.
fn revision(list: &Value) -> u64 {
    list["revision"].as_u64().unwrap_or(0)
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

/// What asks the program for the output it kept, and for each piece from
/// then on.
fn asking() -> Arc<str> {
    line(&json!({ "jsonrpc": "2.0", "method": "listen" }))
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead as _, BufReader, Write as _};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::Path;
    use std::sync::mpsc;
    use std::thread::JoinHandle;

    use serde_json::{json, Value};

    use super::{Host, Message, Opening, Place, Session, Wait, CANCELLED, NOT_RUNNING};

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

        /// The answer to request `id`, past the events and output told
        /// before it.
        fn answer(&self, id: u64) -> Value {
            loop {
                let message = self.next();
                if message.get("method").is_none() {
                    assert_eq!(message["id"], id, "{message}");
                    return message;
                }
            }
        }

        /// The events from `hello`'s answer.
        fn hello(&self) -> Vec<Value> {
            self.send(&json!({ "jsonrpc": "2.0", "id": 0, "method": "hello" }));
            let answer = self.answer(0);
            answer["result"]["events"].as_array().unwrap().clone()
        }

        /// Close the connection both ways, as a shell that goes away does:
        /// the reading thread's copy of the socket closes with it.
        fn close(self) {
            self.stream.shutdown(std::net::Shutdown::Both).unwrap();
        }
    }

    fn host(name: &str, handed: bool) -> (Host, Place) {
        opened(
            name,
            Opening {
                handed,
                ..Opening::default()
            },
        )
    }

    fn opened(name: &str, opening: Opening) -> (Host, Place) {
        let root = format!("/rawr-session-test/{name}/{}", std::process::id());
        let place = Place::new(Path::new(&root)).unwrap();
        place.make().unwrap();
        let _ = std::fs::remove_file(place.socket());
        let listener = UnixListener::bind(place.socket()).unwrap();
        let host = Host::serve(Path::new(&root), place.clone(), listener, opening).unwrap();
        (host, place)
    }

    impl Host {
        /// Record `event`, as the owner's `emit` does.
        fn record(&self, event: Value) {
            self.0.send.send(Message::Record(event)).unwrap();
        }

        /// Wait for `met` to hold of the session.
        fn reaches(&self, met: impl Fn(&Session) -> bool + Send + 'static) {
            let (reply, done) = mpsc::channel();
            self.0
                .send
                .send(Message::Wait(Wait {
                    met: Box::new(met),
                    reply,
                }))
                .unwrap();
            done.recv().unwrap();
        }
    }

    /// The program's inspector says where it serves on a connection of its
    /// own, held open while it does.
    fn serve(place: &Place) -> UnixStream {
        let told = UnixStream::connect(place.socket()).unwrap();
        let serve = json!({ "jsonrpc": "2.0", "method": "serve", "params": { "socket": place.inspector() } });
        writeln!(&told, "{serve}").unwrap();
        told
    }

    /// The line one exchange with the inspector carries.
    fn read(stream: &UnixStream) -> String {
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).unwrap();
        line
    }

    /// A program that ends while it holds a request, as a restart retires
    /// it, leaves the request to the program that serves next (#1964).
    #[test]
    fn a_request_the_ending_program_held_goes_to_the_next_one() {
        let (host, place) = host("retire", true);
        host.record(json!({ "event": "running" }));
        let inspector = UnixListener::bind(place.inspector()).unwrap();
        let ending = serve(&place);
        let shell = Shell::connect(&place.socket());
        shell.hello();
        let request = json!({ "op": "screenshot" });
        shell.send(&json!({ "jsonrpc": "2.0", "id": 1, "method": "program", "params": request }));
        // The retiring program reads the request and ends.
        let (held, _) = inspector.accept().unwrap();
        let line = read(&held);
        drop(held);
        drop(ending);
        // The next one serves and answers it.
        let _next = serve(&place);
        let (next, _) = inspector.accept().unwrap();
        let again = read(&next);
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

    /// A step the program holds, waiting for a pause it may never reach,
    /// keeps no continue waiting behind it.
    #[test]
    fn a_continue_reaches_the_program_while_a_step_waits() {
        let (host, place) = host("continue", true);
        host.record(json!({ "event": "running" }));
        let inspector = UnixListener::bind(place.inspector()).unwrap();
        let _told = serve(&place);
        let shell = Shell::connect(&place.socket());
        shell.hello();
        shell.send(&json!({
            "jsonrpc": "2.0", "id": 1, "method": "program", "params": { "op": "step" }
        }));
        let (step, _) = inspector.accept().unwrap();
        read(&step);
        shell.send(&json!({
            "jsonrpc": "2.0", "id": 2, "method": "program", "params": { "op": "continue" }
        }));
        let (go, _) = inspector.accept().unwrap();
        assert_eq!(read(&go).trim(), json!({ "op": "continue" }).to_string());
        writeln!(&go, "{}", json!({ "paused": null })).unwrap();
        writeln!(&step, "{}", json!({ "paused": null })).unwrap();
        let mut answered = [shell.next()["id"].clone(), shell.next()["id"].clone()];
        answered.sort_by_key(|id| id.as_u64());
        assert_eq!(answered, [json!(1), json!(2)]);
        drop(host);
    }

    /// Whether `command` starts a program that registers its debugger at a
    /// shell's first request, beside the inspector's socket.
    fn on_request(command: &std::process::Command) -> bool {
        let envs: Vec<_> = command.get_envs().collect();
        assert!(envs
            .iter()
            .any(|(name, value)| *name == crate::events::INSPECT && value.is_some()));
        envs.iter()
            .any(|(name, value)| *name == crate::events::DEBUGGER_ON_REQUEST && value.is_some())
    }

    /// One request, which the program's inspector on `inspector` reads and
    /// answers.
    fn answered(shell: &Shell, inspector: &UnixListener, id: u64, request: &Value) {
        shell.send(&json!({ "jsonrpc": "2.0", "id": id, "method": "program", "params": request }));
        let (asked, _) = inspector.accept().unwrap();
        assert_eq!(read(&asked).trim(), request.to_string());
        writeln!(&asked, "{}", json!({ "answered": id })).unwrap();
        assert_eq!(shell.answer(id)["result"]["answered"], id);
    }

    /// A program no shell started serves its inspector and registers the
    /// debugger at the first request that needs one, in place: the owner
    /// restarts nothing, and the programs it starts after that have the
    /// debugger from their start (#2266, #2297).
    #[test]
    fn a_program_registers_its_debugger_at_the_first_request_that_needs_one() {
        let (host, place) = host("on-request", false);
        let mut started = std::process::Command::new("true");
        host.0.ready(&mut started);
        assert!(on_request(&started));
        host.record(json!({ "event": "running" }));
        let inspector = UnixListener::bind(place.inspector()).unwrap();
        let _told = serve(&place);
        let shell = Shell::connect(&place.socket());
        shell.hello();
        // A screenshot needs no debugger.
        answered(&shell, &inspector, 1, &json!({ "op": "screenshot" }));
        let mut again = std::process::Command::new("true");
        host.0.ready(&mut again);
        assert!(on_request(&again));
        // A read does: the program registers it.
        answered(&shell, &inspector, 2, &json!({ "name": "count" }));
        let mut restarted = std::process::Command::new("true");
        host.0.ready(&mut restarted);
        assert!(!on_request(&restarted));
        drop(host);
    }

    /// A run whose shell holds its start holds only the first program the
    /// owner starts; one started again goes at once (#2274).
    #[test]
    fn only_the_first_program_of_a_held_run_waits_at_its_start() {
        let held = |command: &std::process::Command| {
            command
                .get_envs()
                .find(|(name, _)| *name == crate::events::HOLD)
                .and_then(|(_, value)| value)
                .is_some()
        };
        let (host, _) = opened(
            "hold",
            Opening {
                handed: true,
                hold: true,
                ..Opening::default()
            },
        );
        let mut first = std::process::Command::new("true");
        host.0.ready(&mut first);
        assert!(held(&first));
        let mut again = std::process::Command::new("true");
        host.0.ready(&mut again);
        assert!(!held(&again));
    }

    /// A breakpoint one shell sets reaches the other as a `breakpoints`
    /// event, unnumbered, and a shell that attaches later has the list in
    /// `hello` (#2278).
    #[test]
    fn a_breakpoint_one_shell_sets_reaches_the_others() {
        let (host, place) = host("breakpoints", true);
        host.record(json!({ "event": "running" }));
        let inspector = UnixListener::bind(place.inspector()).unwrap();
        let told = serve(&place);
        let setting = Shell::connect(&place.socket());
        let watching = Shell::connect(&place.socket());
        setting.hello();
        watching.send(&json!({ "jsonrpc": "2.0", "id": 0, "method": "hello" }));
        let greeted = watching.next();
        assert_eq!(
            greeted["result"]["breakpoints"],
            json!({ "event": "breakpoints", "revision": 0, "breakpoints": [] })
        );
        setting.send(&json!({
            "jsonrpc": "2.0", "id": 1, "method": "program",
            "params": { "op": "break", "file": "/p/src/main.rs", "line": 4 }
        }));
        // The program's inspector keeps the list and tells it, as
        // `rawr::inspect` does, then answers.
        let (asked, _) = inspector.accept().unwrap();
        read(&asked);
        let kept = json!({
            "revision": 1,
            "breakpoints": [{ "file": "/p/src/main.rs", "line": 4, "text": "tick();" }],
        });
        std::fs::write(place.breakpoints(), kept.to_string()).unwrap();
        let mut event = kept.clone();
        event["event"] = json!("breakpoints");
        let emit = json!({ "jsonrpc": "2.0", "method": "emit", "params": event });
        writeln!(&told, "{emit}").unwrap();
        writeln!(
            &asked,
            "{}",
            json!({ "set": kept["breakpoints"][0], "revision": 1 })
        )
        .unwrap();
        let heard = watching.next();
        assert_eq!(heard["method"], "event", "{heard}");
        assert_eq!(heard["params"], event, "a told list takes no number");

        let late = Shell::connect(&place.socket());
        late.send(&json!({ "jsonrpc": "2.0", "id": 0, "method": "hello" }));
        let greeted = late.next();
        assert_eq!(greeted["result"]["breakpoints"], event);
        assert_eq!(
            greeted["result"]["events"],
            json!([{ "event": "running", "seq": 1 }]),
            "the list stays out of the history"
        );
        drop(host);
    }

    /// `hello` gives the newer of the inspector's file and the list it
    /// last told, whichever was written last: a list a shell heard is
    /// never taken back by an older file, and a newer file wins over an
    /// older told list.
    #[test]
    fn hello_gives_the_newest_breakpoints_whichever_reached_the_session_first() {
        let (host, place) = host("breakpoints-order", true);
        let told = serve(&place);
        let list = |revision: u64, line: u64| {
            json!({
                "event": "breakpoints",
                "revision": revision,
                "breakpoints": [{ "file": "/p/src/main.rs", "line": line }],
            })
        };
        // The told list is newer than the file.
        std::fs::write(place.breakpoints(), list(1, 4).to_string()).unwrap();
        let emit = json!({ "jsonrpc": "2.0", "method": "emit", "params": list(2, 5) });
        writeln!(&told, "{emit}").unwrap();
        host.reaches(|session| session.breakpoints.is_some());
        let shell = Shell::connect(&place.socket());
        shell.send(&json!({ "jsonrpc": "2.0", "id": 0, "method": "hello" }));
        assert_eq!(shell.answer(0)["result"]["breakpoints"], list(2, 5));
        // The file is newer than the told list.
        std::fs::write(place.breakpoints(), list(3, 6).to_string()).unwrap();
        let late = Shell::connect(&place.socket());
        late.send(&json!({ "jsonrpc": "2.0", "id": 0, "method": "hello" }));
        assert_eq!(late.answer(0)["result"]["breakpoints"], list(3, 6));
        drop(host);
    }

    /// An event the session records before a `hello` is in its answer and
    /// not told again; one recorded after it is told once, numbered on.
    #[test]
    fn each_event_reaches_a_shell_once_whether_it_comes_before_or_after_hello() {
        let (host, place) = host("hello-order", false);
        host.record(json!({ "event": "running" }));
        let shell = Shell::connect(&place.socket());
        assert_eq!(shell.hello(), [json!({ "event": "running", "seq": 1 })]);
        host.record(json!({ "event": "swapped" }));
        host.record(json!({ "event": "queued" }));
        let heard: Vec<Value> = [shell.next(), shell.next()]
            .into_iter()
            .map(|message| message["params"].clone())
            .collect();
        assert_eq!(
            heard,
            [
                json!({ "event": "swapped", "seq": 2 }),
                json!({ "event": "queued", "seq": 3 }),
            ]
        );
        drop(host);
    }

    /// The `hello` answer, past the notifications before it.
    fn greeted(shell: &Shell) -> Value {
        shell.send(&json!({ "jsonrpc": "2.0", "id": 0, "method": "hello" }));
        shell.answer(0)["result"].clone()
    }

    fn says_output(hello: &Value) -> bool {
        hello["capabilities"]
            .as_array()
            .is_some_and(|capabilities| capabilities.contains(&json!("output")))
    }

    /// Output an owner copies to the terminal reaches the shells attached,
    /// and `hello` replays it to a shell that attaches later (#2288).
    #[test]
    fn output_the_owner_copies_reaches_every_shell() {
        let (host, place) = host("copied", false);
        host.0.copied(rawr::Output::Stdout, b"before\n");
        let shell = Shell::connect(&place.socket());
        let hello = greeted(&shell);
        assert!(says_output(&hello), "{hello}");
        assert_eq!(
            hello["output"],
            json!([{ "stream": "stdout", "text": "before\n" }])
        );
        host.0.copied(rawr::Output::Stderr, b"after\n");
        assert_eq!(
            shell.next(),
            json!({
                "jsonrpc": "2.0",
                "method": "output",
                "params": { "stream": "stderr", "text": "after\n" },
            })
        );
        drop(host);
    }

    /// A shell that started the run holds its output and tells it: the
    /// owner tells none of it, and its program isn't asked to.
    #[test]
    fn a_run_a_shell_started_tells_no_output_of_its_own() {
        let tells = |host: &Host| {
            let mut command = std::process::Command::new("true");
            host.0.ready(&mut command);
            command
                .get_envs()
                .any(|(name, value)| name == crate::events::OUTPUT && value.is_some())
        };
        let (plain, _) = host("tells-plain", false);
        assert!(tells(&plain));
        let (handed, place) = host("tells-handed", true);
        assert!(!tells(&handed));
        handed.0.copied(rawr::Output::Stdout, b"twice\n");
        let shell = Shell::connect(&place.socket());
        assert_eq!(greeted(&shell)["output"], json!([]));
        drop((plain, handed));
    }

    /// A shell that started the run and reads its stderr as rawr's own
    /// hears the program's stderr from the session, which takes it in
    /// place of the owner's stderr; the program's stdout stays the
    /// shell's (#2367).
    #[test]
    fn a_shell_that_reads_rawr_s_lines_hears_the_program_s_stderr_from_the_session() {
        let (handed, place) = opened(
            "program-stderr",
            Opening {
                handed: true,
                program_stderr: true,
                ..Opening::default()
            },
        );
        let shell = Shell::connect(&place.socket());
        greeted(&shell);
        assert!(!handed.0.copied(rawr::Output::Stdout, b"out\n"));
        assert!(handed.0.copied(rawr::Output::Stderr, b"err\n"));
        assert_eq!(
            shell.next(),
            json!({
                "jsonrpc": "2.0",
                "method": "output",
                "params": { "stream": "stderr", "text": "err\n" },
            })
        );
        drop(handed);
    }

    /// The program's connection that tells its output, once the session
    /// read its `tell`. Each connection is read on a thread of its own, so
    /// a shell that connects later may still be read first.
    fn teller(host: &Host, place: &Place) -> UnixStream {
        let teller = UnixStream::connect(place.socket()).unwrap();
        writeln!(&teller, "{}", json!({ "jsonrpc": "2.0", "method": "tell" })).unwrap();
        host.reaches(|session| session.teller.is_some());
        teller
    }

    fn piece(text: &str) -> Value {
        json!({
            "jsonrpc": "2.0", "method": "output",
            "params": { "stream": "stdout", "text": text },
        })
    }

    /// The program on `teller`: it reads `listen`, then tells `kept` and
    /// `told`.
    fn listened(teller: UnixStream, kept: &'static str) -> JoinHandle<UnixStream> {
        std::thread::spawn(move || {
            let asked: Value = serde_json::from_str(&read(&teller)).unwrap();
            assert_eq!(asked["method"], "listen", "{asked}");
            writeln!(&teller, "{}", piece(kept)).unwrap();
            writeln!(&teller, "{}", json!({ "jsonrpc": "2.0", "method": "told" })).unwrap();
            teller
        })
    }

    /// A program whose run no shell started keeps its output until the
    /// first `hello`, which waits for it: the session asks with `listen`,
    /// the program tells what it kept and `told`, then each piece as it
    /// writes it (#2288).
    #[test]
    fn the_first_hello_has_the_output_the_program_kept() {
        let (host, place) = host("teller", false);
        let program = listened(teller(&host, &place), "kept\n");
        let shell = Shell::connect(&place.socket());
        let hello = greeted(&shell);
        assert!(says_output(&hello), "{hello}");
        assert_eq!(
            hello["output"],
            json!([{ "stream": "stdout", "text": "kept\n" }]),
            "{hello}"
        );
        // What the program writes after the `hello` reaches the shell.
        let teller = program.join().unwrap();
        writeln!(&teller, "{}", piece("live\n")).unwrap();
        assert_eq!(shell.next()["params"]["text"], "live\n");
        drop(host);
    }

    /// A `hello` waiting for the output the program kept is answered when
    /// the program's connection closes before it says `told`.
    #[test]
    fn a_hello_waiting_for_kept_output_is_answered_when_the_program_goes() {
        let (host, place) = host("teller-gone", false);
        let teller = teller(&host, &place);
        let shell = Shell::connect(&place.socket());
        shell.send(&json!({ "jsonrpc": "2.0", "id": 0, "method": "hello" }));
        let asked: Value = serde_json::from_str(&read(&teller)).unwrap();
        assert_eq!(asked["method"], "listen", "{asked}");
        host.reaches(|session| !session.greetings.is_empty());
        drop(teller);
        assert_eq!(shell.answer(0)["result"]["output"], json!([]));
        drop(host);
    }

    /// A program that writes to the terminal itself tells only what the
    /// engine prints, and `hello` says so (#2411).
    #[test]
    fn hello_says_a_run_at_a_terminal_tells_the_engine_s_output() {
        let (host, place) = host("coverage-engine", false);
        let program = listened(teller(&host, &place), "kept\n");
        let shell = Shell::connect(&place.socket());
        assert_eq!(greeted(&shell)["coverage"], "engine");
        drop((program.join().unwrap(), host));
    }

    /// A shell that started the run reads its streams off pipes, so
    /// `hello` says the session tells all of its output (#2411).
    #[test]
    fn hello_says_a_run_a_shell_started_tells_all_its_output() {
        let (host, place) = host("coverage-all", true);
        let shell = Shell::connect(&place.socket());
        assert_eq!(greeted(&shell)["coverage"], "all");
        drop(host);
    }

    /// A program that says `tell` after the first `hello` is asked at once,
    /// and what it kept reaches that shell as output after the answer.
    #[test]
    fn a_program_that_tells_after_the_first_hello_tells_what_it_kept_next() {
        let (host, place) = host("teller-late", false);
        let shell = Shell::connect(&place.socket());
        greeted(&shell);
        let program = listened(teller(&host, &place), "kept\n");
        assert_eq!(shell.next()["params"]["text"], "kept\n");
        drop((program.join().unwrap(), host));
    }

    #[test]
    fn two_shells_hear_every_event_each_through_its_own_cursor() {
        let (host, place) = host("cursors", false);
        let first = Shell::connect(&place.socket());
        assert_eq!(first.hello(), Vec::<Value>::new());
        host.record(json!({ "event": "running" }));
        assert_eq!(
            first.next()["params"],
            json!({ "event": "running", "seq": 1 })
        );

        // A shell that attaches later hears what happened so far, then
        // what happens next, numbered on.
        let second = Shell::connect(&place.socket());
        assert_eq!(second.hello(), [json!({ "event": "running", "seq": 1 })]);
        host.record(json!({ "event": "swapped" }));
        for shell in [&first, &second] {
            assert_eq!(
                shell.next()["params"],
                json!({ "event": "swapped", "seq": 2 })
            );
        }
    }

    /// A shell that closes while its control request holds the program
    /// ends that request: its exchange with the inspector shuts, and the
    /// turn passes to the next shell's control request.
    #[test]
    fn a_shell_that_closes_mid_request_passes_the_turn_on() {
        let (host, place) = host("close-turn", true);
        host.record(json!({ "event": "running" }));
        let inspector = UnixListener::bind(place.inspector()).unwrap();
        let _told = serve(&place);
        let leaving = Shell::connect(&place.socket());
        leaving.hello();
        leaving.send(&json!({
            "jsonrpc": "2.0", "id": 1, "method": "program", "params": { "op": "step" }
        }));
        let (held, _) = inspector.accept().unwrap();
        let mut reader = BufReader::new(&held);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        leaving.close();
        // The program sees its exchange end.
        line.clear();
        assert_eq!(reader.read_line(&mut line).unwrap(), 0);
        let staying = Shell::connect(&place.socket());
        staying.hello();
        answered(
            &staying,
            &inspector,
            2,
            &json!({ "op": "break", "line": 3 }),
        );
        drop(host);
    }

    /// A request waiting for its program to start ends with its cancel,
    /// and one waiting when its shell closes ends too, leaving the turn
    /// free.
    #[test]
    fn a_request_waiting_for_its_program_ends_at_a_cancel_or_a_close() {
        let (host, place) = host("close-parked", true);
        let shell = Shell::connect(&place.socket());
        shell.hello();
        shell.send(&json!({
            "jsonrpc": "2.0", "id": 1, "method": "program", "params": { "op": "step" }
        }));
        host.reaches(|session| {
            session
                .requests
                .first()
                .is_some_and(|request| request.parked.is_some())
        });
        shell.send(&json!({ "jsonrpc": "2.0", "method": "cancel", "params": { "id": 1 } }));
        assert_eq!(shell.answer(1)["error"]["code"], CANCELLED);
        shell.send(&json!({
            "jsonrpc": "2.0", "id": 2, "method": "program", "params": { "op": "step" }
        }));
        host.reaches(|session| {
            session
                .requests
                .iter()
                .any(|request| request.ticket.number == 2 && request.parked.is_some())
        });
        shell.close();
        host.reaches(|session| session.requests.is_empty() && session.turn.is_none());
        drop(host);
    }

    /// A request whose program ends while it holds it, with no program to
    /// follow, ends as not running.
    #[test]
    fn a_request_whose_program_ends_is_answered_not_running() {
        let (host, place) = host("ended", true);
        host.record(json!({ "event": "running" }));
        let inspector = UnixListener::bind(place.inspector()).unwrap();
        let told = serve(&place);
        let shell = Shell::connect(&place.socket());
        shell.hello();
        shell.send(&json!({
            "jsonrpc": "2.0", "id": 1, "method": "program", "params": { "name": "count" }
        }));
        let (held, _) = inspector.accept().unwrap();
        read(&held);
        host.record(json!({ "event": "ended", "status": 0 }));
        drop(told);
        drop(held);
        let answer = shell.answer(1);
        assert_eq!(answer["error"]["code"], NOT_RUNNING, "{answer}");
        assert_eq!(answer["error"]["data"]["state"], "ended", "{answer}");
        drop(host);
    }

    #[test]
    fn a_pause_one_shell_asks_for_reaches_the_other_and_a_cancel_ends_only_its_request() {
        let (host, place) = host("pause", true);
        host.record(json!({ "event": "running" }));
        let inspector = UnixListener::bind(place.inspector()).unwrap();
        // The program's inspector tells the session where it paused, as
        // `rawr::inspect` does.
        let told = serve(&place);
        let (holding, held) = mpsc::channel();
        let program = std::thread::spawn(move || {
            let (step, _) = inspector.accept().unwrap();
            read(&step);
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
