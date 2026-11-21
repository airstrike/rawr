//! The programs the server reaches, each through its run's session
//! ([`crate::events::session`]): one `rawr run` per project root that
//! `start` runs, in a process group of its own with a stdin pipe and its
//! output kept with a cursor (D537.3), or a run started elsewhere, as in a
//! terminal, which a tool attaches to (D1053.2). The owner keeps watching
//! and reloading, as at a terminal; the server keeps its own copy of what
//! the session tells, numbered as the session numbers it.
//!
//! One thread per session owns that copy and the requests waiting for
//! answers. The connection's reader, the run's output readers, the thread
//! that reaps the run, and every caller send to its inbox. A caller's
//! request or wait names an [`Ear`] on the caller's own channel, and the
//! owner tells it there once the answer comes, what it waited for
//! happens, or the session is over. Each wait is checked after every
//! message, so none waits on something already true. `rawr mcp`'s calls
//! block on their call's channel, which their cancel also wakes; `rawr
//! dap`'s adapter hears on its own inbox and blocks on nothing.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};

use serde_json::{json, Value};

use super::call::{Call, Wake};
use super::options::Options;
use super::process;
use crate::events::session;

pub use crate::events::session::Phase;

/// Output kept per session, oldest first out.
const OUTPUT: usize = 256 * 1024;

/// The number of the next request or wait, across sessions, so an answer
/// meant for a wait of one session never passes for another's. Requests
/// use it on the wire too: the session's `hello` is 0.
static NEXT: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stream {
    /// The program's stdout.
    Stdout,
    /// The program's stderr.
    Stderr,
    /// Rawr's own lines: the stderr of a `rawr run` the server started,
    /// whose program's stderr comes through the session instead (#2367).
    Rawr,
}

impl Stream {
    /// The stream other shells hear the piece on, as a terminal shows it:
    /// rawr's own lines on stderr.
    const fn name(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr | Self::Rawr => "stderr",
        }
    }
}

/// What the server knows of a session: the program as the session's
/// events tell it, and what the server gave its client.
#[derive(Debug, Default)]
struct Mirror {
    session: session::State,
    /// The pid of `rawr run`.
    pid: u32,
    /// The session answered `hello`, or its connection closed first.
    greeted: bool,
    /// The connection to the session closed.
    closed: bool,
    /// Whether `rawr run` has exited, for one `start` ran.
    exited: bool,
    /// The output pipes of a `rawr run` the server started that are still
    /// open.
    pipes: usize,
    output: VecDeque<(u64, Stream, String)>,
    output_bytes: usize,
    next_output: u64,
    /// The number of the newest piece of the program's own output, which
    /// ends a [`Wait::Next`]; rawr's lines don't (#2267).
    program_output: u64,
    /// What the last answer returned: a call without a cursor goes on
    /// from here.
    given_event: u64,
    given_output: u64,
    /// The newest `Next` wait's number: a wait whose number is older was
    /// given up by its client, which waits once at a time, and answers
    /// nothing.
    waiter: u64,
    /// The newest breakpoints the session told, as its `breakpoints` event
    /// tells them (#2278).
    breakpoints: Value,
    /// How much of the program's output the session tells, as `hello`
    /// said: `None` before any reached it.
    coverage: Option<Coverage>,
}

/// How much of a program's output its session tells (#2411).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coverage {
    /// The program writes to the terminal itself and tells what the engine
    /// prints: a dependency's own writes reach only the terminal.
    Engine,
    /// A shell or the run's owner reads the program's streams off pipes.
    All,
}

impl Coverage {
    /// The coverage `hello` names, `engine` or `all`.
    fn of(told: &Value) -> Option<Self> {
        match told.as_str()? {
            "engine" => Some(Self::Engine),
            "all" => Some(Self::All),
            _ => None,
        }
    }

    /// The name `hello` gives it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Engine => "engine",
            Self::All => "all",
        }
    }
}

/// What a shell says of a run at a terminal, whose session hears only
/// what the engine prints.
pub const AT_TERMINAL: &str =
    "this run was started at a terminal: shells hear rawr's prints, not a dependency's own writes";

/// Why a request to the program has no answer.
#[derive(Debug)]
pub enum Refusal {
    /// No program runs to answer it.
    NotRunning,
    /// The program refused it.
    Error(String),
    Cancelled,
}

/// How far the session's events had come when a request was sent. What
/// its answer says of the program is older than any event that came after
/// that, whether the event reached the server before the answer or after
/// it: the program answers the request, then pauses or goes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seen(u64);

/// What a session tells one of its waiters.
#[derive(Debug)]
pub enum Told {
    /// The answer to a request, and how far the events had come when it
    /// was sent.
    Answer(Result<(Value, Seen), Refusal>),
    /// A build's stage and percent, while a wait for the program waits.
    Building { stage: String, percent: u64 },
    /// What the wait waited for came, or the session is over.
    Done,
    /// A [`Wait::Next`] ended: the cursor it goes on from, or `None` once a
    /// later wait took its place.
    Next(Option<u64>),
}

/// What a waiter waits for. Each ends too when the session is over, but
/// for [`Wait::Exit`], which outlasts a stop.
#[derive(Clone, Copy, Debug)]
pub enum Wait {
    /// The session's greeting, or its close.
    Greeting,
    /// The program runs or fails.
    Settled,
    /// The next event past `cursor` other than build progress, or the
    /// program's output past `output` (#2267), each by default past what
    /// was last given. A client waits once at a time, so a wait that comes
    /// while another is still parked means the client gave the first up
    /// (#1542): that one ends with no cursor.
    Next {
        cursor: Option<u64>,
        output: Option<u64>,
    },
    /// An event past `event`, output past `output` or breakpoints past
    /// revision `breakpoints`: for a shell that tells each as it comes.
    Change {
        event: u64,
        output: u64,
        breakpoints: u64,
    },
    /// The exit of the `rawr run` the server started, at once for a run
    /// started elsewhere.
    Exit,
}

/// Where a session tells a waiter: the waiter's own channel. A tell fails
/// once the waiter is gone, and the session forgets the wait.
pub struct Ear(Box<dyn Fn(u64, Told) -> bool + Send>);

