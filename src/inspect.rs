//! Reading a running program's values (D557, #488). A tool connects to a
//! socket the program serves and asks for the bindings in scope where the
//! program is, or for one binding by name, optionally in one file. The
//! program answers at its next step in its own code, where the debugger
//! callback runs: each value as Rust's `{:?}` prints it, through the
//! program's own `Debug` impls and derives and a crate type's `Debug`.
//!
//! The protocol is one JSON line each way:
//! - the request: `{"name": "count", "file": "/abs/src/main.rs", "timeout_ms": 2000}`,
//!   each field optional;
//! - the answer: `{"reached": true, "file": .., "line": .., "function": ..,
//!   "bindings": [{"name": .., "value": ..} or {"name": .., "error": ..}]}`,
//!   or `{"reached": false}` when the program ran none of its own code
//!   that matched before the timeout.
//!
//! Nothing runs while no request waits: the engine checks one flag at each
//! step.
//!
//! The same socket sets breakpoints and resumes a paused program (#489),
//! with an `op`:
//! - `{"op": "break", "file": .., "line": ..}` or `{"op": "break",
//!   "function": "tick"}`, then a wait of up to `timeout_ms` for a pause:
//!   `{"set": .., "lines": [{"file": .., "line": ..}], "compiled": ..,
//!   "paused": {"file": .., "line": .., "function": .., "reason": ..} or null}`,
//!   or `{"error": ..}`;
//! - `{"op": "clear"}`, with `file`, `line` or `function` to take fewer:
//!   `{"cleared": n, "left": [..], "paused": ..}`;
//! - `{"op": "step", "kind": "over" | "into" | "out"}` and
//!   `{"op": "continue"}` on a paused program, then a wait of up to
//!   `timeout_ms` for the next pause: `{"paused": .. or null}`, or
//!   `{"error": "not paused"}`.
//!
//! A pause holds the thread that runs the program's code inside the
//! debugger callback. Native code that called into the program, such as a
//! window's event loop, waits in that call until the program goes on. A
//! step pauses at the next statement the program wrote. It can't stop in
//! a crate's native code, so a step out of a function that native code
//! called pauses at the program's next call from it.
#![cfg(feature = "debugging")]

mod stops;

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use serde_json::{json, Value};

use crate::debugger::{BreakPositions, DebuggerCommand, DebuggerEvent};
use crate::types::dynamic::Union;
use crate::{Dynamic, Engine, EvalAltResult, EvalContext, Position};

/// How long a request waits by default.
const TIMEOUT: Duration = Duration::from_secs(2);

/// One binding as a tool reads it: its name and its `Debug` text, or why it
/// has none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    /// The binding's name.
    pub name: String,
    /// Its value as `{:?}` prints it, or Rust's reason it can't.
    pub value: Result<String, String>,
}

impl Binding {
    fn to_json(&self) -> Value {
        match &self.value {
            Ok(text) => json!({ "name": self.name, "value": text }),
            Err(reason) => json!({ "name": self.name, "error": reason }),
        }
    }
}

/// Whether a scope entry is one the program wrote, not one the compiler
/// added.
fn is_written(name: &str) -> bool {
    !name.is_empty() && !name.starts_with("__rawr") && !name.contains('\0') && !name.contains('$')
}

/// The bindings in scope where `context` is, the last of each name, in the
/// order they were made.
pub fn bindings(context: &mut EvalContext) -> Vec<Binding> {
    let mut seen = HashSet::new();
    let mut found: Vec<(String, Dynamic)> = context
        .scope()
        .iter_rev_inner()
        .filter(|(name, ..)| is_written(name) && seen.insert(name.to_string()))
        .map(|(name, _, value)| (name.to_string(), value.flatten_clone()))
        .collect();
    found.reverse();
    found
        .into_iter()
        .map(|(name, value)| Binding {
            value: debug_text(context, &value),
            name,
        })
        .collect()
}

