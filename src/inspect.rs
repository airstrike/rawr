//! Reading a running program's values (D557, #488). A tool connects to a
//! socket the program serves and asks for the bindings in scope where the
//! program is, or for one binding by name, optionally in one file. The
//! program answers at its next step in its own code, where the debugger
//! callback runs: each value as Rust's `{:?}` prints it, through the
//! program's own `Debug` impls and derives and a crate type's `Debug`.
//!
//! The protocol is one JSON line each way:
//! - the request: `{"name": "count", "file": "/abs/src/main.rs", "frame": 1}`,
//!   each field optional. `frame` picks a call: 0
//!   (the default) for the function the program is in, 1 for its caller,
//!   and on (#511); `name` and `file` apply to that call;
//! - the answer: `{"reached": true, "frame": .., "file": .., "line": ..,
//!   "function": .., "bindings": [{"name": .., "value": ..} or {"name": ..,
//!   "error": ..}], "frames": [{"function": .., "file": .., "line": ..}]}`,
//!   the frames innermost first, a caller's line the line of its call; or
//!   `{"reached": false, "paused": .., "depth": ..}` when the program is
//!   paused where the request doesn't match. A request waits for its step
//!   as long as that takes; the tool closing the socket gives it up.
//!
//! A `&mut` parameter or `self`, which the compiled function holds as its
//! receiver, reads under the name the program gave it (#534). A caller's
//! bindings are read where its call keeps them, while it runs: the
//! debugger's call stack records where, and only while a debugger is
//! registered (D675.5).
//!
//! Nothing runs while no request waits: the engine checks one flag at each
//! step.
//!
//! The same socket sets breakpoints and resumes a paused program (#489),
//! with an `op`:
//! - `{"op": "break", "file": .., "line": ..}` or `{"op": "break",
//!   "function": "tick"}`: `{"set": .., "lines": [{"file": .., "line": ..}],
//!   "compiled": .., "paused": {"file": .., "line": .., "function": ..,
//!   "reason": ..} or null}`, the pause the program is in if any, or
//!   `{"error": ..}`;
//! - `{"op": "clear"}`, with `file`, `line` or `function` to take fewer:
//!   `{"cleared": n, "left": [..], "paused": ..}`;
//! - `{"op": "step", "kind": "over" | "into" | "out"}` on a paused program,
//!   answered where the step pauses: `{"paused": ..}`; `{"op": "continue"}`,
//!   answered once it runs on: `{"paused": null}`; either `{"error": "not
//!   paused"}` on a program that isn't.
//!
//! It also sends the program's window input (#989), with `{"op": "input",
//! "action": ..}`: a pointer move, a click, a drag, a scroll, a key press or
//! typed text, answered once the window drew it; `{"error": "paused",
//! "paused": ..}` while it is paused, with `"taken": true` when the input
//! made it pause.
//!
//! Under `rawr mcp`, a pause and its end are also told as `paused` (with
//! the file, line, function and reason) and `resumed` events in the file
//! `RAWR_MCP_DEBUG_EVENTS` names, so `wait` and `status` say so (#535).
//!
//! A pause holds the thread that runs the program's code inside the
//! debugger callback. Native code that called into the program, such as a
//! window's event loop, waits in that call until the program goes on. A
//! step pauses at the next statement the program wrote. It can't stop in
//! a crate's native code, so a step out of a function that native code
//! called pauses at the program's next call from it.
#![cfg(feature = "debugging")]

mod input;
mod stops;

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use serde_json::{json, Value};

use crate::debugger::{BreakPositions, DebuggerCommand, DebuggerEvent};
use crate::types::dynamic::Union;
use crate::{Dynamic, Engine, EvalAltResult, EvalContext, Position};

/// How often a waiting request looks whether its tool went away.
const POLL: Duration = Duration::from_millis(50);

/// The variable naming the file `rawr mcp` reads a supervised run's events
/// from, one JSON object per line, where the program says where it paused
/// and when it went on. The process that runs the program has it, a
/// runner or the program's own.
pub const EVENTS: &str = "RAWR_MCP_DEBUG_EVENTS";

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

/// The program's own function calls where `context` is: their indices in
/// the debugger's call stack, the innermost first. A crate's native calls
/// between them are left out.
fn script_frames(context: &EvalContext) -> Vec<usize> {
    let Some(debugger) = context.global_runtime_state().debugger.as_ref() else {
        return Vec::new();
    };
    let stack = debugger.call_stack();
    (0..stack.len())
        .rev()
        .filter(|&index| stack[index].bindings.is_some())
        .collect()
}