impl std::fmt::Debug for Ear {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Ear")
    }
}

impl Ear {
    /// An ear that hands each wait's number and what it is told to `tell`,
    /// which says whether the waiter took it.
    pub fn new(tell: impl Fn(u64, Told) -> bool + Send + 'static) -> Self {
        Self(Box::new(tell))
    }

    fn tell(&self, id: u64, told: Told) -> bool {
        (self.0)(id, told)
    }
}

/// A `rawr run` the server started.
#[derive(Debug)]
struct Started {
    /// The process group it leads.
    group: u32,
    /// Where its session is, which the server removes once the run is
    /// over: a run a signal ended leaves it behind.
    place: session::Place,
}

/// What a session's owner hears.
enum Inbox {
    /// A message the session sent.
    Message(Value),
    /// The connection to the session closed.
    Closed,
    /// A piece of the output of the `rawr run` the server started.
    Output { stream: Stream, text: String },
    /// One of that run's output pipes closed.
    PipeClosed,
    /// That run exited, with this status.
    Exited(Option<i32>),
    /// A look at the mirror, or a change to it.
    Look(Box<dyn FnOnce(&mut Mirror) + Send>),
    /// A request for the program, answered to `ear` as `id`.
    Ask { id: u64, request: Value, ear: Ear },
    /// A wait, told to `ear` as `id`.
    Park { id: u64, wait: Wait, ear: Ear },
    /// The waiter of `id` gave up: a request, which the session is told to
    /// cancel, or a wait.
    Forget(u64),
    /// End the session.
    Stop,
}

/// How a shell picks another of a program's runs than the newest.
const PICK: &str = "`rawr tui --pid <pid>` and `rawr gui --pid <pid>` reach another, and so does `rawr dap`'s `attach` with its `processId`.";

/// One program the server reaches: a handle on its owner thread.
#[derive(Debug)]
pub struct Session {
    /// The project root, the session's key.
    pub root: PathBuf,
    /// The path given to `rawr run`.
    pub path: PathBuf,
    /// The program's arguments.
    pub arguments: Vec<String>,
    /// Whether the server started the run, rather than one started
    /// elsewhere.
    started: bool,
    /// What the server tells of the program's other runs, when it attached
    /// to the newest of several.
    others: Option<String>,
    inbox: mpsc::Sender<Inbox>,
}

/// The thread that owns a session's mirror.
struct Owner {
    mirror: Mirror,
    /// The run `start` started; `None` for a run started elsewhere.
    started: Option<Started>,
    /// The connection, which a stop shuts for a run started elsewhere.
    stream: UnixStream,
    /// The connection's writer thread: requests in order, so output the
    /// server tells never waits on the session.
    out: mpsc::Sender<String>,
    /// The requests waiting for their answers, each with how far the
    /// events had come when it was sent; `None` once the connection closed.
    pending: Option<HashMap<u64, (Ear, Seen)>>,
    parked: Vec<Parked>,
    /// The status of the `rawr run` the server started, once it was reaped.
    status: Option<Option<i32>>,
}

/// A wait the owner holds until it ends.
struct Parked {
    id: u64,
    until: Until,
    ear: Ear,
    /// The build update last told.
    heard: Option<(String, u64)>,
}

/// A [`Wait`] as the owner holds it.
enum Until {
    Greeting,
    Settled,
    Next {
        after: u64,
        output: u64,
        ticket: u64,
    },
    Change {
        event: u64,
        output: u64,
        breakpoints: u64,
    },
    Exit,
}

/// How the program of a run [`Session::spawn`] starts goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    /// It runs at once.
    Run,
    /// It waits at its first statement until the shell sends `release`,
    /// once its breakpoints are in (#2274).
    Hold,
}

/// What one answer about a session holds.
#[derive(Debug)]
pub struct Answer {
    /// The data.
    pub structured: Value,
    /// The text form.
    pub summary: String,
}

