//! The programs the server reaches, each through its run's session
//! ([`crate::events::session`]): one `rawr run` per project root that
//! `start` runs, in a process group of its own with a stdin pipe and its
//! output kept with a cursor (D537.3), or a run started elsewhere, as in a
//! terminal, which a tool attaches to (D1053.2). The owner keeps watching
//! and reloading, as at a terminal; the server keeps its own copy of what
//! the session tells, numbered as the session numbers it.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard, PoisonError};

use serde_json::{json, Value};

use super::process;
use crate::events::session;

pub use crate::events::session::Phase;

/// Output kept per session, oldest first out.
const OUTPUT: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stream {
    Stdout,
    Stderr,
}

impl Stream {
    const fn name(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
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
    /// What the last answer returned: a call without a cursor goes on
    /// from here.
    given_event: u64,
    given_output: u64,
    /// The newest `wait`'s number: a wait whose number is older was given
    /// up by its client, which waits once at a time, and answers nothing.
    waiter: u64,
}

/// How the session answered a request.
#[derive(Debug)]
enum Reply {
    Answer(Value),
    Error(Value),
    /// The connection closed first.
    Closed,
    Cancelled,
}

/// Why a request to the program has no answer.
#[derive(Debug)]
pub enum Refusal {
    /// No program runs to answer it.
    NotRunning,
    /// The program refused it.
    Error(String),
    Cancelled,
}

/// The connection to a run's session: requests by number, written in
/// order by a thread of its own, so output the server tells never waits on
/// the session.
#[derive(Debug)]
struct Link {
    stream: UnixStream,
    out: mpsc::Sender<String>,
    /// The requests waiting for their answers; `None` once the connection
    /// closed.
    pending: Mutex<Option<HashMap<u64, mpsc::Sender<Reply>>>>,
    next: AtomicU64,
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

/// One program the server reaches.
#[derive(Debug)]
pub struct Session {
    /// The project root, the session's key.
    pub root: PathBuf,
    /// The path given to `rawr run`.
    pub path: PathBuf,
    /// The program's arguments.
    pub arguments: Vec<String>,
    /// The run `start` started; `None` for a run started elsewhere.
    started: Option<Started>,
    link: Link,
    state: Mutex<Mirror>,
    changed: Condvar,
}

/// The sessions, one per project root.
#[derive(Debug, Default)]
pub struct Sessions(Mutex<HashMap<PathBuf, Arc<Session>>>);

impl Sessions {
    fn lock(&self) -> MutexGuard<'_, HashMap<PathBuf, Arc<Session>>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The session for `root`: the server's, or the session of a run
    /// started elsewhere, which the server attaches to. One that is over
    /// gives way to a run there now.
    pub fn get(&self, root: &Path, cancel: &process::Cancel) -> Option<Arc<Session>> {
        let known = self.lock().get(root).cloned();
        if known
            .as_ref()
            .is_some_and(|session| !session.phase().is_over())
        {
            return known;
        }
        let Some(attached) = Session::attach(root, cancel) else {
            return known;
        };
        self.lock()
            .insert(root.to_path_buf(), Arc::clone(&attached));
        Some(attached)
    }

    /// Every session.
    #[must_use]
    pub fn all(&self) -> Vec<Arc<Session>> {
        let mut all: Vec<_> = self.lock().values().cloned().collect();
        all.sort_by(|a, b| a.root.cmp(&b.root));
        all
    }