/// The binding `name` in scope where `context` is.
pub fn binding(context: &mut EvalContext, name: &str) -> Option<Binding> {
    let value = context
        .scope()
        .iter_rev_inner()
        .find(|(entry, ..)| entry.as_str() == name)
        .map(|(_, _, value)| value.flatten_clone())?;
    Some(Binding {
        name: name.to_owned(),
        value: debug_text(context, &value),
    })
}

/// Whether `name` is in scope where `context` is.
fn in_scope(context: &EvalContext, name: &str) -> bool {
    context
        .scope()
        .iter_rev_inner()
        .any(|(entry, ..)| entry.as_str() == name)
}

/// The function the program is in where `context` is, as Rust names it.
#[must_use]
pub fn function(context: &EvalContext) -> Option<String> {
    let frame = context
        .global_runtime_state()
        .debugger
        .as_ref()?
        .call_stack()
        .last()?;
    Some(crate::resolve::lower_nominal::function_display_name(&frame.fn_name).to_owned())
}

/// `value` as the program's `{:?}` prints it: a hand-written `Debug` impl
/// first, then a derive or a builtin's. A crate type without `Debug` gives
/// Rust's reason instead.
pub fn debug_text(context: &mut EvalContext, value: &Dynamic) -> Result<String, String> {
    if let Some(name) = undebuggable(value) {
        return Err(format!(
            "`{}` doesn't implement `Debug`",
            short_type_name(name)
        ));
    }
    let mut receiver = value.clone();
    let mut formatter = Dynamic::from(crate::ImmutableString::from(""));
    let dispatched = context.call_fn_raw(
        crate::resolve::lower_nominal::DEBUG_DISPATCHER,
        true,
        true,
        &mut [&mut receiver, &mut formatter],
    );
    let text = match dispatched {
        Err(error) if matches!(*error, EvalAltResult::ErrorFunctionNotFound(..)) => {
            let mut receiver = value.clone();
            context.call_fn_raw(
                crate::resolve::lower_nominal::DEBUG_GENERIC,
                true,
                false,
                &mut [&mut receiver],
            )
        }
        other => other,
    };
    match text {
        Ok(text) => Ok(text
            .into_immutable_string()
            .map_or_else(|other| other.to_string(), |text| text.to_string())),
        // The program's formatter failed: the value's own text.
        Err(_) => Ok(format!("{value:?}")),
    }
}

/// How a host that keeps crate values behind handles names the type of one
/// without `Debug`, as a runner's arena does.
pub type Undebuggable = fn(&Dynamic) -> Option<&'static str>;

static UNDEBUGGABLE: std::sync::OnceLock<Undebuggable> = std::sync::OnceLock::new();

/// Name a crate value's type that has no `Debug` with `name`, for values
/// the host keeps behind handles. Set once per process.
pub fn name_undebuggable_with(name: Undebuggable) {
    let _ = UNDEBUGGABLE.set(name);
}

/// The type of the first value inside `value` that has no `Debug` text: a
/// crate value whose type doesn't implement `Debug`.
fn undebuggable(value: &Dynamic) -> Option<&'static str> {
    match &value.0 {
        Union::Array(items, ..) => items.iter().find_map(undebuggable),
        Union::Map(map, ..) => map.values().find_map(undebuggable),
        Union::Struct(value, ..) => value.fields().iter().find_map(undebuggable),
        Union::Enum(value, ..) => value.payload().iter().find_map(undebuggable),
        Union::Shared(..) => undebuggable(&value.flatten_clone()),
        Union::Variant(variant, ..) => {
            if let Some(option) = crate::option::read(value) {
                return option.as_ref().and_then(undebuggable);
            }
            if let Some(result) = crate::result::read(value) {
                return match &result {
                    Ok(inner) | Err(inner) => undebuggable(inner),
                };
            }
            if let Some(reverse) = value.read_lock::<std::cmp::Reverse<Dynamic>>() {
                return undebuggable(&reverse.0);
            }
            if crate::place::get(value).is_some() {
                return None;
            }
            if let Some(name) = UNDEBUGGABLE.get().and_then(|name| name(value)) {
                return Some(name);
            }
            // Without a `Debug` text, `{:?}` falls back to the type's name.
            let name = (***variant).type_name();
            (format!("{value:?}") == name).then_some(name)
        }
        _ => None,
    }
}