impl Session {
    /// Start `rawr run <path> <feature flags> -- arguments` for `root` in
    /// `options`' environment, its program going as `entry` says and
    /// reading `stdin`. The run keeps the options for its reloads: its
    /// process selects the flags for every Cargo answer it asks and hands
    /// them to the program's process, which also inherits the environment.
    pub fn spawn(
        root: &Path,
        path: &Path,
        options: &Options,
        arguments: &[String],
        stdin: Option<&str>,
        entry: Entry,
    ) -> std::io::Result<Arc<Self>> {
        let place = session::Place::new(root)?;
        place.make()?;
        let socket = place.socket();
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket)?;
        let mut command = Command::new(std::env::current_exe()?);
        command.arg("run").arg(path);
        options.apply(&mut command);
        command
            // A host that takes no arguments refuses even an empty `--`.
            .args(if arguments.is_empty() {
                None
            } else {
                Some("--")
            })
            .args(arguments)
            .current_dir(if root.is_dir() {
                root
            } else {
                root.parent().unwrap_or(root)
            })
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env(crate::events::PROGRAM_STDERR, "1");
        session::hand(&mut command, &listener);
        if entry == Entry::Hold {
            command.env(crate::events::HOLD, "1");
        }
        let mut child = process::spawn(&mut command)?;
        drop(listener);
        // The kernel holds the connection until the run accepts it.
        let session = Arc::new(Self::open(
            root,
            path,
            arguments,
            Some(Started {
                group: child.id(),
                place,
            }),
            UnixStream::connect(&socket)?,
        )?);
        // The program reads what the agent gave, then end of input.
        if let Some(mut pipe) = child.stdin.take() {
            let text = stdin.unwrap_or_default().to_owned();
            let _ = std::thread::Builder::new()
                .name("rawr-mcp-stdin".into())
                .spawn(move || {
                    let _ = pipe.write_all(text.as_bytes());
                });
        }
        for (pipe, stream) in [
            (
                child
                    .stdout
                    .take()
                    .map(|pipe| Box::new(pipe) as Box<dyn std::io::Read + Send>),
                Stream::Stdout,
            ),
            (
                child
                    .stderr
                    .take()
                    .map(|pipe| Box::new(pipe) as Box<dyn std::io::Read + Send>),
                Stream::Rawr,
            ),
        ] {
            if let Some(pipe) = pipe {
                // Counted before the reader starts, so its close comes after.
                session.change(|mirror| mirror.pipes += 1);
                let inbox = session.inbox.clone();
                let spawned = std::thread::Builder::new()
                    .name("rawr-mcp-session-output".into())
                    .spawn(move || read_output(pipe, stream, &inbox));
                if spawned.is_err() {
                    session.change(|mirror| mirror.pipes -= 1);
                }
            }
        }
        let inbox = session.inbox.clone();
        let reaping = std::thread::Builder::new()
            .name("rawr-mcp-session-exit".into())
            .spawn(move || {
                let status = child.wait().ok().map(process::code);
                let _ = inbox.send(Inbox::Exited(status));
            });
        if reaping.is_err() {
            // The child went with the closure: the run's waits still end.
            let _ = session.inbox.send(Inbox::Exited(None));
        }
        Ok(session)
    }

    /// A session reached on `stream`, which it greets: of the `rawr run`
    /// the server `started`, or of a run started elsewhere.
    fn open(
        root: &Path,
        path: &Path,
        arguments: &[String],
        started: Option<Started>,
        stream: UnixStream,
    ) -> std::io::Result<Self> {
        let reading = stream.try_clone()?;
        let mut writing = stream.try_clone()?;
        let (out, lines) = mpsc::channel::<String>();
        std::thread::Builder::new()
            .name("rawr-mcp-session-write".into())
            .spawn(move || {
                for line in lines {
                    if writing.write_all(line.as_bytes()).is_err() {
                        return;
                    }
                }
            })?;
        // The session's first answer is to `hello`: what happened so far.
        let hello = json!({ "jsonrpc": "2.0", "id": 0, "method": "hello" });
        let _ = out.send(format!("{hello}\n"));
        let is_started = started.is_some();
        let owner = Owner {
            mirror: Mirror {
                pid: started.as_ref().map_or(0, |started| started.group),
                ..Mirror::default()
            },
            started,
            stream,
            out,
            pending: Some(HashMap::new()),
            parked: Vec::new(),
            status: None,
        };
        let (inbox, messages) = mpsc::channel();
        std::thread::Builder::new()
            .name("rawr-mcp-session".into())
            .spawn(move || owner.run(&messages))?;
        let listening = inbox.clone();
        std::thread::Builder::new()
            .name("rawr-mcp-session-read".into())
            .spawn(move || listen(reading, &listening))?;
        Ok(Self {
            root: root.to_path_buf(),
            path: path.to_path_buf(),
            arguments: arguments.to_vec(),
            started: is_started,
            others: None,
            inbox,
        })
    }

    /// The session of a run started elsewhere that serves `root`, the one
    /// whose pid is `pid` or the newest, not yet greeted: `None` when no
    /// such run serves it there. Its greeting ends a [`Wait::Greeting`],
    /// and [`Self::reached`] then says whether it came.
    #[must_use]
    pub fn reach(root: &Path, pid: Option<u32>) -> Option<Arc<Self>> {
        let reached = session::Runs::of(root).ok()?.connect(pid)?;
        let others = reached.notice(PICK);
        let mut session = Self::open(root, root, &[], None, reached.stream).ok()?;
        session.others = others;
        Some(Arc::new(session))
    }

    /// What the server tells of the program's other runs, when it attached
    /// to the newest of several.
    #[must_use]
    pub fn others(&self) -> Option<&str> {
        self.others.as_deref()
    }

    /// The session of a run started elsewhere that serves `root`, once it
    /// greeted the server: `None` when no run serves it there, or `call`
    /// was cancelled first.
    pub fn attach(root: &Path, call: &Call) -> Option<Arc<Self>> {
        let session = Self::reach(root, None)?;
        let greeted = session.wait_for(call, Wait::Greeting, |told| {
            matches!(told, Told::Done).then_some(())
        });
        if greeted.is_none() || !session.reached() {
            session.stop();
            return None;
        }
        Some(session)
    }

    /// A session of a run started elsewhere, not yet greeted, with no
    /// process of its own, on one end of a pair whose other end is
    /// returned: it greets once that end answers `hello`.
    #[cfg(test)]
    pub fn greeting() -> (Arc<Self>, UnixStream) {
        let (stream, other) = UnixStream::pair().unwrap();
        let session =
            Arc::new(Self::open(Path::new("/p"), Path::new("/p"), &[], None, stream).unwrap());
        (session, other)
    }

    /// A session already in `phase`, with no process of its own, on one
    /// end of a pair whose other end is returned.
    #[cfg(test)]
    pub fn running_at(phase: Phase) -> (Arc<Self>, UnixStream) {
        let (stream, other) = UnixStream::pair().unwrap();
        let session =
            Arc::new(Self::open(Path::new("/p"), Path::new("/p"), &[], None, stream).unwrap());
        session.change(move |mirror| {
            mirror.greeted = true;
            match phase {
                Phase::Running => mirror.session.record(1, json!({ "event": "running" })),
                Phase::Starting => {}
                other => mirror.session.end(other),
            }
        });
        (session, other)
    }

    /// Read `text` as the stderr of the `rawr run` the server started:
    /// rawr's own lines.
    #[cfg(test)]
    pub fn read_rawr(&self, text: &'static [u8]) {
        self.change(|mirror| mirror.pipes += 1);
        read_output(Box::new(text), Stream::Rawr, &self.inbox);
    }

    /// Change the mirror on the owner's thread.
    fn change(&self, change: impl FnOnce(&mut Mirror) + Send + 'static) {
        let _ = self.inbox.send(Inbox::Look(Box::new(change)));
    }

    /// What `look` reads of the mirror on the owner's thread. The owner
    /// reads its inbox while this handle holds a sender, so it is `None`
    /// only if the owner panicked.
    fn look<T: Send + 'static>(
        &self,
        look: impl FnOnce(&mut Mirror) -> T + Send + 'static,
    ) -> Option<T> {
        let (reply, answer) = mpsc::channel();
        self.change(move |mirror| {
            let _ = reply.send(look(mirror));
        });
        answer.recv().ok()
    }

    /// The current phase.
    #[must_use]
    pub fn phase(&self) -> Phase {
        self.look(|mirror| mirror.session.phase().clone())
            .unwrap_or(Phase::Exited(None))
    }

    /// The pid of `rawr run`.
    #[must_use]
    pub fn pid(&self) -> u32 {
        self.look(|mirror| mirror.pid).unwrap_or(0)
    }

    /// Whether the server started this session's run.
    #[must_use]
    pub const fn is_started(&self) -> bool {
        self.started
    }

    /// Whether the session greeted the server before its connection
    /// closed.
    #[must_use]
    pub fn reached(&self) -> bool {
        self.look(|mirror| mirror.greeted && !mirror.closed)
            .unwrap_or(false)
    }

    /// Where the program is paused, if it is.
    #[must_use]
    pub fn paused(&self) -> Option<Value> {
        self.look(|mirror| mirror.session.paused().cloned())
            .flatten()
    }

    /// The program answered a debugger tool asked as of `seen`: paused at
    /// `paused`, or running. Its own `paused` and `resumed` events say the
    /// same once they come, so the answer holds only while no event came
    /// since the request was sent, as `status` and `wait` read it before
    /// those events arrive (#535).
    pub fn observe_pause(&self, paused: Option<Value>, seen: Seen) {
        self.change(move |mirror| {
            if mirror.seen() == seen {
                mirror.session.observe_pause(paused);
            }
        });
    }

    /// How much of the program's output the session tells, as its `hello`
    /// said.
    #[must_use]
    pub fn coverage(&self) -> Option<Coverage> {
        self.look(|mirror| mirror.coverage).flatten()
    }

    /// The newest breakpoints the session told: `{"event": "breakpoints",
    /// "revision", "breakpoints": [..]}`, or null before `hello`.
    #[must_use]
    pub fn breakpoints(&self) -> Value {
        self.look(|mirror| mirror.breakpoints.clone())
            .unwrap_or(Value::Null)
    }

    /// The session now: its phase, the events past `cursor` and the output
    /// past `output_cursor` (each defaulting to what was last given), which
    /// then count as given.
    #[must_use]
    pub fn answer(&self, cursor: Option<u64>, output_cursor: Option<u64>) -> Answer {
        let mut structured = self
            .look(move |mirror| mirror.answer(cursor, output_cursor))
            .unwrap_or_else(|| Mirror::default().answer(cursor, output_cursor));
        structured["root"] = json!(self.root);
        let summary = summary(&structured);
        Answer {
            structured,
            summary,
        }
    }

    /// End the session: the group the server started, SIGTERM and then
    /// SIGKILL after a grace, or the connection to a run started
    /// elsewhere, which goes on.
    pub fn stop(&self) {
        let _ = self.inbox.send(Inbox::Stop);
    }

    /// Ask the program `request` through the session, which waits for the
    /// program that runs next while it starts or restarts. The answer goes
    /// to `ear` under the number returned.
    pub fn request(&self, request: Value, ear: Ear) -> u64 {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let _ = self.inbox.send(Inbox::Ask { id, request, ear });
        id
    }

    /// Wait for `wait`, told to `ear` under the number returned.
    pub fn park(&self, wait: Wait, ear: Ear) -> u64 {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let _ = self.inbox.send(Inbox::Park { id, wait, ear });
        id
    }

    /// Give up the request or wait `id`.
    pub fn forget(&self, id: u64) {
        let _ = self.inbox.send(Inbox::Forget(id));
    }

    /// [`Self::request`] for a call, which waits for the answer or its
    /// cancel.
    pub fn ask(&self, request: &Value, call: &Call) -> Result<(Value, Seen), Refusal> {
        if call.is_cancelled() {
            return Err(Refusal::Cancelled);
        }
        let id = self.request(request.clone(), ear(call));
        let answer = call.wait(|wake| match wake {
            Wake::Session {
                id: asked,
                told: Told::Answer(answer),
            } if asked == id => Some(answer),
            _ => None,
        });
        answer.unwrap_or_else(|| {
            self.forget(id);
            Err(Refusal::Cancelled)
        })
    }

    /// Wait for `wait` on behalf of `call`, until `heard` takes what the
    /// session tells or the call is cancelled.
    fn wait_for<T>(
        &self,
        call: &Call,
        wait: Wait,
        mut heard: impl FnMut(Told) -> Option<T>,
    ) -> Option<T> {
        if call.is_cancelled() {
            return None;
        }
        let id = self.park(wait, ear(call));
        let taken = call.wait(|wake| match wake {
            Wake::Session { id: waited, told } if waited == id => heard(told),
            _ => None,
        });
        if taken.is_none() {
            self.forget(id);
        }
        taken
    }

    /// Wait until the program runs or fails, or `call` is cancelled;
    /// `building` hears each build update.
    pub fn settle(&self, call: &Call, mut building: impl FnMut(&str, u64)) {
        self.wait_for(call, Wait::Settled, |told| match told {
            Told::Building { stage, percent } => {
                building(&stage, percent);
                None
            }
            Told::Done => Some(()),
            _ => None,
        });
    }

    /// Wait for the next event past `cursor` other than build progress, the
    /// program's output past `output`, or `call`'s cancel: the cursor the
    /// wait went on from, or `None` once a later wait took its place (see
    /// [`Wait::Next`]). `building` hears each build update.
    pub fn next(
        &self,
        cursor: Option<u64>,
        output: Option<u64>,
        call: &Call,
        mut building: impl FnMut(&str, u64),
    ) -> Option<u64> {
        self.wait_for(call, Wait::Next { cursor, output }, |told| match told {
            Told::Building { stage, percent } => {
                building(&stage, percent);
                None
            }
            Told::Next(after) => Some(after),
            _ => None,
        })
        .flatten()
    }

    /// Wait for `rawr run` to exit, after [`Self::stop`] ended the group
    /// the server started, or for `call`'s cancel.
    pub fn wait_exit(&self, call: &Call) {
        if !self.started {
            return;
        }
        self.wait_for(call, Wait::Exit, |told| {
            matches!(told, Told::Done).then_some(())
        });
    }
}