    /// Start `rawr run <path> -- arguments` for `root`, replacing the
    /// session the server started there. A run started elsewhere that
    /// serves `root` is attached to instead.
    pub fn start(
        &self,
        root: &Path,
        path: &Path,
        arguments: &[String],
        stdin: Option<&str>,
        cancel: &process::Cancel,
    ) -> std::io::Result<Arc<Session>> {
        if let Some(old) = self.lock().remove(root) {
            old.stop();
            old.wait_exit();
        }
        if let Some(attached) = Session::attach(root, cancel) {
            self.lock()
                .insert(root.to_path_buf(), Arc::clone(&attached));
            return Ok(attached);
        }
        let place = session::Place::new(root)?;
        place.make()?;
        let socket = place.socket();
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket)?;
        let mut command = Command::new(std::env::current_exe()?);
        command
            .arg("run")
            .arg(path)
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
            .stderr(Stdio::piped());
        session::hand(&mut command, &listener);
        let mut child = process::spawn(&mut command)?;
        drop(listener);
        // The kernel holds the connection until the run accepts it.
        let session = Session::open(
            root,
            path,
            arguments,
            Some(Started {
                group: child.id(),
                place,
            }),
            UnixStream::connect(&socket)?,
        )?;
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
                Stream::Stderr,
            ),
        ] {
            if let Some(pipe) = pipe {
                let reading = Arc::clone(&session);
                session.state().pipes += 1;
                let spawned = std::thread::Builder::new()
                    .name("rawr-mcp-session-output".into())
                    .spawn(move || reading.read_output(pipe, stream));
                if spawned.is_err() {
                    session.state().pipes -= 1;
                }
            }
        }
        let waiting = Arc::clone(&session);
        let _ = std::thread::Builder::new()
            .name("rawr-mcp-session".into())
            .spawn(move || waiting.follow(child));
        self.lock().insert(root.to_path_buf(), Arc::clone(&session));
        Ok(session)
    }

    /// End the session for `root`, if any.
    pub fn stop(&self, root: &Path) -> Option<Arc<Session>> {
        let session = self.lock().remove(root)?;
        session.stop();
        Some(session)
    }

    /// End every session.
    pub fn stop_all(&self) {
        let sessions: Vec<_> = self.lock().drain().map(|(_, session)| session).collect();
        for session in sessions {
            session.stop();
        }
    }
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
    /// A session reached on `stream`, which it greets: of the `rawr run`
    /// the server `started`, or of a run started elsewhere.
    fn open(
        root: &Path,
        path: &Path,
        arguments: &[String],
        started: Option<Started>,
        stream: UnixStream,
    ) -> std::io::Result<Arc<Self>> {
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
        let session = Arc::new(Self {
            root: root.to_path_buf(),
            path: path.to_path_buf(),
            arguments: arguments.to_vec(),
            link: Link {
                stream,
                out,
                pending: Mutex::new(Some(HashMap::new())),
                next: AtomicU64::new(1),
            },
            state: Mutex::new(Mirror {
                pid: started.as_ref().map_or(0, |started| started.group),
                ..Mirror::default()
            }),
            started,
            changed: Condvar::new(),
        });
        // The session's first answer is to `hello`: what happened so far.
        session
            .link
            .send(&json!({ "jsonrpc": "2.0", "id": 0, "method": "hello" }));
        let listening = Arc::clone(&session);
        std::thread::Builder::new()
            .name("rawr-mcp-session-read".into())
            .spawn(move || listening.listen(reading))?;
        Ok(session)
    }

    /// The session of a run started elsewhere that serves `root`, once it
    /// greeted the server: `None` when no run serves it there, or `cancel`
    /// fired first.
    fn attach(root: &Path, cancel: &process::Cancel) -> Option<Arc<Self>> {
        let socket = session::Place::new(root).ok()?.socket();
        let stream = UnixStream::connect(socket).ok()?;
        let session = Self::open(root, root, &[], None, stream).ok()?;
        session.wake_on(cancel);
        let mut state = session.state();
        while !state.greeted && !cancel.is_cancelled() {
            state = session
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        let reached = state.greeted && !state.closed;
        drop(state);
        if !reached {
            session.stop();
            return None;
        }
        Some(session)
    }

    /// A session already in `phase`, with no process of its own, on one
    /// end of a pair whose other end is returned.
    #[cfg(test)]
    pub fn running_at(phase: Phase) -> (Arc<Self>, UnixStream) {
        let (stream, other) = UnixStream::pair().unwrap();
        let session = Self::open(Path::new("/p"), Path::new("/p"), &[], None, stream).unwrap();
        let mut state = session.state();
        state.greeted = true;
        match phase {
            Phase::Running => state.session.record(1, json!({ "event": "running" })),
            Phase::Starting => {}
            other => state.session.end(other),
        }
        drop(state);
        (session, other)
    }

    fn state(&self) -> MutexGuard<'_, Mirror> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Wake this session's waits when `cancel` fires.
    fn wake_on(self: &Arc<Self>, cancel: &process::Cancel) {
        let waking = Arc::clone(self);
        cancel.on_cancel(move || {
            // A wait looks at the cancel under this lock.
            drop(waking.state());
            waking.changed.notify_all();
        });
    }

    /// The current phase.
    #[must_use]
    pub fn phase(&self) -> Phase {
        self.state().session.phase().clone()
    }

    /// The pid of `rawr run`.
    #[must_use]
    pub fn pid(&self) -> u32 {
        self.state().pid
    }

    /// Whether the server started this session's run.
    #[must_use]
    pub const fn is_started(&self) -> bool {
        self.started.is_some()
    }

    /// Where the program is paused, if it is.
    #[must_use]
    pub fn paused(&self) -> Option<Value> {
        self.state().session.paused().cloned()
    }

    /// The program answered a debugger tool: paused at `paused`, or
    /// running. Its own `paused` and `resumed` events say the same once
    /// they come.
    pub fn observe_pause(&self, paused: Option<Value>) {
        self.state().session.observe_pause(paused);
    }

    /// Ask the program `request` through the session, which waits for the
    /// program that runs next while it starts or restarts.
    pub fn ask(&self, request: &Value, cancel: &process::Cancel) -> Result<Value, Refusal> {
        let id = self.link.next.fetch_add(1, Ordering::Relaxed);
        let (reply, answer) = mpsc::channel();
        {
            let mut pending = self
                .link
                .pending
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let Some(pending) = pending.as_mut() else {
                return Err(Refusal::NotRunning);
            };
            pending.insert(id, reply.clone());
        }
        let out = self.link.out.clone();
        cancel.on_cancel(move || {
            let cancel = json!({ "jsonrpc": "2.0", "method": "cancel", "params": { "id": id } });
            let _ = out.send(format!("{cancel}\n"));
            let _ = reply.send(Reply::Cancelled);
        });
        self.link
            .send(&json!({ "jsonrpc": "2.0", "id": id, "method": "program", "params": request }));
        let reply = answer.recv().unwrap_or(Reply::Closed);
        if let Some(pending) = self
            .link
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
        {
            pending.remove(&id);
        }
        match reply {
            Reply::Answer(answer) => Ok(answer),
            Reply::Error(error) if error["code"] == session::NOT_RUNNING => {
                Err(Refusal::NotRunning)
            }
            Reply::Error(error) if error["code"] == session::CANCELLED => Err(Refusal::Cancelled),
            Reply::Error(error) => Err(Refusal::Error(
                error["message"].as_str().unwrap_or_default().to_owned(),
            )),
            Reply::Closed => Err(Refusal::NotRunning),
            Reply::Cancelled => Err(Refusal::Cancelled),
        }
    }

    /// Read the session's messages until its connection closes.
    fn listen(&self, stream: UnixStream) {
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else {
                break;
            };
            let Ok(mut message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            match (message["id"].as_u64(), message["method"].as_str()) {
                (Some(0), None) => self.greeted(&message["result"]),
                (Some(id), None) => self.link.answered(id, message),
                (None, Some("event")) => {
                    let event = message["params"].take();
                    if let Some(seq) = event["seq"].as_u64() {
                        self.state().session.record(seq, event);
                        self.changed.notify_all();
                    }
                }
                (None, Some("output")) => {
                    let params = &message["params"];
                    let stream = match params["stream"].as_str() {
                        Some("stderr") => Stream::Stderr,
                        _ => Stream::Stdout,
                    };
                    if let Some(text) = params["text"].as_str() {
                        self.state().keep(stream, text.to_owned());
                    }
                }
                _ => {}
            }
        }
        self.link.close();
        let mut state = self.state();
        state.closed = true;
        state.greeted = true;
        // A run started elsewhere ended: its last `ended` says how.
        if self.started.is_none() {
            let status = match state.session.phase() {
                Phase::Ended(status) => Some(*status),
                _ => None,
            };
            state.session.end(Phase::Exited(status));
        }
        drop(state);
        self.changed.notify_all();
    }

    /// The session's answer to `hello`: the events so far, and for a run
    /// started elsewhere, its pid and the output it kept.
    fn greeted(&self, result: &Value) {
        let mut state = self.state();
        for event in result["events"].as_array().into_iter().flatten() {
            if let Some(seq) = event["seq"].as_u64() {
                state.session.record(seq, event.clone());
            }
        }
        if self.started.is_none() {
            state.pid = result["pid"]
                .as_u64()
                .and_then(|pid| u32::try_from(pid).ok())
                .unwrap_or(0);
            for piece in result["output"].as_array().into_iter().flatten() {
                let stream = match piece["stream"].as_str() {
                    Some("stderr") => Stream::Stderr,
                    _ => Stream::Stdout,
                };
                if let Some(text) = piece["text"].as_str() {
                    state.keep(stream, text.to_owned());
                }
            }
        }
        state.greeted = true;
        drop(state);
        self.changed.notify_all();
    }

    /// Keep `rawr run`'s output, and tell it to the session for the other
    /// shells attached to it.
    fn read_output(&self, mut pipe: Box<dyn std::io::Read + Send>, stream: Stream) {
        let mut buffer = [0_u8; 8192];
        while let Ok(read) = pipe.read(&mut buffer) {
            if read == 0 {
                break;
            }
            let text = String::from_utf8_lossy(&buffer[..read]).into_owned();
            self.link.send(&json!({
                "jsonrpc": "2.0",
                "method": "output",
                "params": { "stream": stream.name(), "text": text },
            }));
            self.state().keep(stream, text);
        }
        self.state().pipes -= 1;
        self.changed.notify_all();
    }

    /// Wait for `rawr run` to exit, then for its session's last words and
    /// the end of its output, then record the exit. The run's place goes
    /// first, so a `start` that waited for the exit binds a new one.
    fn follow(&self, mut child: std::process::Child) {
        let status = child.wait().ok().map(process::code);
        let mut state = self.state();
        while !state.closed || state.pipes > 0 {
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        if let Some(started) = &self.started {
            started.place.remove();
        }
        state.session.end(Phase::Exited(status));
        state.exited = true;
        drop(state);
        self.changed.notify_all();
    }

    /// End the session: the group the server started, SIGTERM and then
    /// SIGKILL after a grace, or the connection to a run started
    /// elsewhere, which goes on.
    pub fn stop(&self) {
        {
            let mut state = self.state();
            if state.session.phase().is_over() {
                return;
            }
            state.session.end(Phase::Stopped);
        }
        match &self.started {
            Some(started) => process::end_group(started.group),
            None => {
                let _ = self.link.stream.shutdown(std::net::Shutdown::Both);
            }
        }
        self.changed.notify_all();
    }

    /// Wait for `rawr run` to exit, after [`Self::stop`] ended the group
    /// the server started.
    pub fn wait_exit(&self) {
        if self.started.is_none() {
            return;
        }
        let mut state = self.state();
        while !state.exited {
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Block until something past `event` happens that `wakes`, the session
    /// is over, or `cancel` fires; `building` hears each build update.
    fn block(
        self: &Arc<Self>,
        event: u64,
        cancel: &process::Cancel,
        wakes: impl Fn(&Mirror, u64) -> bool,
        mut building: impl FnMut(&str, u64),
    ) {
        self.wake_on(cancel);
        let mut heard: Option<(String, u64)> = None;
        let mut state = self.state();
        loop {
            if let Some((stage, percent)) = state.session.building()
                && heard
                    .as_ref()
                    .is_none_or(|(said, then)| said != stage || *then != percent)
            {
                building(stage, percent);
                heard = Some((stage.to_owned(), percent));
            }
            if wakes(&state, event) || state.session.phase().is_over() || cancel.is_cancelled() {
                return;
            }
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Wait until the program runs or fails, or `cancel` fires.
    pub fn settle(self: &Arc<Self>, cancel: &process::Cancel, building: impl FnMut(&str, u64)) {
        self.block(
            0,
            cancel,
            |state, _| state.greeted && !state.session.phase().is_pending(),
            building,
        );
    }

    /// Wait for the next event past `cursor` other than build progress, or
    /// `cancel`: the cursor the wait went on from, or `None` once a later
    /// wait took its place. A client waits once at a time, so a wait that
    /// comes while another is still blocked means the client gave the
    /// first up (#1542): that one gives no events, and the later one
    /// returns them.
    pub fn next(
        self: &Arc<Self>,
        cursor: Option<u64>,
        cancel: &process::Cancel,
        building: impl FnMut(&str, u64),
    ) -> Option<u64> {
        let (after, ticket) = {
            let mut state = self.state();
            state.waiter += 1;
            (cursor.unwrap_or(state.given_event), state.waiter)
        };
        self.changed.notify_all();
        self.block(
            after,
            cancel,
            |state, after| {
                state.waiter != ticket
                    || state
                        .session
                        .events()
                        .iter()
                        .any(|(seq, event)| *seq > after && event["event"] != "building")
            },
            building,
        );
        (self.state().waiter == ticket).then_some(after)
    }

    /// The session now: its phase, the events past `cursor` and the output
    /// past `output_cursor` (each defaulting to what was last given), which
    /// then count as given.
    pub fn answer(&self, cursor: Option<u64>, output_cursor: Option<u64>) -> Answer {
        let mut state = self.state();
        let after = cursor.unwrap_or(state.given_event);
        let output_after = output_cursor.unwrap_or(state.given_output);
        let events: Vec<Value> = state
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
        let cursor = state
            .session
            .events()
            .last()
            .map_or(after, |(seq, _)| *seq.max(&after));
        let dropped = state
            .output
            .front()
            .is_some_and(|(number, _, _)| *number > output_after + 1);
        let mut stdout = String::new();
        let mut stderr = String::new();
        for (number, stream, text) in &state.output {
            if *number > output_after {
                match stream {
                    Stream::Stdout => stdout.push_str(text),
                    Stream::Stderr => stderr.push_str(text),
                }
            }
        }
        let output_cursor = state.next_output.max(output_after);
        state.given_event = cursor;
        state.given_output = output_cursor;
        // The newest failure among these events, else the one the running
        // program carries.
        let diagnostics = events
            .iter()
            .rev()
            .find(|event| event["event"] == "failed")
            .or(state.session.failure())
            .map(|event| event["diagnostics"].clone());
        let phase = state.session.phase().clone();
        let paused = state.session.paused().cloned();
        let status = match phase {
            Phase::Ended(status) => Some(status),
            Phase::Exited(status) => status,
            _ => None,
        };
        let building = state
            .session
            .building()
            .filter(|_| phase == Phase::Building)
            .map(|(stage, percent)| json!({ "stage": stage, "percent": percent }));
        let pid = state.pid;
        drop(state);
        let structured = json!({
            "root": self.root,
            "pid": pid,
            "state": phase.name(),
            "paused": paused,
            "status": status,
            "building": building,
            "events": events,
            "diagnostics": diagnostics,
            "stdout": stdout,
            "stderr": stderr,
            "output_dropped": dropped,
            "cursor": cursor,
            "output_cursor": output_cursor,
        });
        let summary = summary(&structured);
        Answer {
            structured,
            summary,
        }
    }
}

impl Mirror {
    /// Keep a piece of output, dropping the oldest past [`OUTPUT`].
    fn keep(&mut self, stream: Stream, text: String) {
        self.next_output += 1;
        let number = self.next_output;
        self.output_bytes += text.len();
        self.output.push_back((number, stream, text));
        while self.output_bytes > OUTPUT {
            let Some((_, _, dropped)) = self.output.pop_front() else {
                break;
            };
            self.output_bytes -= dropped.len();
        }
    }
}

impl Link {
    fn send(&self, message: &Value) {
        let _ = self.out.send(format!("{message}\n"));
    }

    /// The session answered request `id`.
    fn answered(&self, id: u64, mut message: Value) {
        let reply = match message.get_mut("result") {
            Some(result) => Reply::Answer(result.take()),
            None => Reply::Error(message["error"].take()),
        };
        let waiting = self
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
            .and_then(|pending| pending.remove(&id));
        if let Some(waiting) = waiting {
            let _ = waiting.send(reply);
        }
    }

    /// The connection closed: no request waiting gets an answer.
    fn close(&self) {
        let pending = self
            .pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        for (_, waiting) in pending.into_iter().flatten() {
            let _ = waiting.send(Reply::Closed);
        }
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
    for (stream, key) in [("stdout", "stdout"), ("stderr", "stderr")] {
        let output = structured[key].as_str().unwrap_or_default();
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
    use super::{Phase, Session};
    use serde_json::json;

    /// A wait the client gave up on without cancelling, still blocked when
    /// the next wait comes, gives that one the events (#1542).
    #[test]
    fn a_later_wait_takes_the_place_of_one_still_blocked() {
        let (session, _other) = Session::running_at(Phase::Starting);
        let waiting = |session: &std::sync::Arc<Session>| {
            let session = std::sync::Arc::clone(session);
            std::thread::spawn(move || {
                let cancel = super::process::Cancel::default();
                let after = session.next(None, &cancel, |_, _| {});
                after.map(|after| session.answer(Some(after), None).structured)
            })
        };
        let given_up = waiting(&session);
        while session.state().waiter < 1 {
            std::thread::yield_now();
        }
        let later = waiting(&session);
        assert_eq!(given_up.join().unwrap(), None);
        session
            .state()
            .session
            .record(1, json!({ "event": "swapped" }));
        session.changed.notify_all();
        let answer = later.join().unwrap().expect("the later wait answers");
        assert_eq!(answer["events"][0]["event"], "swapped");
        assert_eq!(answer["cursor"], 1);
        // The next wait goes on from there.
        assert_eq!(session.state().given_event, 1);
    }
}