/// A type's name as rustc writes it in a message: each path trimmed to its
/// last segment, `Window<Id>` for `app::Window<app::Id>`.
fn short_type_name(name: &str) -> String {
    let mut output = String::with_capacity(name.len());
    let mut segment = String::new();
    for character in name.chars() {
        if character.is_alphanumeric() || character == '_' || character == ':' {
            segment.push(character);
        } else {
            output.push_str(segment.rsplit("::").next().unwrap_or_default());
            segment.clear();
            output.push(character);
        }
    }
    output.push_str(segment.rsplit("::").next().unwrap_or_default());
    output
}

/// Where a step of the program is: its file and line.
pub type Locate = dyn Fn(Position) -> Option<(String, usize)> + Send + Sync;

/// A compiled program's entry source and the compiler's positions, which
/// place a step of the lowered program in its file and line. A compiler
/// keeps them only on an engine a debugger was registered on.
pub struct Located {
    entry: crate::diagnostic::Source,
    positions: crate::program::Positions,
}

impl Located {
    // Only a runner's compiler, a rust-import build, keeps one.
    #[cfg_attr(not(feature = "rust-import"), allow(dead_code))]
    pub(crate) const fn new(
        entry: crate::diagnostic::Source,
        positions: crate::program::Positions,
    ) -> Self {
        Self { entry, positions }
    }

    /// The file and line `position` in the lowered program comes from, or
    /// `None` in code the compiler wrote around the program's.
    #[must_use]
    pub fn line(&self, position: Position) -> Option<(String, usize)> {
        let (file, span) = self.positions.locate_position(&self.entry, position)?;
        Some((file.identity().to_owned(), span.start().line()?))
    }
}

struct Request {
    id: u64,
    name: Option<String>,
    file: Option<String>,
    reply: mpsc::Sender<Value>,
}

/// How a paused program goes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// To the next statement in this function or one it returns to.
    Over,
    /// To the next statement, in a function it calls too.
    Into,
    /// To the next statement in a function it returns to.
    Out,
}

enum Resume {
    Continue,
    Step(Step),
}

/// A step that pauses again at the next statement it reaches.
struct Stepping {
    step: Step,
    /// The call depth, position and line it started at.
    depth: usize,
    position: Position,
    file: String,
    line: usize,
    /// The program left the code it stepped from for native code, which
    /// called into the program again: its next statement pauses.
    anywhere: bool,
}

#[derive(Default)]
struct State {
    pending: Vec<Request>,
    /// The breakpoints, and the program's statements once it compiled.
    points: Vec<stops::Point>,
    stops: Option<Arc<stops::Stops>>,
    /// Where the program is paused, as a tool reads it.
    paused: Option<Value>,
    /// How many times it paused, and the last place.
    pauses: u64,
    last: Value,
    resume: Option<Resume>,
    stepping: Option<Stepping>,
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    wake: Arc<AtomicBool>,
    breaks: Arc<BreakPositions>,
    /// A step is under way: each step calls back until it pauses.
    stepping: AtomicBool,
    /// The callback is answering: steps of the `Debug` code it runs are not
    /// answered and don't pause.
    answering: AtomicBool,
    /// One step of the program pauses at a time.
    one: Mutex<()>,
    /// Where the running program's steps are, once it compiled.
    locate: Mutex<Option<Arc<Locate>>>,
    /// Where the breakpoints are kept across restarts.
    saved: std::path::PathBuf,
}

impl Shared {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Where `position` is in the program's files.
    fn locate(&self, position: Position) -> Option<(String, usize)> {
        let locator = self
            .locate
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        locator.and_then(|locate| locate(position))
    }
}

/// A socket a tool asks for a running program's values on, and pauses it
/// at breakpoints through. Each engine attached to it answers: a runner
/// compiles each generation of the program on an engine of its own.
#[derive(Clone)]
pub struct Inspector(Arc<Shared>);