/// An ear on `call`'s channel.
fn ear(call: &Call) -> Ear {
    let waker = call.waker();
    Ear::new(move |id, told| waker.wake(Wake::Session { id, told }))
}

/// Read the session's messages until its connection closes.
fn listen(stream: UnixStream, inbox: &mpsc::Sender<Inbox>) {
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else {
            break;
        };
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if inbox.send(Inbox::Message(message)).is_err() {
            return;
        }
    }
    let _ = inbox.send(Inbox::Closed);
}

/// Read one of `rawr run`'s output pipes until it closes.
fn read_output(
    mut pipe: Box<dyn std::io::Read + Send>,
    stream: Stream,
    inbox: &mpsc::Sender<Inbox>,
) {
    let mut buffer = [0_u8; 8192];
    while let Ok(read @ 1..) = pipe.read(&mut buffer) {
        let text = String::from_utf8_lossy(&buffer[..read]).into_owned();
        if inbox.send(Inbox::Output { stream, text }).is_err() {
            return;
        }
    }
    let _ = inbox.send(Inbox::PipeClosed);
}

impl Owner {
    /// Take each message, then end the waits it ended, until every sender
    /// is gone.
    fn run(mut self, inbox: &mpsc::Receiver<Inbox>) {
        for message in inbox {
            self.take(message);
            self.reap();
            self.wake();
        }
    }