/// How many frames the program has where `context` is: its function calls,
/// or one for code outside any function.
fn depth_of(context: &EvalContext) -> usize {
    script_frames(context).len().max(1)
}

/// One binding of a frame: its name, and its value or why it can't be read
/// there.
type Entry<'c> = (&'c str, Result<&'c Dynamic, String>);

/// The scope frame `frame` keeps its bindings in where `context` is, and
/// where its own entries end in it. `None` when it can't be read there.
fn scope_of<'c>(
    context: &'c EvalContext,
    stack: &'c [crate::debugger::CallStackFrame],
    frames: &[usize],
    frame: usize,
) -> Option<(&'c crate::Scope<'c>, usize)> {
    if frame == 0 {
        return Some((context.scope(), usize::MAX));
    }
    let index = *frames.get(frame)?;
    let call = stack.get(index + 1)?;
    match call.caller.as_ref()?.scope {
        // SAFETY: the call at `index + 1` is on the call stack, so it runs
        // on this thread below the step `context` is at. Its caller lent
        // it this pointer after its last use of its scope before the call,
        // and uses it again only when the call returns (`Lent`), so no
        // live `&mut` aliases it. The reference ends with `context`'s
        // borrow, inside this step's callback.
        Some(scope) => Some((
            unsafe { scope.cast::<crate::Scope<'c>>().as_ref() },
            usize::MAX,
        )),
        // The call extended this frame's scope: it is the next frame in,
        // whose entries start where this frame's end.
        None if frames.get(frame - 1) == Some(&(index + 1)) => {
            let end = call.bindings.as_ref()?.start;
            scope_of(context, stack, frames, frame - 1).map(|(scope, _)| (scope, end))
        }
        None => None,
    }
}

/// The entries of frame `frame` where `context` is (0 is the function the
/// program is in, 1 its caller, and on), the newest first: its scope's
/// entries, then its receiver under the name the program gave it (#534).
/// A caller's bindings are read where the call it made was lent them
/// (#540); one that call holds by reference reads as borrowed, as Rust
/// would have it, and a caller that lent nothing (native code called the
/// next frame in) has none to read. `None` when the program isn't that
/// deep.
fn entries<'c>(context: &'c EvalContext, frame: usize) -> Option<Vec<Entry<'c>>> {
    let frames = script_frames(context);
    let stack = context
        .global_runtime_state()
        .debugger
        .as_ref()
        .map_or(&[][..], |debugger| debugger.call_stack());
    let own = |index: usize| stack.get(*frames.get(index)?)?.bindings.as_ref();
    let mut found: Vec<Entry<'c>> = Vec::new();
    if frame == 0 {
        found.extend(
            context
                .scope()
                .iter_rev_inner()
                .map(|(name, _, value)| (name.as_str(), Ok(value))),
        );
        if let (Some(name), Some(this)) = (
            own(0).and_then(|bindings| bindings.this_name.as_ref()),
            context.this_ptr(),
        ) {
            found.push((name.as_str(), Ok(this)));
        }
        return Some(found);
    }
    let bindings = own(frame)?;
    let index = frames[frame];
    let call = stack.get(index + 1)?;
    let (Some(lent), Some((scope, end))) = (
        call.caller.as_ref(),
        scope_of(context, stack, &frames, frame),
    ) else {
        return Some(found);
    };
    let callee = crate::resolve::lower_nominal::function_display_name(&call.fn_name);
    let borrowed = |name: &str| format!("`{name}` is borrowed by the call to `{callee}`");
    let places: Vec<(&'c str, *const Dynamic)> = scope
        .places()
        .enumerate()
        .filter(|(index, _)| (bindings.start..end).contains(index))
        .map(|(_, (name, place))| (name.as_str(), place))
        .collect();
    let held = match lent.held {
        Some(crate::eval::Held::Local(place)) => Some(place),
        _ => None,
    };
    // A place inside one of the frame's values, or inside its receiver:
    // which one isn't known, so the whole frame reads as borrowed.
    let all_held = held.is_some_and(|held| places.iter().all(|&(_, place)| place != held));
    for &(name, place) in places.iter().rev() {
        if all_held || held == Some(place) {
            found.push((name, Err(borrowed(name))));
        } else {
            // SAFETY: as for the scope in `scope_of`; the one place the
            // call holds by reference is not read.
            found.push((name, Ok(unsafe { &*place })));
        }
    }
    if let Some(name) = bindings.this_name.as_ref() {
        if all_held || lent.held == Some(crate::eval::Held::This) {
            found.push((name.as_str(), Err(borrowed(name))));
        } else if let Some(this) = lent.this {
            // SAFETY: as for the scope in `scope_of`.
            found.push((name.as_str(), Ok(unsafe { this.as_ref() })));
        }
    }
    Some(found)
}