impl Inspector {
    /// Serve requests on a Unix socket at `socket`, replacing a stale one
    /// there. Nothing answers them until an engine is attached. The
    /// breakpoints are kept beside the socket, so a program started again
    /// there has them.
    ///
    /// # Errors
    ///
    /// When the socket can't be bound.
    #[cfg(unix)]
    pub fn bind(socket: &std::path::Path) -> std::io::Result<Self> {
        use std::os::unix::net::UnixListener;

        let _ = std::fs::remove_file(socket);
        let listener = UnixListener::bind(socket)?;
        let saved = socket.with_file_name("breakpoints.json");
        let points = std::fs::read(&saved)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Vec<Value>>(&bytes).ok())
            .unwrap_or_default()
            .iter()
            .filter_map(stops::Spec::from_json)
            .map(|spec| stops::Point {
                spec,
                at: Vec::new(),
            })
            .collect();
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                points,
                ..State::default()
            }),
            changed: Condvar::new(),
            wake: Arc::default(),
            breaks: Arc::default(),
            stepping: AtomicBool::new(false),
            answering: AtomicBool::new(false),
            one: Mutex::new(()),
            locate: Mutex::new(None),
            saved,
        });
        let listening = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("rawr-inspect".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let shared = Arc::clone(&listening);
                    let _ = std::thread::Builder::new()
                        .name("rawr-inspect-request".into())
                        .spawn(move || shared.handle(stream));
                }
            })?;
        Ok(Self(shared))
    }

    /// Answer requests from the program `engine` runs, and pause it at the
    /// breakpoints. Attach before the program compiles, so the compiler
    /// keeps what reading needs (#526).
    pub fn attach(&self, engine: &mut Engine) {
        let shared = Arc::clone(&self.0);
        engine.wake_debugger_on(Arc::clone(&shared.wake));
        engine.break_debugger_at(Arc::clone(&shared.breaks));
        #[allow(deprecated)]
        engine.register_debugger(
            |_, debugger| debugger,
            move |mut context, event, node, _source, position| {
                Ok(shared.on_step(&mut context, event, node, position))
            },
        );
    }

    /// Place the program's steps with `locate` from now on: the positions
    /// of the program that runs next.
    pub fn locate_with(
        &self,
        locate: impl Fn(Position) -> Option<(String, usize)> + Send + Sync + 'static,
    ) {
        *self.0.locate.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(locate));
    }

    /// The program that runs next compiled as `ast`, its steps placed by
    /// [`Self::locate_with`]: its breakpoints move to where their lines are
    /// now, and those whose line is gone are removed.
    pub fn compiled(&self, ast: &crate::AST) {
        let shared = &self.0;
        let stops = Arc::new(stops::Stops::of(ast, &|position| shared.locate(position)));
        let mut state = shared.state();
        state.points = state
            .points
            .iter()
            .filter_map(|point| stops::moved(point, &stops))
            .collect();
        state.stops = Some(stops);
        shared.armed(&state);
        shared.save(&state);
    }
}

/// How deep in calls the program is where `context` is.
fn depth(context: &EvalContext) -> usize {
    context
        .global_runtime_state()
        .debugger
        .as_ref()
        .map_or(0, |debugger| debugger.call_stack().len())
}

impl Shared {
    /// Take one request from `stream` and answer it.
    #[cfg(unix)]
    fn handle(&self, stream: std::os::unix::net::UnixStream) {
        use std::io::{BufRead as _, BufReader, Write as _};

        let mut line = String::new();
        let Ok(reading) = stream.try_clone() else {
            return;
        };
        if BufReader::new(reading).read_line(&mut line).is_err() {
            return;
        }
        let request: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
        let timeout = request
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .map_or(TIMEOUT, Duration::from_millis);
        let answer = match request.get("op").and_then(Value::as_str) {
            Some("break") => self.set(&request, timeout),
            Some("clear") => self.clear(&request),
            Some("continue") => self.resume(Resume::Continue, timeout),
            Some("step") => self.resume(
                Resume::Step(match request.get("kind").and_then(Value::as_str) {
                    Some("into") => Step::Into,
                    Some("out") => Step::Out,
                    _ => Step::Over,
                }),
                timeout,
            ),
            _ => self.inspect(&request, timeout),
        };
        let mut stream = stream;
        let _ = writeln!(stream, "{answer}");
    }