    fn send(&self, message: &Value) {
        let _ = self.out.send(format!("{message}\n"));
    }

    fn take(&mut self, message: Inbox) {
        match message {
            Inbox::Message(message) => self.heard(message),
            Inbox::Closed => self.close(),
            Inbox::Output { stream, text } => {
                // The other shells attached to the session hear it too.
                self.send(&json!({
                    "jsonrpc": "2.0",
                    "method": "output",
                    "params": { "stream": stream.name(), "text": text },
                }));
                self.mirror.keep(stream, text);
            }
            Inbox::PipeClosed => self.mirror.pipes -= 1,
            Inbox::Exited(status) => self.status = Some(status),
            Inbox::Look(look) => look(&mut self.mirror),
            Inbox::Ask { id, request, ear } => match &mut self.pending {
                Some(pending) => {
                    pending.insert(id, (ear, self.mirror.seen()));
                    self.send(&json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "method": "program",
                        "params": request,
                    }));
                }
                None => {
                    ear.tell(id, Told::Answer(Err(Refusal::NotRunning)));
                }
            },
            Inbox::Park { id, wait, ear } => {
                let until = match wait {
                    Wait::Greeting => Until::Greeting,
                    Wait::Settled => Until::Settled,
                    Wait::Next { cursor, output } => {
                        self.mirror.waiter += 1;
                        Until::Next {
                            after: cursor.unwrap_or(self.mirror.given_event),
                            output: output.unwrap_or(self.mirror.given_output),
                            ticket: self.mirror.waiter,
                        }
                    }
                    Wait::Change {
                        event,
                        output,
                        breakpoints,
                    } => Until::Change {
                        event,
                        output,
                        breakpoints,
                    },
                    Wait::Exit => Until::Exit,
                };
                self.parked.push(Parked {
                    id,
                    until,
                    ear,
                    heard: None,
                });
            }
            Inbox::Forget(id) => {
                if let Some(pending) = &mut self.pending
                    && pending.remove(&id).is_some()
                {
                    self.send(
                        &json!({ "jsonrpc": "2.0", "method": "cancel", "params": { "id": id } }),
                    );
                }
                self.parked.retain(|parked| parked.id != id);
            }
            Inbox::Stop => self.stop(),
        }
    }

    /// A message the session sent: an answer, an event or output.
    fn heard(&mut self, mut message: Value) {
        match (message["id"].as_u64(), message["method"].as_str()) {
            (Some(0), None) => self.greeted(&message["result"]),
            (Some(id), None) => {
                if let Some((ear, seen)) = self
                    .pending
                    .as_mut()
                    .and_then(|pending| pending.remove(&id))
                {
                    let answer = match message.get_mut("result") {
                        Some(result) => Ok((result.take(), seen)),
                        None => Err(refusal(&message["error"])),
                    };
                    ear.tell(id, Told::Answer(answer));
                }
            }
            (None, Some("event")) => {
                let event = message["params"].take();
                if let Some(seq) = event["seq"].as_u64() {
                    self.mirror.session.record(seq, event);
                } else if event["event"] == "breakpoints" {
                    self.mirror.take_breakpoints(event);
                }
            }
            (None, Some("output")) => {
                let params = &message["params"];
                let stream = match params["stream"].as_str() {
                    Some("stderr") => Stream::Stderr,
                    _ => Stream::Stdout,
                };
                if let Some(text) = params["text"].as_str() {
                    self.mirror.keep(stream, text.to_owned());
                }
            }
            _ => {}
        }
    }

    /// The session's answer to `hello`: the events so far, and for a run
    /// started elsewhere, its pid and the output it kept.
    fn greeted(&mut self, result: &Value) {
        let mirror = &mut self.mirror;
        for event in result["events"].as_array().into_iter().flatten() {
            if let Some(seq) = event["seq"].as_u64() {
                mirror.session.record(seq, event.clone());
            }
        }
        mirror.take_breakpoints(result["breakpoints"].clone());
        mirror.coverage = Coverage::of(&result["coverage"]);
        if self.started.is_none() {
            mirror.pid = result["pid"]
                .as_u64()
                .and_then(|pid| u32::try_from(pid).ok())
                .unwrap_or(0);
            for piece in result["output"].as_array().into_iter().flatten() {
                let stream = match piece["stream"].as_str() {
                    Some("stderr") => Stream::Stderr,
                    _ => Stream::Stdout,
                };
                if let Some(text) = piece["text"].as_str() {
                    mirror.keep(stream, text.to_owned());
                }
            }
        }
        mirror.greeted = true;
    }

    /// The connection closed: no request waiting gets an answer.
    fn close(&mut self) {
        for (id, (ear, _)) in self.pending.take().into_iter().flatten() {
            ear.tell(id, Told::Answer(Err(Refusal::NotRunning)));
        }
        let mirror = &mut self.mirror;
        mirror.closed = true;
        mirror.greeted = true;
        // A run started elsewhere ended: its last `ended` says how.
        if self.started.is_none() {
            let status = match mirror.session.phase() {
                Phase::Ended(status) => Some(*status),
                _ => None,
            };
            mirror.session.end(Phase::Exited(status));
        }
    }

    /// Once the `rawr run` the server started exited, its session said its
    /// last words and its output ended, record the exit. The run's place
    /// goes first, so a `start` that waited for the exit binds a new one.
    fn reap(&mut self) {
        let mirror = &mut self.mirror;
        let (Some(started), Some(status)) = (&self.started, self.status) else {
            return;
        };
        if mirror.exited || !mirror.closed || mirror.pipes > 0 {
            return;
        }
        started.place.remove();
        mirror.session.end(Phase::Exited(status));
        mirror.exited = true;
    }

    fn stop(&mut self) {
        if self.mirror.session.phase().is_over() {
            return;
        }
        self.mirror.session.end(Phase::Stopped);
        match &self.started {
            Some(started) => process::end_group(started.group),
            None => {
                let _ = self.stream.shutdown(std::net::Shutdown::Both);
            }
        }
    }

    /// Tell each wait the build updates it hasn't heard, and end those
    /// whose ends came. A wait whose waiter is gone goes.
    fn wake(&mut self) {
        let mirror = &self.mirror;
        let started = self.started.is_some();
        self.parked.retain_mut(|parked| {
            if matches!(parked.until, Until::Settled | Until::Next { .. })
                && let Some((stage, percent)) = mirror.session.building()
                && parked
                    .heard
                    .as_ref()
                    .is_none_or(|(said, then)| said != stage || *then != percent)
            {
                let told = Told::Building {
                    stage: stage.to_owned(),
                    percent,
                };
                if !parked.ear.tell(parked.id, told) {
                    return false;
                }
                parked.heard = Some((stage.to_owned(), percent));
            }
            match mirror.ended(&parked.until, started) {
                Some(told) => {
                    parked.ear.tell(parked.id, told);
                    false
                }
                None => true,
            }
        });
    }
}