/// The bindings of frame `frame` where `context` is, the last of each name,
/// in the order they were made; or one binding, `name`. `None` when the
/// program isn't that deep.
fn read(context: &mut EvalContext, frame: usize, name: Option<&str>) -> Option<Vec<Binding>> {
    let mut seen = HashSet::new();
    let mut found: Vec<(String, Result<Dynamic, String>)> = entries(context, frame)?
        .into_iter()
        .filter(|(entry, _)| {
            name.map_or_else(|| is_written(entry), |name| *entry == name)
                && seen.insert(entry.to_string())
        })
        .map(|(entry, value)| (entry.to_owned(), value.map(Dynamic::flatten_clone)))
        .collect();
    found.reverse();
    Some(
        found
            .into_iter()
            .map(|(name, value)| Binding {
                value: value.and_then(|value| debug_text(context, &value)),
                name,
            })
            .collect(),
    )
}

/// The bindings in scope where `context` is, the last of each name, in the
/// order they were made.
pub fn bindings(context: &mut EvalContext) -> Vec<Binding> {
    read(context, 0, None).unwrap_or_default()
}

/// The binding `name` in scope where `context` is.
pub fn binding(context: &mut EvalContext, name: &str) -> Option<Binding> {
    read(context, 0, Some(name))?.pop()
}

/// Whether `name` is in scope in frame `frame` where `context` is.
fn in_scope(context: &EvalContext, frame: usize, name: &str) -> bool {
    entries(context, frame).is_some_and(|entries| entries.iter().any(|(entry, _)| *entry == name))
}

/// The function the program is in where `context` is, as Rust names it.
#[must_use]
pub fn function(context: &EvalContext) -> Option<String> {
    frame_function(context, 0)
}

/// The function of frame `frame` where `context` is, as Rust names it.
fn frame_function(context: &EvalContext, frame: usize) -> Option<String> {
    let index = *script_frames(context).get(frame)?;
    let call = &context
        .global_runtime_state()
        .debugger
        .as_ref()?
        .call_stack()[index];
    Some(crate::resolve::lower_nominal::function_display_name(&call.fn_name).to_owned())
}