    /// Read values: answered at the program's next matching step, or where
    /// it is paused.
    fn inspect(&self, request: &Value, timeout: Duration) -> Value {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let text = |key: &str| request.get(key).and_then(Value::as_str).map(str::to_owned);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let (reply, answer) = mpsc::channel();
        self.state().pending.push(Request {
            id,
            name: text("name"),
            file: text("file"),
            reply,
        });
        self.wake.store(true, Ordering::Release);
        self.changed.notify_all();
        answer.recv_timeout(timeout).unwrap_or_else(|_| {
            let mut state = self.state();
            state.pending.retain(|request| request.id != id);
            if state.pending.is_empty() {
                self.wake.store(false, Ordering::Release);
            }
            drop(state);
            // An answer made while the request was being given up.
            answer
                .try_recv()
                .unwrap_or_else(|_| json!({ "reached": false }))
        })
    }

    /// Set a breakpoint, then wait for the program to pause, there or at
    /// another one.
    fn set(&self, request: &Value, timeout: Duration) -> Value {
        let spec = match (
            request.get("function").and_then(Value::as_str),
            request.get("file").and_then(Value::as_str),
            request
                .get("line")
                .and_then(Value::as_u64)
                .and_then(|line| usize::try_from(line).ok()),
        ) {
            (Some(name), ..) => stops::Spec::Function {
                name: name.to_owned(),
            },
            (None, Some(file), Some(line)) => stops::Spec::Line {
                file: file.to_owned(),
                line,
                text: String::new(),
            },
            _ => {
                return json!({ "error": "a breakpoint needs a `file` and a `line`, or a `function`" })
            }
        };
        let mut state = self.state();
        let point = match stops::set(spec, state.stops.as_deref()) {
            Ok(point) => point,
            Err(error) => return json!({ "error": error }),
        };
        let compiled = state.stops.is_some();
        let lines = lines(&state, &point);
        let spec = point.spec.to_json();
        state.points.retain(|held| held.spec != point.spec);
        state.points.push(point);
        self.armed(&state);
        self.save(&state);
        let seen = state.pauses;
        let paused = if state.paused.is_some() {
            state.paused.clone()
        } else {
            self.next_pause(state, seen, timeout)
        };
        json!({ "set": spec, "lines": lines, "compiled": compiled, "paused": paused })
    }

    /// Remove the breakpoints a request names: all of them when it names
    /// none.
    fn clear(&self, request: &Value) -> Value {
        let file = request.get("file").and_then(Value::as_str);
        let line = request
            .get("line")
            .and_then(Value::as_u64)
            .and_then(|line| usize::try_from(line).ok());
        let function = request.get("function").and_then(Value::as_str);
        let mut state = self.state();
        let before = state.points.len();
        state
            .points
            .retain(|point| !point.spec.cleared_by(file, line, function));
        let cleared = before - state.points.len();
        self.armed(&state);
        self.save(&state);
        json!({
            "cleared": cleared,
            "left": state.points.iter().map(|point| point.spec.to_json()).collect::<Vec<_>>(),
            "paused": state.paused,
        })
    }

    /// Resume a paused program, then wait for it to pause again.
    fn resume(&self, resume: Resume, timeout: Duration) -> Value {
        let mut state = self.state();
        if state.paused.is_none() {
            return json!({ "error": "not paused" });
        }
        let seen = state.pauses;
        state.resume = Some(resume);
        self.changed.notify_all();
        let paused = self.next_pause(state, seen, timeout);
        json!({ "paused": paused })
    }

    /// Wait, up to `timeout`, for a pause after the `seen`th; where it
    /// paused.
    fn next_pause(
        &self,
        mut state: std::sync::MutexGuard<'_, State>,
        seen: u64,
        timeout: Duration,
    ) -> Option<Value> {
        let deadline = std::time::Instant::now() + timeout;
        while state.pauses == seen {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return None;
            }
            state = self
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        Some(state.last.clone())
    }