/// A session's error as a refusal.
fn refusal(error: &Value) -> Refusal {
    if error["code"] == session::NOT_RUNNING {
        Refusal::NotRunning
    } else if error["code"] == session::CANCELLED {
        Refusal::Cancelled
    } else {
        Refusal::Error(error["message"].as_str().unwrap_or_default().to_owned())
    }
}

impl Mirror {
    /// How far the events have come.
    fn seen(&self) -> Seen {
        Seen(self.session.next_number())
    }

    /// Keep a piece of output, dropping the oldest past [`OUTPUT`].
    fn keep(&mut self, stream: Stream, text: String) {
        self.next_output += 1;
        let number = self.next_output;
        self.output_bytes += text.len();
        if stream != Stream::Rawr {
            self.program_output = number;
        }
        self.output.push_back((number, stream, text));
        while self.output_bytes > OUTPUT {
            let Some((_, _, dropped)) = self.output.pop_front() else {
                break;
            };
            self.output_bytes -= dropped.len();
        }
    }

    /// Take `told` breakpoints when they are newer than those kept: the
    /// session may tell a list `hello` already gave.
    fn take_breakpoints(&mut self, told: Value) {
        let revision = |breakpoints: &Value| breakpoints["revision"].as_u64();
        if revision(&told) > revision(&self.breakpoints) {
            self.breakpoints = told;
        }
    }

    /// What ends a wait for `until` now, if it ends: of a session whose run
    /// the server `started` or not.
    fn ended(&self, until: &Until, started: bool) -> Option<Told> {
        let phase = self.session.phase();
        let over = phase.is_over();
        let past = |after: u64| {
            self.session
                .events()
                .iter()
                .any(|(seq, event)| *seq > after && event["event"] != "building")
        };
        let done = match *until {
            Until::Greeting => self.greeted,
            Until::Settled => over || (self.greeted && !phase.is_pending()),
            Until::Next {
                after,
                output,
                ticket,
            } => {
                if self.waiter != ticket {
                    return Some(Told::Next(None));
                }
                let printed = self.program_output > output;
                return (over || printed || past(after)).then_some(Told::Next(Some(after)));
            }
            Until::Change {
                event,
                output,
                breakpoints,
            } => {
                over || self.next_output > output
                    || self.breakpoints["revision"]
                        .as_u64()
                        .is_some_and(|revision| revision > breakpoints)
                    || self
                        .session
                        .events()
                        .last()
                        .is_some_and(|(seq, _)| *seq > event)
            }
            Until::Exit => self.exited || !started,
        };
        done.then_some(Told::Done)
    }

    /// The session now, as [`Session::answer`] gives it, but for its root.
    fn answer(&mut self, cursor: Option<u64>, output_cursor: Option<u64>) -> Value {
        let after = cursor.unwrap_or(self.given_event);
        let output_after = output_cursor.unwrap_or(self.given_output);
        let events: Vec<Value> = self
            .session
            .events()
            .iter()
            .filter(|(seq, event)| *seq > after && event["event"] != "building")
            .map(|(seq, event)| {
                let mut event = event.clone();
                event["seq"] = json!(seq);
                event
            })
            .collect();
        let cursor = self
            .session
            .events()
            .last()
            .map_or(after, |(seq, _)| *seq.max(&after));
        let dropped = self
            .output
            .front()
            .is_some_and(|(number, _, _)| *number > output_after + 1);
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut rawr = String::new();
        for (number, stream, text) in &self.output {
            if *number > output_after {
                match stream {
                    Stream::Stdout => stdout.push_str(text),
                    Stream::Stderr => stderr.push_str(text),
                    Stream::Rawr => rawr.push_str(text),
                }
            }
        }
        let output_cursor = self.next_output.max(output_after);
        self.given_event = cursor;
        self.given_output = output_cursor;
        // The newest failure among these events, else the one the running
        // program carries.
        let diagnostics = events
            .iter()
            .rev()
            .find(|event| event["event"] == "failed")
            .or(self.session.failure())
            .map(|event| event["diagnostics"].clone());
        let phase = self.session.phase().clone();
        let paused = self.session.paused().cloned();
        let status = match phase {
            Phase::Ended(status) => Some(status),
            Phase::Exited(status) => status,
            _ => None,
        };
        let building = self
            .session
            .building()
            .filter(|_| phase == Phase::Building)
            .map(|(stage, percent)| json!({ "stage": stage, "percent": percent }));
        json!({
            "root": Value::Null,
            "pid": self.pid,
            "state": phase.name(),
            "paused": paused,
            "status": status,
            "building": building,
            "events": events,
            "diagnostics": diagnostics,
            "stdout": stdout,
            "stderr": stderr,
            "rawr": rawr,
            "output_dropped": dropped,
            "cursor": cursor,
            "output_cursor": output_cursor,
        })
    }
}