/// Where frame `frame` is when `context` is at `position`: there for the
/// innermost, and for a caller, where it made the call the next frame up
/// came from.
fn frame_position(context: &EvalContext, frame: usize, position: Position) -> Option<Position> {
    if frame == 0 {
        return Some(position);
    }
    let index = *script_frames(context).get(frame)?;
    let stack = context
        .global_runtime_state()
        .debugger
        .as_ref()?
        .call_stack();
    Some(stack.get(index + 1)?.pos)
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
    /// Which frame: 0 for the function the program is in, 1 for its
    /// caller, and on (#511).
    frame: usize,
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
    /// The file a supervising `rawr mcp` reads the program's events from,
    /// where a pause and its end are told (#535).
    events: Option<std::path::PathBuf>,
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
            events: std::env::var_os(EVENTS)
                .filter(|path| !path.is_empty())
                .map(std::path::PathBuf::from),
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

/// Whether the tool on `stream` went away: it sends one request line and
/// then only waits, so a read that sees the end is its leaving.
#[cfg(unix)]
fn gone(stream: &std::os::unix::net::UnixStream) -> bool {
    use std::io::Read as _;

    if stream.set_nonblocking(true).is_err() {
        return true;
    }
    let mut byte = [0_u8; 1];
    let gone = match (&*stream).read(&mut byte) {
        Ok(0) => true,
        Ok(_) => false,
        Err(error) => error.kind() != std::io::ErrorKind::WouldBlock,
    };
    stream.set_nonblocking(false).is_err() || gone
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
        let gone = || gone(&stream);
        let answer = match request.get("op").and_then(Value::as_str) {
            Some("input") => self.input(&request, &gone),
            Some("break") => self.set(&request),
            Some("clear") => self.clear(&request),
            Some("continue") => self.resume(Resume::Continue, &gone),
            Some("step") => self.resume(
                Resume::Step(match request.get("kind").and_then(Value::as_str) {
                    Some("into") => Step::Into,
                    Some("out") => Step::Out,
                    _ => Step::Over,
                }),
                &gone,
            ),
            _ => self.inspect(&request, &gone),
        };
        let mut stream = stream;
        let _ = writeln!(stream, "{answer}");
    }

    /// Send the program's window an input. A paused program holds its
    /// event loop, so it takes none until it goes on; an input that makes it
    /// pause is answered with the pause.
    fn input(&self, request: &Value, gone: &dyn Fn() -> bool) -> Value {
        if let Some(paused) = self.state().paused.clone() {
            return json!({ "error": "paused", "paused": paused });
        }
        input::answer(request, &|| self.state().paused.is_some() || gone()).unwrap_or_else(
            || json!({ "error": "paused", "paused": self.state().paused.clone(), "taken": true }),
        )
    }

    /// Read values: answered at the program's next matching step, or where
    /// it is paused, unless the tool goes away first.
    fn inspect(&self, request: &Value, gone: &dyn Fn() -> bool) -> Value {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let text = |key: &str| request.get(key).and_then(Value::as_str).map(str::to_owned);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let (reply, answer) = mpsc::channel();
        self.state().pending.push(Request {
            id,
            name: text("name"),
            file: text("file"),
            frame: request
                .get("frame")
                .and_then(Value::as_u64)
                .and_then(|frame| usize::try_from(frame).ok())
                .unwrap_or(0),
            reply,
        });
        self.wake.store(true, Ordering::Release);
        self.changed.notify_all();
        loop {
            match answer.recv_timeout(POLL) {
                Ok(value) => return value,
                Err(mpsc::RecvTimeoutError::Timeout) if !gone() => {}
                Err(_) => break,
            }
        }
        {
            let mut state = self.state();
            state.pending.retain(|request| request.id != id);
            if state.pending.is_empty() {
                self.wake.store(false, Ordering::Release);
            }
            drop(state);
        }
        // An answer made while the request was being given up.
        answer
            .try_recv()
            .unwrap_or_else(|_| json!({ "reached": false }))
    }

    /// Set a breakpoint. The program pauses when it reaches it; the answer
    /// has the pause it is in already, if any.
    fn set(&self, request: &Value) -> Value {
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
        json!({ "set": spec, "lines": lines, "compiled": compiled, "paused": state.paused })
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

    /// Resume a paused program. A step is answered where it pauses, unless
    /// the tool goes away first; a continue once the program runs on.
    fn resume(&self, resume: Resume, gone: &dyn Fn() -> bool) -> Value {
        let mut state = self.state();
        if state.paused.is_none() {
            return json!({ "error": "not paused" });
        }
        let seen = state.pauses;
        let step = matches!(resume, Resume::Step(_));
        state.resume = Some(resume);
        self.changed.notify_all();
        if !step {
            return json!({ "paused": null });
        }
        while state.pauses == seen {
            if gone() {
                return json!({ "paused": null });
            }
            state = self
                .changed
                .wait_timeout(state, POLL)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        json!({ "paused": state.last })
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
        // A native call's end has no bindings of the program's to read.
        if self.wake.load(Ordering::Acquire)
            && !matches!(
                event,
                DebuggerEvent::FunctionExitWithValue(_) | DebuggerEvent::FunctionExitWithError(_)
            )
        {
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
        let mut event = at.clone();
        event["event"] = json!("paused");
        self.emit(&event);
        state.paused = Some(at.clone());
        state.last = at;
        state.pauses += 1;
        state.resume = None;
        self.changed.notify_all();
        let resume = loop {
            if !state.pending.is_empty() {
                let pending = std::mem::take(&mut state.pending);
                self.wake.store(false, Ordering::Release);
                let paused = state.paused.clone();
                drop(state);
                self.answering.store(true, Ordering::Release);
                for request in pending {
                    if self.matches(context, &request, position) {
                        self.reply(context, &request, position);
                    } else {
                        let _ = request.reply.send(json!({
                            "reached": false,
                            "paused": paused,
                            "depth": depth_of(context),
                        }));
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
        self.emit(&json!({ "event": "resumed" }));
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
        let mut matched = Vec::new();
        {
            let mut state = self.state();
            let pending = &mut state.pending;
            let mut index = 0;
            while index < pending.len() {
                if self.matches(context, &pending[index], position) {
                    matched.push(pending.swap_remove(index));
                } else {
                    index += 1;
                }
            }
            if pending.is_empty() {
                self.wake.store(false, Ordering::Release);
            }
        }
        for request in matched {
            self.reply(context, &request, position);
        }
        self.answering.store(false, Ordering::Release);
    }

    /// Whether `request` reads where `context` is, at `position`: the
    /// program is as deep as its frame, which is in its file and has its
    /// binding.
    fn matches(&self, context: &EvalContext, request: &Request, position: Position) -> bool {
        request.frame < depth_of(context)
            && request
                .name
                .as_deref()
                .is_none_or(|name| in_scope(context, request.frame, name))
            && request.file.as_deref().is_none_or(|file| {
                self.frame_line(context, request.frame, position)
                    .is_some_and(|(at, _)| at == file)
            })
    }

    /// The file and line frame `frame` is at where `context` is, at
    /// `position`.
    fn frame_line(
        &self,
        context: &EvalContext,
        frame: usize,
        position: Position,
    ) -> Option<(String, usize)> {
        self.locate(frame_position(context, frame, position)?)
    }

    /// Answer `request` with the values of its frame where `context` is, at
    /// `position`, and where each frame is.
    fn reply(&self, context: &mut EvalContext, request: &Request, position: Position) {
        let frame = request.frame;
        let location = self.frame_line(context, frame, position);
        let frames: Vec<Value> = (0..depth_of(context))
            .map(|index| {
                let at = self.frame_line(context, index, position);
                json!({
                    "function": frame_function(context, index),
                    "file": at.as_ref().map(|(file, _)| file),
                    "line": at.as_ref().map(|(_, line)| line),
                })
            })
            .collect();
        let function = frame_function(context, frame);
        let found = read(context, frame, request.name.as_deref()).unwrap_or_default();
        let _ = request.reply.send(json!({
            "reached": true,
            "frame": frame,
            "file": location.as_ref().map(|(file, _)| file),
            "line": location.as_ref().map(|(_, line)| line),
            "function": function,
            "bindings": found.iter().map(Binding::to_json).collect::<Vec<_>>(),
            "frames": frames,
        }));
    }

    /// Tell a supervising `rawr mcp` `event`, as a line of its events file.
    fn emit(&self, event: &Value) {
        use std::io::Write as _;
        let Some(path) = &self.events else {
            return;
        };
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = file.write_all(format!("{event}\n").as_bytes());
        }
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

#[cfg(test)]
mod tests {
    use super::{depth_of, frame_function, read, short_type_name, undebuggable};
    use crate::Dynamic;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    /// What `look` saw at each step of `source`'s function `function`,
    /// with a debugger registered before it compiled.
    fn at_steps(
        source: &str,
        function: &'static str,
        look: impl Fn(&mut crate::EvalContext) -> String + Send + Sync + 'static,
    ) -> Vec<String> {
        let mut engine = crate::Engine::new();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seeing = Arc::clone(&seen);
        let looking = AtomicBool::new(false);
        #[allow(deprecated)]
        engine.register_debugger(
            |_, mut debugger| {
                debugger.status = crate::eval::DebuggerStatus::STEP;
                debugger
            },
            move |mut context, event, _, _, _| {
                // The `Debug` code a read runs steps too.
                if matches!(event, crate::debugger::DebuggerEvent::Step)
                    && super::function(&context).is_some_and(|name| name.ends_with(function))
                    && !looking.swap(true, Ordering::AcqRel)
                {
                    let saw = look(&mut context);
                    seeing.lock().unwrap().push(saw);
                    looking.store(false, Ordering::Release);
                }
                Ok(crate::debugger::DebuggerCommand::StepInto)
            },
        );
        engine
            .eval::<i64>(source)
            .unwrap_or_else(|error| panic!("{error}"));
        seen.lock().unwrap().clone()
    }

    fn shown(context: &mut crate::EvalContext, frame: usize) -> String {
        read(context, frame, None).map_or_else(
            || "none".to_owned(),
            |found| {
                found
                    .iter()
                    .map(|binding| {
                        format!(
                            "{}={}",
                            binding.name,
                            binding
                                .value
                                .clone()
                                .unwrap_or_else(|error| format!("<{error}>"))
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            },
        )
    }

    #[test]
    fn a_mut_parameter_and_self_read_under_the_names_the_program_gave_them() {
        let seen = at_steps(
            "fn tick(count: &mut u64, step: u64) { *count += step; }\n\
             pub fn run() -> i64 { let mut n: u64 = 3; tick(&mut n, 2); n as i64 }\n\
             run()",
            "tick",
            |context| shown(context, 0),
        );
        assert!(seen.iter().any(|saw| saw == "count=3 step=2"), "{seen:?}");
        let seen = at_steps(
            "struct Counter { count: i64 }\n\
             impl Counter { fn bump(&mut self, by: i64) { self.count += by; } }\n\
             pub fn run() -> i64 { let mut c = Counter { count: 1 }; c.bump(4); c.count }\n\
             run()",
            "bump",
            |context| shown(context, 0),
        );
        assert!(
            seen.iter()
                .any(|saw| saw == "self=Counter { count: 1 } by=4"),
            "{seen:?}"
        );
    }

    #[test]
    fn each_caller_frame_reads_its_own_bindings_while_its_call_runs() {
        let seen = at_steps(
            "fn inner(x: i64) -> i64 { x * 2 }\n\
             fn outer(a: i64) -> i64 { let b = a + 1; inner(b) }\n\
             pub fn run() -> i64 { let n = 5; let m = outer(n); m }\n\
             run()",
            "inner",
            |context| {
                let frames = (0..=depth_of(context))
                    .map(|frame| {
                        format!(
                            "{}: {}",
                            frame_function(context, frame).unwrap_or_default(),
                            shown(context, frame)
                        )
                    })
                    .collect::<Vec<_>>();
                frames.join(" | ")
            },
        );
        assert!(
            seen.iter()
                .any(|saw| saw == "inner: x=6 | outer: a=5 b=6 | run: n=5 | : none"),
            "{seen:?}"
        );
    }

    /// Each frame's function and bindings where `context` is.
    fn every_frame(context: &mut crate::EvalContext) -> String {
        (0..depth_of(context))
            .map(|frame| {
                format!(
                    "{}: {}",
                    frame_function(context, frame).unwrap_or_default(),
                    shown(context, frame)
                )
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }

    #[test]
    fn a_caller_binding_a_running_call_holds_reads_as_borrowed() {
        let seen = at_steps(
            "struct Counter { count: i64 }\n\
             impl Counter { fn bump(&mut self, by: i64) { self.count += by; } }\n\
             pub fn run() -> i64 { let k = 7; let mut c = Counter { count: 1 }; c.bump(4); c.count + k }\n\
             run()",
            "bump",
            every_frame,
        );
        // A method's frame is named by its symbol, which ends in its name.
        assert!(
            seen.iter().any(|saw| {
                let (bump, run) = saw.split_once(" | ").unwrap();
                bump.ends_with("bump: self=Counter { count: 1 } by=4")
                    && run.starts_with("run: k=7 c=<`c` is borrowed by the call to `")
                    && run.ends_with("bump`>")
            }),
            "{seen:?}"
        );
        let seen = at_steps(
            "fn tick(count: &mut u64, step: u64) { *count += step; }\n\
             pub fn run() -> i64 { let k = 7; let mut n: u64 = 3; tick(&mut n, 2); n as i64 + k }\n\
             run()",
            "tick",
            every_frame,
        );
        assert!(
            seen.iter().any(|saw| saw
                == "tick: count=3 step=2 | run: k=7 n=<`n` is borrowed by the call to `tick`>"),
            "{seen:?}"
        );
    }

    /// A value whose copies are counted.
    struct Counted;

    static COPIES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    impl Clone for Counted {
        fn clone(&self) -> Self {
            COPIES.fetch_add(1, Ordering::Relaxed);
            Self
        }
    }

    #[test]
    fn a_native_call_frame_copies_no_argument() {
        let copies = |debugged: bool| {
            let mut engine = crate::Engine::new();
            engine
                .register_type_with_name::<Counted>("Counted")
                .register_fn("counted", || Counted)
                .register_fn("touch", |_: Counted| ());
            if debugged {
                #[allow(deprecated)]
                engine.register_debugger(
                    |_, debugger| debugger,
                    |_, _, _, _, _| Ok(crate::debugger::DebuggerCommand::Continue),
                );
            }
            let before = COPIES.load(Ordering::Relaxed);
            engine
                .run("pub fn run() { let c = counted(); touch(c); touch(counted()); } run()")
                .unwrap_or_else(|error| panic!("{error}"));
            COPIES.load(Ordering::Relaxed) - before
        };
        assert_eq!(copies(true), copies(false));
    }

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