    /// Break at every breakpoint's steps.
    fn armed(&self, state: &State) {
        self.breaks.set(
            state
                .points
                .iter()
                .flat_map(|point| point.at.iter().copied()),
        );
    }

    /// Keep the breakpoints for a program started again.
    fn save(&self, state: &State) {
        let specs: Vec<Value> = state
            .points
            .iter()
            .map(|point| point.spec.to_json())
            .collect();
        let _ = std::fs::write(&self.saved, Value::from(specs).to_string());
    }

    /// At a step of the program: answer what waits, and pause at a
    /// breakpoint or where a step ends. The command keeps a step going.
    fn on_step(
        &self,
        context: &mut EvalContext,
        event: DebuggerEvent,
        node: crate::ast::ASTNode,
        position: Position,
    ) -> DebuggerCommand {
        if self.answering.load(Ordering::Acquire) {
            return DebuggerCommand::Continue;
        }
        if self.wake.load(Ordering::Acquire) {
            self.answer(context, position);
        }
        match event {
            DebuggerEvent::Break => {
                if let Some(stop) = self.stop(position, &node) {
                    self.pause(context, position, stop, "breakpoint");
                }
            }
            DebuggerEvent::Start if self.stepping.load(Ordering::Acquire) => {
                // Native code called into the program again.
                if let Some(stepping) = self.state().stepping.as_mut() {
                    stepping.anywhere = true;
                }
            }
            DebuggerEvent::Step if self.stepping.load(Ordering::Acquire) => {
                if let Some(stop) = self.stop(position, &node) {
                    let reason = if self.breaks.contains(position) {
                        Some("breakpoint")
                    } else {
                        let depth = depth(context);
                        self.state()
                            .stepping
                            .as_ref()
                            .is_some_and(|stepping| ends(stepping, depth, position, &stop))
                            .then_some("step")
                    };
                    if let Some(reason) = reason {
                        self.pause(context, position, stop, reason);
                    }
                }
            }
            _ => {}
        }
        if self.stepping.load(Ordering::Acquire) {
            DebuggerCommand::StepInto
        } else {
            DebuggerCommand::Continue
        }
    }

    /// The statement a step is at, if it is one the program wrote.
    fn stop(&self, position: Position, node: &crate::ast::ASTNode) -> Option<stops::Stop> {
        self.state().stops.as_ref()?.get(position, node).cloned()
    }

    /// Hold the program at `stop` until a tool resumes it, answering reads
    /// meanwhile.
    fn pause(
        &self,
        context: &mut EvalContext,
        position: Position,
        stop: stops::Stop,
        reason: &str,
    ) {
        let _one = self.one.lock().unwrap_or_else(PoisonError::into_inner);
        let at = json!({
            "file": stop.file,
            "line": stop.line,
            "function": function(context),
            "reason": reason,
        });
        let mut state = self.state();
        state.stepping = None;
        self.stepping.store(false, Ordering::Release);
        state.paused = Some(at.clone());
        state.last = at;
        state.pauses += 1;
        state.resume = None;
        self.changed.notify_all();
        let location = (stop.file.clone(), stop.line);
        let resume = loop {
            if !state.pending.is_empty() {
                let pending = std::mem::take(&mut state.pending);
                self.wake.store(false, Ordering::Release);
                let paused = state.paused.clone();
                drop(state);
                self.answering.store(true, Ordering::Release);
                for request in pending {
                    let here = request.file.as_deref().is_none_or(|file| file == stop.file)
                        && request
                            .name
                            .as_deref()
                            .is_none_or(|name| in_scope(context, name));
                    if here {
                        reply(context, &request, Some(&location));
                    } else {
                        let _ = request
                            .reply
                            .send(json!({ "reached": false, "paused": paused }));
                    }
                }
                self.answering.store(false, Ordering::Release);
                state = self.state();
                continue;
            }
            if let Some(resume) = state.resume.take() {
                break resume;
            }
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        };
        state.paused = None;
        if let Resume::Step(step) = resume {
            state.stepping = Some(Stepping {
                step,
                depth: depth(context),
                position,
                file: stop.file,
                line: stop.line,
                anywhere: false,
            });
            self.stepping.store(true, Ordering::Release);
        }
    }