/// A session's answer as an agent reads it.
fn summary(structured: &Value) -> String {
    let mut text = format!(
        "{}: {}",
        structured["root"].as_str().unwrap_or_default(),
        structured["state"].as_str().unwrap_or_default()
    );
    if let Some(status) = structured["status"].as_i64() {
        text.push_str(&format!(" (status {status})"));
    }
    if let Some(paused) = structured["paused"].as_object() {
        text.push_str(&format!(" {}", pause_text(paused)));
    }
    if let Some(building) = structured["building"].as_object() {
        text.push_str(&format!(
            ", {} {}%",
            building["stage"].as_str().unwrap_or_default(),
            building["percent"]
        ));
    }
    text.push_str(&format!(
        "\ncursor {}, output cursor {}\n",
        structured["cursor"], structured["output_cursor"]
    ));
    for event in structured["events"].as_array().into_iter().flatten() {
        let name = event["event"].as_str().unwrap_or_default();
        match name {
            "restarting" | "preparing" => text.push_str(&format!(
                "#{} {name}: {}\n",
                event["seq"],
                event["reason"].as_str().unwrap_or_default()
            )),
            "ended" => text.push_str(&format!(
                "#{} ended with {}\n",
                event["seq"], event["status"]
            )),
            "paused" => text.push_str(&format!(
                "#{} paused {}\n",
                event["seq"],
                event.as_object().map(pause_text).unwrap_or_default()
            )),
            _ => text.push_str(&format!("#{} {name}\n", event["seq"])),
        }
    }
    if let Some(diagnostics) = structured["diagnostics"].as_array() {
        text.push_str(&crate::diagnostic::lines(diagnostics));
    }
    for stream in ["stdout", "stderr", "rawr"] {
        let output = structured[stream].as_str().unwrap_or_default();
        if !output.is_empty() {
            text.push_str(&format!("--- {stream} ---\n{output}"));
            if !output.ends_with('\n') {
                text.push('\n');
            }
        }
    }
    if structured["output_dropped"] == true {
        text.push_str("(older output was dropped)\n");
    }
    text
}