    /// At a step of the program: answer each waiting request this step
    /// matches.
    fn answer(&self, context: &mut EvalContext, position: Position) {
        if self.answering.swap(true, Ordering::AcqRel) {
            return;
        }
        let locate = |position| self.locate(position);
        let mut location = None;
        let mut located = false;
        let mut matched = Vec::new();
        {
            let mut state = self.state();
            let pending = &mut state.pending;
            let mut index = 0;
            while index < pending.len() {
                let request = &pending[index];
                let named = request
                    .name
                    .as_deref()
                    .is_none_or(|name| in_scope(context, name));
                let here = named
                    && request.file.as_deref().is_none_or(|file| {
                        if !located {
                            location = locate(position);
                            located = true;
                        }
                        location.as_ref().is_some_and(|(at, _)| at == file)
                    });
                if here {
                    matched.push(pending.swap_remove(index));
                } else {
                    index += 1;
                }
            }
            if pending.is_empty() {
                self.wake.store(false, Ordering::Release);
            }
        }
        if !matched.is_empty() {
            if !located {
                location = locate(position);
            }
            for request in matched {
                reply(context, &request, location.as_ref());
            }
        }
        self.answering.store(false, Ordering::Release);
    }
}

/// The lines a breakpoint pauses on.
fn lines(state: &State, point: &stops::Point) -> Vec<Value> {
    let Some(stops) = &state.stops else {
        return Vec::new();
    };
    let mut lines: Vec<Value> = point
        .at
        .iter()
        .filter_map(|position| stops.located(*position))
        .map(|(file, line)| json!({ "file": file, "line": line }))
        .collect();
    lines.dedup();
    lines
}

/// Whether `stepping` ends at a statement at `depth` and `position`.
fn ends(stepping: &Stepping, depth: usize, position: Position, stop: &stops::Stop) -> bool {
    if stepping.anywhere {
        return true;
    }
    // Another line, or this one again as a loop runs it.
    let moved =
        stop.file != stepping.file || stop.line != stepping.line || position <= stepping.position;
    match stepping.step {
        Step::Into => depth != stepping.depth || moved,
        Step::Over => depth < stepping.depth || depth == stepping.depth && moved,
        Step::Out => depth < stepping.depth,
    }
}

/// Answer `request` with the values where `context` is.
fn reply(context: &mut EvalContext, request: &Request, location: Option<&(String, usize)>) {
    let found = match &request.name {
        Some(name) => binding(context, name).into_iter().collect(),
        None => bindings(context),
    };
    let _ = request.reply.send(json!({
        "reached": true,
        "file": location.map(|(file, _)| file),
        "line": location.map(|(_, line)| line),
        "function": function(context),
        "bindings": found.iter().map(Binding::to_json).collect::<Vec<_>>(),
    }));
}

#[cfg(test)]
mod tests {
    use super::{short_type_name, undebuggable};
    use crate::Dynamic;

    #[derive(Clone)]
    struct Opaque;

    #[test]
    fn a_crate_value_without_debug_is_named_inside_what_holds_it() {
        let named = |value: &Dynamic| undebuggable(value).map(short_type_name);
        assert_eq!(named(&Dynamic::from(Opaque)), Some("Opaque".to_owned()));
        let held = Dynamic::from_array(vec![Dynamic::from(1_i64), Dynamic::from(Opaque)]);
        assert_eq!(named(&held), Some("Opaque".to_owned()));
        let some = crate::option::some(Dynamic::from(Opaque));
        assert_eq!(named(&some), Some("Opaque".to_owned()));
        assert_eq!(named(&Dynamic::from(1_u8)), None);
        assert_eq!(named(&crate::option::none()), None);
    }

    #[test]
    fn type_names_are_trimmed_as_rustc_writes_them() {
        assert_eq!(short_type_name("app::Window"), "Window");
        assert_eq!(
            short_type_name("app::Window<core::option::Option<app::Id>>"),
            "Window<Option<Id>>"
        );
        assert_eq!(short_type_name("(a::B, c::D)"), "(B, D)");
    }
}