/// Where a paused program is, as a session's summary says it: `at
/// file:line in `function` (reason)`.
#[must_use]
pub fn pause_text(paused: &serde_json::Map<String, Value>) -> String {
    let mut text = format!(
        "at {}:{}",
        paused
            .get("file")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        paused.get("line").unwrap_or(&Value::Null)
    );
    if let Some(function) = paused.get("function").and_then(Value::as_str) {
        text.push_str(&format!(" in `{function}`"));
    }
    if let Some(reason) = paused.get("reason").and_then(Value::as_str) {
        text.push_str(&format!(" ({reason})"));
    }
    text
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead as _, BufReader, Write as _};
    use std::sync::mpsc;

    use serde_json::{json, Value};

    use super::{Ear, Phase, Refusal, Session, Told, Wait};
    use crate::mcp::call::Call;

    /// A wait for the next event or output past what was last given.
    const NEXT: Wait = Wait::Next {
        cursor: None,
        output: None,
    };

    /// An ear on a plain channel, for a test to read what a wait is told.
    fn ear() -> (Ear, mpsc::Receiver<(u64, Told)>) {
        let (told, heard) = mpsc::channel();
        let ear = Ear::new(move |id, what| told.send((id, what)).is_ok());
        (ear, heard)
    }

    /// The next line the session's peer reads, as JSON.
    fn next(lines: &mut impl Iterator<Item = std::io::Result<String>>) -> Value {
        serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap()
    }

    /// A wait the client gave up on without cancelling, still parked when
    /// the next wait comes, ends with no cursor, and the later one takes
    /// the events (#1542).
    #[test]
    fn a_later_wait_takes_the_place_of_one_still_parked() {
        let (session, _other) = Session::running_at(Phase::Starting);
        let (given_up_ear, given_up) = ear();
        let first = session.park(NEXT, given_up_ear);
        let (later_ear, later) = ear();
        let second = session.park(NEXT, later_ear);
        let (id, told) = given_up.recv().unwrap();
        assert_eq!(id, first);
        assert!(matches!(told, Told::Next(None)), "{told:?}");
        session.change(|mirror| mirror.session.record(1, json!({ "event": "swapped" })));
        let (id, told) = later.recv().unwrap();
        assert_eq!(id, second);
        let Told::Next(Some(after)) = told else {
            panic!("the later wait goes on from its cursor: {told:?}");
        };
        let answer = session.answer(Some(after), None).structured;
        assert_eq!(answer["events"][0]["event"], "swapped");
        assert_eq!(answer["cursor"], 1);
        // The next wait goes on from there.
        assert_eq!(session.look(|mirror| mirror.given_event), Some(1));
    }

    /// A wait parked after the event it waits for already came ends at
    /// once: the owner checks each wait as it parks it.
    #[test]
    fn a_wait_parked_after_its_event_ends_at_once() {
        let (session, _other) = Session::running_at(Phase::Running);
        let given = session.answer(None, None).structured;
        assert_eq!(given["cursor"], 1);
        session.change(|mirror| {
            let paused = json!({ "event": "paused", "file": "/p/src/main.rs", "line": 9 });
            mirror.session.record(2, paused);
        });
        let (waiting, told) = ear();
        let id = session.park(NEXT, waiting);
        let (heard, what) = told.recv().unwrap();
        assert_eq!(heard, id);
        assert!(matches!(what, Told::Next(Some(1))), "{what:?}");
        assert_eq!(session.answer(Some(1), None).structured["state"], "paused");
    }

    /// A running program that prints and tells no event ends a parked
    /// wait with what it printed, while rawr's own lines leave the wait
    /// parked (#2267).
    #[test]
    fn a_line_the_program_prints_ends_a_parked_wait() {
        let (session, other) = Session::running_at(Phase::Running);
        assert_eq!(session.answer(None, None).structured["cursor"], 1);
        let (waiting, told) = ear();
        let id = session.park(NEXT, waiting);
        session.read_rawr(b"rawr: the program runs\n");
        // A look comes after the owner took the line and checked the wait.
        assert_eq!(session.phase(), Phase::Running);
        assert!(told.try_recv().is_err(), "rawr's line ended the wait");
        let printed = json!({ "jsonrpc": "2.0", "method": "output", "params": {
            "stream": "stdout", "text": "listening on 8080\n",
        } });
        writeln!(&other, "{printed}").unwrap();
        let (heard, what) = told.recv().unwrap();
        assert_eq!(heard, id);
        let Told::Next(Some(after)) = what else {
            panic!("the wait goes on from its cursor: {what:?}");
        };
        let answer = session.answer(Some(after), None).structured;
        assert_eq!(answer["stdout"], "listening on 8080\n", "{answer}");
        assert_eq!(answer["events"], json!([]), "{answer}");
    }

    /// Which of a debugger answer and the program's `paused` event the
    /// session tells first.
    #[derive(Clone, Copy)]
    enum Order {
        AnswerFirst,
        PauseFirst,
    }

    /// The program answers `ask` with no pause, then pauses at line 9,
    /// the two told in `order` in one write. Whichever comes
    /// first, the program stays paused, and the next wait returns that
    /// pause rather than waiting for an event that never comes.
    fn a_pause_after_an_answer(ask: &'static str, order: Order) {
        let (session, other) = Session::running_at(Phase::Running);
        let mut lines = BufReader::new(other.try_clone().unwrap()).lines();
        assert_eq!(next(&mut lines)["method"], "hello");
        let asking = std::sync::Arc::clone(&session);
        let asked = std::thread::spawn(move || {
            let call = Call::default();
            let (answer, seen) = asking.ask(&json!({ "op": ask }), &call).unwrap();
            let paused = &answer["paused"];
            asking.observe_pause(paused.as_object().map(|_| paused.clone()), seen);
        });
        let request = next(&mut lines);
        let answer = json!({ "jsonrpc": "2.0", "id": request["id"], "result": { "paused": null } });
        let paused = json!({ "jsonrpc": "2.0", "method": "event", "params": {
            "event": "paused", "seq": 2, "file": "/p/src/main.rs", "line": 9,
        } });
        match order {
            Order::AnswerFirst => write!(&other, "{answer}\n{paused}\n").unwrap(),
            Order::PauseFirst => write!(&other, "{paused}\n{answer}\n").unwrap(),
        }
        asked.join().unwrap();
        let after = session.next(Some(1), None, &Call::default(), |_, _| {});
        assert_eq!(after, Some(1));
        let now = session.answer(after, None).structured;
        assert_eq!(now["state"], "paused", "{now}");
        assert_eq!(now["paused"]["line"], 9, "{now}");
    }

    /// A `continue` the program answers, then pauses again at once.
    #[test]
    fn a_pause_told_after_a_continue_answer_wins_over_it() {
        a_pause_after_an_answer("continue", Order::AnswerFirst);
    }

    /// A `break` (or input) whose answer, saying the program runs, reaches
    /// the server after the pause the breakpoint made: the answer is older
    /// than the pause, though it came later.
    #[test]
    fn a_pause_told_before_a_break_answer_wins_over_it() {
        a_pause_after_an_answer("break", Order::PauseFirst);
    }

    /// A request in flight when the session's connection closes is
    /// refused as not running, and a wait for the program ends, so no
    /// call hangs on a peer that went away.
    #[test]
    fn a_peer_that_closes_mid_request_ends_the_request_and_the_wait() {
        let (session, other) = Session::running_at(Phase::Starting);
        let mut lines = BufReader::new(other.try_clone().unwrap()).lines();
        assert_eq!(next(&mut lines)["method"], "hello");
        let asking = std::sync::Arc::clone(&session);
        let asked =
            std::thread::spawn(move || asking.ask(&json!({ "op": "pause" }), &Call::default()));
        let waiting = std::sync::Arc::clone(&session);
        let settled = std::thread::spawn(move || waiting.settle(&Call::default(), |_, _| {}));
        assert_eq!(next(&mut lines)["method"], "program");
        other.shutdown(std::net::Shutdown::Both).unwrap();
        assert!(matches!(asked.join().unwrap(), Err(Refusal::NotRunning)));
        settled.join().unwrap();
        assert_eq!(session.phase(), Phase::Exited(None));
    }

    /// A request cancelled while the program has it returns at once, and
    /// the session hears the cancel of that request.
    #[test]
    fn a_cancel_mid_request_returns_and_cancels_it_at_the_session() {
        let (session, other) = Session::running_at(Phase::Running);
        let mut lines = BufReader::new(other.try_clone().unwrap()).lines();
        assert_eq!(next(&mut lines)["method"], "hello");
        let call = Call::default();
        let waker = call.waker();
        let asking = std::sync::Arc::clone(&session);
        let asked = std::thread::spawn(move || asking.ask(&json!({ "op": "pause" }), &call));
        let request = next(&mut lines);
        assert_eq!(request["method"], "program");
        waker.cancel();
        assert!(matches!(asked.join().unwrap(), Err(Refusal::Cancelled)));
        let cancel = next(&mut lines);
        assert_eq!(cancel["method"], "cancel", "{cancel}");
        assert_eq!(cancel["params"]["id"], request["id"]);
        // The program's late answer finds no request and changes nothing.
        let late = json!({ "jsonrpc": "2.0", "id": request["id"], "result": {} });
        writeln!(&other, "{late}").unwrap();
        assert_eq!(session.phase(), Phase::Running);
    }
}
