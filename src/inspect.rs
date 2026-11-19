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
//!   "error": ..}], "unread": .., "frames": [{"function": .., "file": ..,
//!   "line": ..}]}`, the frames innermost first, a caller's line the line
//!   of its call, and `unread` why the frame's bindings can't be read, or
//!   null (#575); or
//!   `{"reached": false, "paused": .., "depth": ..}` when the program is
//!   paused where the request doesn't match. A request waits for its step
//!   as long as that takes; the tool closing the socket gives it up.
//!   With `"children": true`, each value also lists its parts, as its
//!   `{:?}` text writes them (see [`parts`]); so does an `eval` answer.
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
//! A program can serve the socket before any engine of its has a debugger:
//! the first request that reads or controls it then asks the program to
//! register one ([`Inspector::register_on_request`]), and waits until an
//! engine attached to the inspector runs (#2266).
//!
//! The same socket sets breakpoints and resumes a paused program (#489),
//! with an `op`:
//! - `{"op": "break", "file": .., "line": ..}` or `{"op": "break",
//!   "function": "tick"}`: `{"set": .., "lines": [{"file": .., "line": ..}],
//!   "compiled": .., "paused": {"file": .., "line": .., "function": ..,
//!   "reason": ..} or null, "revision": .., "breakpoints": [..]}`, the
//!   pause the program is in if any, or `{"error": ..}`;
//! - `{"op": "clear"}`, with `file`, `line` or `function` to take fewer:
//!   `{"cleared": n, "left": [..], "paused": .., "revision": ..}`, `left`
//!   the breakpoints there are now;
//! - `{"op": "step", "kind": "over" | "into" | "out"}` on a paused program,
//!   answered where the step pauses: `{"paused": ..}`, or `{"paused": null,
//!   "native": true}` once it went on into the native code that called the
//!   program, its pause told as an event at the next statement the program
//!   runs; `{"op": "continue"}`,
//!   answered once it runs on: `{"paused": null}`. A continue while a step
//!   or a pause is under way ends it: the program runs on, and the step
//!   waiting for its pause is answered `{"paused": null}`. Either is
//!   `{"error": "not paused"}` on a program that is neither paused nor
//!   stepping.
//! - `{"op": "pause"}` on a running program: answered at once, `{"paused":
//!   null}`, and it pauses at the next statement it runs, with the reason
//!   `pause`; on a paused one, `{"paused": ..}` where it is.
//! - `{"op": "release"}` on a program held at its start ([`HOLD`]): it
//!   runs on from its first statement, which pauses there if a breakpoint
//!   is on it now. Answered at once, `{"released": true}`, or `{"released":
//!   false}` when nothing held it.
//!
//! It also sends the program's window input (#989), with `{"op": "input",
//! "action": ..}`: a pointer move, a click, a drag, a scroll, a key press or
//! typed text, answered once the window drew it; `{"error": "paused",
//! "paused": ..}` while it is paused, with `"taken": true` when the input
//! made it pause.
//!
//! It captures the program's window too (#1989), with `{"op":
//! "screenshot", "title": .., "file": ..}`: the window draws a frame
//! through its own renderer, written as a PNG to `file` (see
//! [`screenshot`]); `{"error": "paused", ..}` while it is paused.
//!
//! And it reads the program's menu bar, or chooses one of rawr's own
//! items in it, with `{"op": "menu"}` (#1990, see [`menu`]).
//!
//! A paused program evaluates Rust where it is paused, with `{"op":
//! "eval", "code": ..}`: `{"value": ..}` or `{"error": ..}` (#490, see
//! [`evaluate`]); `{"error": "not paused"}` on a program that isn't.
//!
//! A pause and its end are also told as `paused` (with the file, line,
//! function and reason) and `resumed` events where [`EVENTS`] says, so
//! every shell attached to the run hears them (#535): the run's session
//! socket, which first hears where this socket is (`serve`) and holds that
//! while the connection stays open, or a file, a line each.
//!
//! The breakpoints live in [`BREAKPOINTS`] beside the socket, as
//! `{"revision": n, "breakpoints": [{"id", "file", "line", "text"} or
//! {"id", "function"}]}`, so a program started again there has them. A
//! breakpoint's `id` is the revision that set it, and it keeps it while a
//! compile moves its line, so a shell tells a moved one from a new one.
//! Each change
//! to them, a set, a clear that took one or a compile that moved one,
//! writes that file and tells it as a `breakpoints` event with the same
//! fields (#2278). The revision counts the changes, across restarts, so a
//! shell orders what it heard against the answers it got.
//!
//! A request waits for its answer without looking at a clock: the tool
//! closing its connection wakes the wait, and so does a pause, for a
//! request the program's event loop answers.
//!
//! A pause holds the thread that runs the program's code inside the
//! debugger callback. Native code that called into the program, such as a
//! window's event loop, waits in that call until the program goes on. A
//! step pauses at the next statement the program wrote. It can't stop in
//! a crate's native code, so a step out of a function that native code
//! called pauses at the next statement the program runs, in any call into
//! it, and the step's tool hears it went on before that pause comes.
#![cfg(feature = "debugging")]

// What it compiles code with is shared between threads.
#[cfg(feature = "sync")]
mod evaluate;
mod input;
mod interrupt;
pub mod menu;
mod parts;
mod screenshot;
mod stops;

use std::cell::Cell;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, PoisonError};

use serde_json::{json, Value};

use crate::debugger::{BreakPositions, DebuggerCommand, DebuggerEvent};
use crate::types::dynamic::Union;
use crate::{Dynamic, Engine, EvalAltResult, EvalContext, Position};
use interrupt::Interrupt;

/// The variable naming where the program says it paused and went on: the
/// run's session socket, or a file it appends one JSON object per line
/// to. The process that runs the program has it, a runner or the
/// program's own.
pub const EVENTS: &str = "RAWR_MCP_DEBUG_EVENTS";

/// Set in the environment of a program whose shell sets its breakpoints
/// before it runs (#2274): the program waits at the first statement it
/// wrote until a `release` request, which the shell sends once they are
/// in. Breakpoints are set meanwhile; a read waits for the release.
pub const HOLD: &str = "RAWR_DEBUG_HOLD";

/// The file beside the inspector's socket that keeps its breakpoints.
pub const BREAKPOINTS: &str = "breakpoints.json";

#[cfg(feature = "sync")]
pub use evaluate::Program;

/// One binding as a tool reads it: its name and its `Debug` text, or why it
/// has none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    /// The binding's name.
    pub name: String,
    /// Its value as `{:?}` prints it, or Rust's reason it can't.
    pub value: Result<String, String>,
}

/// Whether an answer splits each value into its parts, for a tool that
/// shows values as trees.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    Text,
    Parts,
}

impl Shape {
    fn of(request: &Value) -> Self {
        if request.get("children").and_then(Value::as_bool) == Some(true) {
            Self::Parts
        } else {
            Self::Text
        }
    }

    /// `answer` with `children` beside its `value`, when this shape asks
    /// for parts and the value has them.
    fn add(self, mut answer: Value) -> Value {
        if self == Self::Parts
            && let Some(children) = answer["value"].as_str().and_then(parts::of)
        {
            answer["children"] = Value::from(children);
        }
        answer
    }
}

impl Binding {
    fn to_json(&self, shape: Shape) -> Value {
        match &self.value {
            Ok(text) => shape.add(json!({ "name": self.name, "value": text })),
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
/// between them are left out, and so are the functions the compiler wrote
/// around the program's, as a value's drop glue.
fn script_frames(context: &EvalContext) -> Vec<usize> {
    let Some(debugger) = context.global_runtime_state().debugger.as_ref() else {
        return Vec::new();
    };
    let stack = debugger.call_stack();
    (0..stack.len())
        .rev()
        .filter(|&index| {
            stack[index].bindings.is_some() && !stack[index].fn_name.starts_with("__rawr")
        })
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
/// would have it. `Err` says why a caller's bindings can't be read: the
/// call it made lent nothing (#575). `None` when the program isn't that
/// deep.
fn entries<'c>(context: &'c EvalContext, frame: usize) -> Option<Result<Vec<Entry<'c>>, String>> {
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
        // A step inside an expression that reaches into the receiver, as
        // `self.items[0].push(x)` does, has none: the expression holds it
        // (#541).
        if let Some(name) = own(0).and_then(|bindings| bindings.this_name.as_ref()) {
            found.push((
                name.as_str(),
                context.this_ptr().ok_or_else(|| {
                    format!("`{name}` is borrowed by the expression being evaluated")
                }),
            ));
        }
        return Some(Ok(found));
    }
    let bindings = own(frame)?;
    let index = frames[frame];
    let call = stack.get(index + 1)?;
    let callee = crate::resolve::lower_nominal::function_display_name(&call.fn_name);
    let (Some(lent), Some((scope, end))) = (
        call.caller.as_ref(),
        scope_of(context, stack, &frames, frame),
    ) else {
        return Some(Err(format!(
            "its bindings can't be read while it calls `{callee}`: rawr doesn't keep them for \
             this call"
        )));
    };
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
        } else {
            found.push((
                name.as_str(),
                Err(format!(
                    "`{name}` can't be read while `{callee}` runs: rawr doesn't keep it for this \
                     call"
                )),
            ));
        }
    }
    Some(Ok(found))
}

/// The bindings of frame `frame` where `context` is, the last of each name,
/// in the order they were made; or one binding, `name`. Each prints by the
/// type its function declares it with (#512), the frame at `line` of its
/// file when that is known (0 when not). `Err` says why the frame's
/// bindings can't be read. `None` when the program isn't that deep.
fn read(
    context: &mut EvalContext,
    frame: usize,
    line: usize,
    name: Option<&str>,
) -> Option<Result<Vec<Binding>, String>> {
    let mut seen = HashSet::new();
    let function = frame_symbol(context, frame);
    let entries = match entries(context, frame)? {
        Ok(entries) => entries,
        Err(why) => return Some(Err(why)),
    };
    let mut found: Vec<(String, Result<Dynamic, String>)> = entries
        .into_iter()
        .filter(|(entry, _)| {
            name.map_or_else(|| is_written(entry), |name| *entry == name)
                && seen.insert(entry.to_string())
        })
        .map(|(entry, value)| (entry.to_owned(), value.map(Dynamic::flatten_clone)))
        .collect();
    found.reverse();
    Some(Ok(found
        .into_iter()
        .map(|(name, value)| Binding {
            value: value.and_then(|value| {
                let declared = function.as_ref().map(|function| Declared {
                    function,
                    name: &name,
                    line,
                });
                binding_text(context, &value, declared)
            }),
            name,
        })
        .collect()))
}

/// The bindings in scope where `context` is, the last of each name, in the
/// order they were made.
pub fn bindings(context: &mut EvalContext) -> Vec<Binding> {
    read(context, 0, 0, None)
        .and_then(Result::ok)
        .unwrap_or_default()
}

/// The binding `name` in scope where `context` is.
pub fn binding(context: &mut EvalContext, name: &str) -> Option<Binding> {
    read(context, 0, 0, Some(name))?.ok()?.pop()
}

/// The name frame `frame`'s function runs under where `context` is.
fn frame_symbol(context: &EvalContext, frame: usize) -> Option<crate::ImmutableString> {
    let index = *script_frames(context).get(frame)?;
    let stack = context
        .global_runtime_state()
        .debugger
        .as_ref()?
        .call_stack();
    Some(stack[index].fn_name.clone())
}

/// Where a binding is declared, for its type: its function's name as the
/// program runs it, its own name, and the line the function is at (0 when
/// not known).
struct Declared<'a> {
    function: &'a crate::ImmutableString,
    name: &'a str,
    line: usize,
}

/// `value`, a binding's, as the program's `{:?}` prints it: by the type
/// `declared` gives it when that type prints otherwise than the value
/// alone (#512).
fn binding_text(
    context: &mut EvalContext,
    value: &Dynamic,
    declared: Option<Declared<'_>>,
) -> Result<String, String> {
    if undebuggable(value).is_none()
        && let Some(declared) = declared
    {
        let mut arguments = [
            value.clone(),
            Dynamic::from(declared.function.clone()),
            Dynamic::from(crate::ImmutableString::from(declared.name)),
            Dynamic::from(crate::INT::try_from(declared.line).unwrap_or_default()),
        ];
        let mut arguments: Vec<&mut Dynamic> = arguments.iter_mut().collect();
        if let Ok(text) = context.call_fn_raw(
            crate::resolve::lower_nominal::DEBUG_DECLARED,
            false,
            false,
            &mut arguments,
        ) && let Ok(text) = text.into_immutable_string()
        {
            return Ok(text.to_string());
        }
    }
    debug_text(context, value)
}

/// Whether `name` is in scope in frame `frame` where `context` is.
fn in_scope(context: &EvalContext, frame: usize, name: &str) -> bool {
    entries(context, frame).is_some_and(|entries| {
        entries.is_ok_and(|entries| entries.iter().any(|(entry, _)| *entry == name))
    })
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
    if let Some(unreadable) = undebuggable(value) {
        return Err(unreadable.reason());
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

/// Why a crate value has no `Debug` text where a debugger reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unreadable {
    /// Its type, by this name, doesn't implement `Debug`.
    NoDebug(&'static str),
    /// A native call up this thread's stack holds the values the host
    /// keeps behind handles, as a runner's arena (#533).
    Held,
    /// The host keeps it where this thread can't reach: another thread's
    /// values, or another call's.
    Elsewhere,
}

impl Unreadable {
    /// The reason, as a tool shows it.
    fn reason(self) -> String {
        match self {
            Self::NoDebug(name) => format!("`{}` doesn't implement `Debug`", short_type_name(name)),
            Self::Held => "the program's crate values can't be read at this step: a native call \
                           it runs inside holds them"
                .to_owned(),
            Self::Elsewhere => {
                "this crate value can't be read from this thread: another thread or \
                                call holds it"
                    .to_owned()
            }
        }
    }
}

/// How a host that keeps crate values behind handles says why one has no
/// `Debug` text, as a runner's arena does.
pub type Undebuggable = fn(&Dynamic) -> Option<Unreadable>;

static UNDEBUGGABLE: std::sync::OnceLock<Undebuggable> = std::sync::OnceLock::new();

/// Say why a crate value the host keeps behind a handle has no `Debug`
/// text with `name`. Set once per process.
pub fn name_undebuggable_with(name: Undebuggable) {
    let _ = UNDEBUGGABLE.set(name);
}

/// Why the first value inside `value` that has no `Debug` text has none: a
/// crate value whose type doesn't implement `Debug`, or one the host can't
/// reach here.
fn undebuggable(value: &Dynamic) -> Option<Unreadable> {
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
            if let Some(unreadable) = UNDEBUGGABLE.get().and_then(|name| name(value)) {
                return Some(unreadable);
            }
            // Without a `Debug` text, `{:?}` falls back to the type's name.
            let name = (***variant).type_name();
            (format!("{value:?}") == name).then_some(Unreadable::NoDebug(name))
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
/// place a step of the lowered program in its file and line, and a panic
/// at its place.
pub struct Located {
    entry: crate::diagnostic::Source,
    positions: Arc<crate::program::Positions>,
    /// The files it was compiled from, when a debugger was registered.
    snapshot: Option<crate::package::snapshot::Snapshot>,
}

impl Located {
    // Only a runner's compiler, a rust-import build, keeps one.
    #[cfg_attr(not(feature = "rust-import"), allow(dead_code))]
    pub(crate) fn new(
        entry: crate::diagnostic::Source,
        positions: crate::program::Positions,
    ) -> Self {
        Self {
            entry,
            positions: Arc::new(positions),
            snapshot: None,
        }
    }

    /// It compiled from `snapshot`.
    #[cfg_attr(not(feature = "rust-import"), allow(dead_code))]
    pub(crate) fn with_snapshot(self, snapshot: crate::package::snapshot::Snapshot) -> Self {
        Self {
            snapshot: Some(snapshot),
            ..self
        }
    }

    /// What evaluating code in this program takes, when it kept the files
    /// it compiled from (#490).
    #[cfg(feature = "sync")]
    #[must_use]
    pub fn program(&self) -> Option<Program> {
        Some(Program::new(
            self.snapshot.clone()?,
            self.entry.clone(),
            Arc::clone(&self.positions),
        ))
    }

    /// The file and line of the program's statement at `position` in the
    /// lowered program, or `None` in code the compiler wrote around or
    /// among the program's statements.
    #[must_use]
    pub fn line(&self, position: Position) -> Option<(String, usize)> {
        let (file, span) = self.positions.locate_statement(&self.entry, position)?;
        Some((file.identity().to_owned(), span.start().line()?))
    }

    /// The report of `error`, a run-time failure of the program, at its
    /// place in the program's files, named by `name`; at the entry's alone
    /// in code the compiler wrote.
    #[cfg_attr(not(feature = "rust-import"), allow(dead_code))]
    pub(crate) fn report(
        &self,
        error: &EvalAltResult,
        name: impl Fn(&crate::diagnostic::Source) -> crate::diagnostic::Source,
    ) -> Option<crate::diagnostic::Report> {
        let (file, span) = match self.positions.locate(&self.entry, error) {
            Some((file, span)) => (file, Some(span)),
            None => (&self.entry, None),
        };
        crate::diagnostic::Report::from_evaluation_located(name(file), span, error).ok()
    }
}

struct Request {
    id: u64,
    name: Option<String>,
    file: Option<String>,
    /// Which frame: 0 for the function the program is in, 1 for its
    /// caller, and on (#511).
    frame: usize,
    /// Code to evaluate where the program is paused (#490).
    code: Option<String>,
    shape: Shape,
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
    /// To the next statement it runs, wherever that is: a tool asked it to
    /// pause.
    Pause,
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
    /// The evaluation the step started in, while it runs: a call from
    /// native code into the program, which ends by returning to that code.
    home: Option<Home>,
}

/// One evaluation of the program on one thread, by where its runtime state
/// lives while it runs.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Home {
    thread: std::thread::ThreadId,
    global: usize,
}

impl Home {
    fn of(context: &EvalContext) -> Self {
        Self {
            thread: std::thread::current().id(),
            global: std::ptr::from_ref(context.global_runtime_state()).addr(),
        }
    }
}

thread_local! {
    /// The callback is answering on this thread: steps of the `Debug` code
    /// and the code it runs are not answered and don't pause. Evaluations on
    /// other threads go on calling back, so one that reaches a breakpoint or
    /// a step's end meanwhile pauses there.
    static ANSWERING: Cell<bool> = const { Cell::new(false) };
}

#[derive(Default)]
struct State {
    pending: Vec<Request>,
    /// The breakpoints, and the program's statements once it compiled.
    points: Vec<stops::Point>,
    /// How many times the breakpoints changed, kept with them.
    revision: u64,
    stops: Option<Arc<stops::Stops>>,
    /// Where the program is paused, as a tool reads it.
    paused: Option<Value>,
    /// How many times it paused, and the last place.
    pauses: u64,
    last: Value,
    /// How many steps went on into native code before they paused.
    departures: u64,
    /// How many steps a continue ended before they paused.
    ended: u64,
    resume: Option<Resume>,
    stepping: Option<Stepping>,
    /// The requests the program's event loop answers, which a pause gives
    /// up: the pause holds that loop.
    held: Vec<Arc<Interrupt>>,
}

/// What a request that reads or controls the program does while no
/// engine answers it.
enum Wanted {
    /// It waits for an engine to attach.
    Wait,
    /// The first one registers a debugger: the program started without
    /// one (#2266).
    Register(Box<dyn FnOnce() + Send>),
    /// An engine answers it.
    Attached,
}

/// Where the program tells that it paused and went on (#535).
enum Told {
    /// The run's session, on a connection that first said where this
    /// program's socket is.
    #[cfg(unix)]
    Session(Mutex<std::os::unix::net::UnixStream>),
    /// A file, one event per line.
    File(std::path::PathBuf),
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    wake: Arc<AtomicBool>,
    breaks: Arc<BreakPositions>,
    /// A step is under way: each step calls back until it pauses.
    stepping: AtomicBool,
    /// The program waits at its first statement until a `release`.
    entry: AtomicBool,
    /// One step of the program pauses at a time.
    one: Mutex<()>,
    /// Where the running program's steps are, once it compiled.
    locate: Mutex<Option<Arc<Locate>>>,
    /// What compiling code into the running program takes, once it
    /// compiled.
    #[cfg(feature = "sync")]
    program: Mutex<Option<Arc<Program>>>,
    /// Where the breakpoints are kept across restarts.
    saved: std::path::PathBuf,
    told: Option<Told>,
    wanted: Mutex<Wanted>,
}

impl Shared {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn wanted(&self) -> std::sync::MutexGuard<'_, Wanted> {
        self.wanted.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A request needs an engine: the first one registers a debugger with
    /// the program, when it started without one.
    fn want(&self) {
        let mut wanted = self.wanted();
        if let Wanted::Register(_) = &*wanted
            && let Wanted::Register(register) = std::mem::replace(&mut *wanted, Wanted::Wait)
        {
            drop(wanted);
            register();
        }
    }

    /// Have every evaluation call back at its next step while a request
    /// waits for one, or while a step pauses wherever the program goes: an
    /// evaluation native code started before the step, or one that copies
    /// the state of a call that wasn't stepping, calls back at no other
    /// step.
    fn rouse(&self, state: &State) {
        let anywhere = state
            .stepping
            .as_ref()
            .is_some_and(|stepping| stepping.anywhere);
        self.wake.store(
            anywhere || !state.pending.is_empty() || self.entry.load(Ordering::Acquire),
            Ordering::Release,
        );
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
        let saved = socket.with_file_name(BREAKPOINTS);
        let kept = std::fs::read(&saved)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .unwrap_or_default();
        let points = kept["breakpoints"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(stops::Point::from_json)
            .collect();
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                points,
                revision: kept["revision"].as_u64().unwrap_or(0),
                ..State::default()
            }),
            changed: Condvar::new(),
            wake: Arc::default(),
            breaks: Arc::default(),
            stepping: AtomicBool::new(false),
            entry: AtomicBool::new(false),
            one: Mutex::new(()),
            locate: Mutex::new(None),
            #[cfg(feature = "sync")]
            program: Mutex::new(None),
            saved,
            told: std::env::var_os(EVENTS)
                .filter(|path| !path.is_empty())
                .and_then(|path| Told::open(std::path::PathBuf::from(path), socket)),
            wanted: Mutex::new(Wanted::Wait),
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
        let inspector = Self(shared);
        if std::env::var_os(HOLD).is_some_and(|value| !value.is_empty()) {
            inspector.hold();
        }
        Ok(inspector)
    }

    /// Hold the program at the first statement it wrote until a `release`
    /// request: every evaluation calls back at its next step until then.
    pub fn hold(&self) {
        let shared = &self.0;
        let state = shared.state();
        shared.entry.store(true, Ordering::Release);
        shared.rouse(&state);
    }

    /// Answer requests from the program `engine` runs, and pause it at the
    /// breakpoints. Attach before the program compiles, so the compiler
    /// keeps what reading needs (#526).
    pub fn attach(&self, engine: &mut Engine) {
        *self.0.wanted() = Wanted::Attached;
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

    /// Until an engine attaches, call `register` once, when the first
    /// request that reads or controls the program arrives, which then
    /// waits for an engine as any request does: a program that started
    /// without a debugger registers one then (#2266). Window input,
    /// screenshots and menus need no engine, and call nothing. A later
    /// `register` replaces this one.
    pub fn register_on_request(&self, register: impl FnOnce() + Send + 'static) {
        let mut wanted = self.0.wanted();
        if !matches!(*wanted, Wanted::Attached) {
            *wanted = Wanted::Register(Box::new(register));
        }
    }

    /// Place the program's steps with `locate` from now on: the positions
    /// of the program that runs next.
    pub fn locate_with(
        &self,
        locate: impl Fn(Position) -> Option<(String, usize)> + Send + Sync + 'static,
    ) {
        *self.0.locate.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(locate));
    }

    /// Evaluate code in the program that runs next, `program` (#490).
    #[cfg(feature = "sync")]
    pub fn evaluate_in(&self, program: Program) {
        *self
            .0
            .program
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(program));
    }

    /// The program that runs next compiled as `ast`, its steps placed by
    /// [`Self::locate_with`]: its breakpoints move to where their lines are
    /// now, and those whose line is gone are removed.
    pub fn compiled(&self, ast: &crate::AST) {
        let shared = &self.0;
        let stops = Arc::new(stops::Stops::of(ast, &|position| shared.locate(position)));
        let mut state = shared.state();
        let points: Vec<stops::Point> = state
            .points
            .iter()
            .filter_map(|point| stops::moved(point, &stops))
            .collect();
        let changed = points.len() != state.points.len()
            || points
                .iter()
                .zip(&state.points)
                .any(|(now, was)| now.spec != was.spec);
        state.points = points;
        state.stops = Some(stops);
        shared.armed(&state);
        if changed {
            shared.save(&mut state);
        }
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
    /// Take one request from `stream` and answer it. The tool sends one
    /// line and then only waits, so the end of its stream is its leaving: a
    /// thread blocked reading it gives the request up then.
    #[cfg(unix)]
    fn handle(self: &Arc<Self>, stream: std::os::unix::net::UnixStream) {
        use std::io::{BufRead as _, BufReader, Read as _, Write as _};

        let mut line = String::new();
        let Ok(reading) = stream.try_clone() else {
            return;
        };
        let mut reading = BufReader::new(reading);
        if reading.read_line(&mut line).is_err() {
            return;
        }
        let request: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
        let gone = Arc::new(Interrupt::default());
        let watching = Arc::clone(&gone);
        let shared = Arc::clone(self);
        let _ = std::thread::Builder::new()
            .name("rawr-inspect-watch".into())
            .spawn(move || {
                let _ = reading.read(&mut [0_u8; 1]);
                watching.fire();
                // A wait on `changed` looks at the interrupt under this lock.
                drop(shared.state());
                shared.changed.notify_all();
            });
        // Every request is answered, a panic as its error, so a tool that
        // reads the end before an answer knows the process ended (#1964).
        let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.respond(&request, &gone)
        }))
        .unwrap_or_else(|_| json!({ "error": "the program panicked answering this request" }));
        let mut stream = stream;
        let _ = writeln!(stream, "{answer}");
        // The watch ends with the stream.
        let _ = stream.shutdown(std::net::Shutdown::Both);
    }

    /// The answer to one request, which `gone` gives up.
    #[cfg(unix)]
    fn respond(&self, request: &Value, gone: &Arc<Interrupt>) -> Value {
        let op = request.get("op").and_then(Value::as_str);
        if !matches!(op, Some("input" | "screenshot" | "menu")) {
            self.want();
        }
        match op {
            Some("input") => self.input(request, gone),
            Some("screenshot") => self.screenshot(request, gone),
            Some("menu") => self.menu(request, gone),
            Some("eval") => self.evaluate(request, gone),
            Some("break") => self.set(request),
            Some("clear") => self.clear(request),
            Some("continue") => self.resume(Resume::Continue, gone),
            Some("pause") => self.interrupt(),
            Some("release") => self.let_go(),
            Some("step") => self.resume(
                Resume::Step(match request.get("kind").and_then(Value::as_str) {
                    Some("into") => Step::Into,
                    Some("out") => Step::Out,
                    _ => Step::Over,
                }),
                gone,
            ),
            _ => self.inspect(request, gone),
        }
    }

    /// Evaluate code where the program is paused: answered by the pause.
    fn evaluate(&self, request: &Value, gone: &Interrupt) -> Value {
        self.inspect(request, gone)
    }

    /// Hold `gone` for a pause to give up too, unless the program is
    /// paused: its pause, then.
    fn hold(&self, gone: &Arc<Interrupt>) -> Option<Value> {
        let mut state = self.state();
        if let Some(paused) = state.paused.clone() {
            return Some(paused);
        }
        state.held.push(Arc::clone(gone));
        None
    }

    fn release(&self, gone: &Arc<Interrupt>) {
        self.state().held.retain(|held| !Arc::ptr_eq(held, gone));
    }

    /// Send the program's window an input. A paused program holds its
    /// event loop, so it takes none until it goes on; an input that makes it
    /// pause is answered with the pause.
    fn input(&self, request: &Value, gone: &Arc<Interrupt>) -> Value {
        if let Some(paused) = self.hold(gone) {
            return json!({ "error": "paused", "paused": paused });
        }
        let answer = input::answer(request, gone);
        self.release(gone);
        answer.unwrap_or_else(
            || json!({ "error": "paused", "paused": self.state().paused.clone(), "taken": true }),
        )
    }

    /// Capture the program's window. A paused program holds its event
    /// loop, so its window draws nothing until it goes on.
    fn screenshot(&self, request: &Value, gone: &Arc<Interrupt>) -> Value {
        if let Some(paused) = self.hold(gone) {
            return json!({ "error": "paused", "paused": paused });
        }
        let answer = screenshot::answer(request, gone);
        self.release(gone);
        answer
            .unwrap_or_else(|| json!({ "error": "paused", "paused": self.state().paused.clone() }))
    }

    /// Read the program's menu bar, or choose one of rawr's items. A paused
    /// program holds its event loop, where menus are read and chosen, so
    /// the request waits for neither.
    fn menu(&self, request: &Value, gone: &Arc<Interrupt>) -> Value {
        if let Some(paused) = self.hold(gone) {
            return json!({ "error": "paused", "paused": paused });
        }
        let answer = menu::answer(request, gone);
        self.release(gone);
        answer.unwrap_or_else(|| {
            let paused = self.state().paused.clone();
            match paused {
                Some(paused) => json!({ "error": "paused", "paused": paused }),
                None => json!({ "error": "the tool went away" }),
            }
        })
    }

    /// Read values: answered at the program's next matching step, or where
    /// it is paused, unless the tool goes away first.
    fn inspect(&self, request: &Value, gone: &Interrupt) -> Value {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let text = |key: &str| request.get(key).and_then(Value::as_str).map(str::to_owned);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let (reply, answer) = mpsc::channel();
        // Every answer is an object: null says the tool went away.
        let giving_up = reply.clone();
        gone.on_fire(move || {
            let _ = giving_up.send(Value::Null);
        });
        let mut state = self.state();
        // Code waits only in a pause, which answers every request it finds
        // before it goes on: one asked of a running program would wait for
        // the next.
        if request.get("code").is_some() && state.paused.is_none() {
            return json!({ "error": "not paused" });
        }
        state.pending.push(Request {
            id,
            name: text("name"),
            file: text("file"),
            frame: request
                .get("frame")
                .and_then(Value::as_u64)
                .and_then(|frame| usize::try_from(frame).ok())
                .unwrap_or(0),
            code: text("code"),
            shape: Shape::of(request),
            reply,
        });
        drop(state);
        self.wake.store(true, Ordering::Release);
        self.changed.notify_all();
        match answer.recv() {
            Ok(Value::Null) | Err(_) => {}
            Ok(value) => return value,
        }
        {
            let mut state = self.state();
            state.pending.retain(|request| request.id != id);
            self.rouse(&state);
            drop(state);
        }
        // An answer made while the request was being given up.
        answer
            .try_iter()
            .find(|value| !value.is_null())
            .unwrap_or_else(|| json!({ "reached": false }))
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
        // Named by the revision this set makes.
        let mut point = match stops::set(state.revision + 1, spec, state.stops.as_deref()) {
            Ok(point) => point,
            Err(error) => return json!({ "error": error }),
        };
        let compiled = state.stops.is_some();
        let lines = lines(&state, &point);
        // A breakpoint set again keeps its id.
        if let Some(held) = state.points.iter().find(|held| held.spec == point.spec) {
            point.id = held.id;
        }
        let spec = point.to_json();
        state.points.retain(|held| held.spec != point.spec);
        state.points.push(point);
        self.armed(&state);
        self.save(&mut state);
        let mut answer = told(&state);
        answer["set"] = spec;
        answer["lines"] = json!(lines);
        answer["compiled"] = json!(compiled);
        answer["paused"] = json!(state.paused);
        answer
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
        if cleared > 0 {
            self.armed(&state);
            self.save(&mut state);
        }
        let told = told(&state);
        json!({
            "cleared": cleared,
            "left": told["breakpoints"],
            "paused": state.paused,
            "revision": told["revision"],
        })
    }

    /// Resume a paused program. A step is answered where it pauses, or
    /// once it went on into the native code that called the program, which
    /// it can't stop in: `{"paused": null, "native": true}`, its pause told
    /// as an event at the next statement the program runs. A continue is
    /// answered once the program runs on; either, once the tool went away.
    /// A continue while a step runs ends the step, whose wait is answered,
    /// so a step that never pauses holds no tool.
    fn resume(&self, resume: Resume, gone: &Interrupt) -> Value {
        let mut state = self.state();
        if state.paused.is_none() {
            if matches!(resume, Resume::Continue) && state.stepping.take().is_some() {
                self.stepping.store(false, Ordering::Release);
                state.ended += 1;
                self.rouse(&state);
                self.changed.notify_all();
                return json!({ "paused": null });
            }
            return json!({ "error": "not paused" });
        }
        let seen = (state.pauses, state.departures, state.ended);
        let step = matches!(resume, Resume::Step(_));
        state.resume = Some(resume);
        self.changed.notify_all();
        if !step {
            return json!({ "paused": null });
        }
        while (state.pauses, state.departures, state.ended) == seen {
            if gone.is_fired() {
                return json!({ "paused": null });
            }
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        // A step ends at its first pause or its departure, which no later
        // one follows without a resume: a pause the departure led to that
        // came before this wait looked is told as its event.
        if state.departures != seen.1 {
            return json!({ "paused": null, "native": true });
        }
        if state.pauses == seen.0 {
            // A continue ended the step before it paused.
            return json!({ "paused": null });
        }
        json!({ "paused": state.last })
    }

    /// Pause the running program at the next statement it runs, answered
    /// at once: the pause it is in already, if any. A program waiting in
    /// native code, as a window's event loop, pauses when that code calls
    /// into it again.
    fn interrupt(&self) -> Value {
        let mut state = self.state();
        if state.paused.is_some() {
            return json!({ "paused": state.paused });
        }
        state.stepping = Some(Stepping {
            step: Step::Pause,
            depth: 0,
            position: Position::NONE,
            file: String::new(),
            line: 0,
            anywhere: true,
            home: None,
        });
        self.stepping.store(true, Ordering::Release);
        // The engine calls back at its next step, which goes on stepping.
        self.rouse(&state);
        json!({ "paused": null })
    }

    /// Let a program held at its start run on: answered at once.
    fn let_go(&self) -> Value {
        let state = self.state();
        let released = self.entry.swap(false, Ordering::AcqRel);
        self.rouse(&state);
        drop(state);
        self.changed.notify_all();
        json!({ "released": released })
    }

    /// Wait at the program's first statement until a `release`.
    fn wait_entry(&self) {
        let mut state = self.state();
        while self.entry.load(Ordering::Acquire) {
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
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

    /// The breakpoints changed: count the change, keep them for a program
    /// started again, and tell every shell what the file now holds.
    fn save(&self, state: &mut State) {
        state.revision += 1;
        let mut told = told(state);
        let _ = std::fs::write(&self.saved, told.to_string());
        told["event"] = json!("breakpoints");
        self.emit(&told);
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
        if ANSWERING.get() {
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
        // A program held at its start waits at its first statement; a
        // breakpoint set on it meanwhile pauses it there once released.
        let mut event = event;
        if self.entry.load(Ordering::Acquire) && self.stop(position, &node).is_some() {
            self.wait_entry();
            if self.breaks.contains(position) {
                event = DebuggerEvent::Break;
            }
        }
        match event {
            DebuggerEvent::Break => {
                if let Some(stop) = self.stop(position, &node) {
                    self.pause(context, position, stop, "breakpoint");
                }
            }
            DebuggerEvent::Start if self.stepping.load(Ordering::Acquire) => {
                // Native code called into the program again.
                let mut state = self.state();
                if let Some(stepping) = state.stepping.as_mut() {
                    stepping.anywhere = true;
                }
                self.rouse(&state);
            }
            DebuggerEvent::End if self.stepping.load(Ordering::Acquire) => self.depart(context),
            // A step that pauses wherever the program goes ends in an
            // evaluation that wasn't stepping too: one native code started
            // before the step, or one that copies the state of a call that
            // wasn't stepping, which call back only once woken.
            DebuggerEvent::Step | DebuggerEvent::Wake if self.stepping.load(Ordering::Acquire) => {
                let woken = matches!(event, DebuggerEvent::Wake);
                if let Some(stop) = self.stop(position, &node) {
                    let reason = if self.breaks.contains(position) {
                        Some("breakpoint")
                    } else {
                        let depth = depth(context);
                        self.state()
                            .stepping
                            .as_ref()
                            .filter(|stepping| !woken || stepping.anywhere)
                            .filter(|stepping| ends(stepping, depth, position, &stop))
                            .map(|stepping| {
                                if stepping.step == Step::Pause {
                                    "pause"
                                } else {
                                    "step"
                                }
                            })
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

    /// An evaluation ended where `context` is. When it is the one a step
    /// started in, the step went on into the native code that called it,
    /// where it can't stop: it pauses at the next statement the program
    /// runs, wherever that is, and its tool is answered now.
    fn depart(&self, context: &EvalContext) {
        let home = Home::of(context);
        let mut state = self.state();
        let Some(stepping) = state
            .stepping
            .as_mut()
            .filter(|stepping| stepping.home == Some(home))
        else {
            return;
        };
        stepping.home = None;
        stepping.anywhere = true;
        state.departures += 1;
        self.rouse(&state);
        drop(state);
        self.changed.notify_all();
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
            "function": self.frame_name(context, 0),
            "reason": reason,
        });
        let mut state = self.state();
        state.stepping = None;
        self.stepping.store(false, Ordering::Release);
        self.rouse(&state);
        let mut event = at.clone();
        event["event"] = json!("paused");
        self.emit(&event);
        state.paused = Some(at.clone());
        state.last = at;
        state.pauses += 1;
        state.resume = None;
        for held in state.held.drain(..) {
            held.fire();
        }
        self.changed.notify_all();
        let resume = loop {
            if !state.pending.is_empty() {
                let pending = std::mem::take(&mut state.pending);
                self.rouse(&state);
                let paused = state.paused.clone();
                drop(state);
                ANSWERING.set(true);
                for request in pending {
                    if let Some(code) = &request.code {
                        let _ = request
                            .reply
                            .send(self.evaluated(context, &request, code, position));
                    } else if self.matches(context, &request, position) {
                        self.reply(context, &request, position);
                    } else {
                        let _ = request.reply.send(json!({
                            "reached": false,
                            "paused": paused,
                            "depth": depth_of(context),
                        }));
                    }
                }
                ANSWERING.set(false);
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
                home: Some(Home::of(context)),
            });
            self.stepping.store(true, Ordering::Release);
        }
    }

    /// At a step of the program: answer each waiting request this step
    /// matches.
    fn answer(&self, context: &mut EvalContext, position: Position) {
        if ANSWERING.replace(true) {
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
            self.rouse(&state);
        }
        for request in matched {
            self.reply(context, &request, position);
        }
        ANSWERING.set(false);
    }

    /// Whether `request` reads where `context` is, at `position`: the
    /// program is as deep as its frame, which is in its file and has its
    /// binding.
    fn matches(&self, context: &EvalContext, request: &Request, position: Position) -> bool {
        // Code is evaluated only where the program is paused.
        request.code.is_none()
            && request.frame < depth_of(context)
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

    /// The function of frame `frame` where `context` is, as Rust names it:
    /// a closure by the function it is written in (`Stops::function_name`).
    fn frame_name(&self, context: &EvalContext, frame: usize) -> Option<String> {
        let symbol = frame_symbol(context, frame)?;
        let stops = self.state().stops.clone();
        Some(stops.map_or_else(
            || crate::resolve::lower_nominal::function_display_name(&symbol).to_owned(),
            |stops| stops.function_name(&symbol),
        ))
    }

    /// The answer to `code`, evaluated where `context` is paused, at
    /// `position`.
    fn evaluated(
        &self,
        context: &mut EvalContext,
        request: &Request,
        code: &str,
        position: Position,
    ) -> Value {
        if request.frame > 0 {
            return json!({ "error": "code is evaluated only in the function the program is paused in, frame 0" });
        }
        #[cfg(feature = "sync")]
        let program = self
            .program
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        #[cfg(not(feature = "sync"))]
        let program: Option<()> = {
            let _ = (context, code, position);
            None
        };
        match program {
            #[cfg(feature = "sync")]
            Some(program) => request
                .shape
                .add(evaluate::answer(context, &program, position, code)),
            _ => {
                json!({ "error": "this run kept none of the program's files, which compiling code into it takes" })
            }
        }
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
                    "function": self.frame_name(context, index),
                    "file": at.as_ref().map(|(file, _)| file),
                    "line": at.as_ref().map(|(_, line)| line),
                })
            })
            .collect();
        let function = self.frame_name(context, frame);
        let line = location.as_ref().map_or(0, |(_, line)| *line);
        let (found, unread) = match read(context, frame, line, request.name.as_deref()) {
            Some(Ok(found)) => (found, None),
            Some(Err(why)) => (Vec::new(), Some(why)),
            None => (Vec::new(), None),
        };
        let _ = request.reply.send(json!({
            "reached": true,
            "frame": frame,
            "file": location.as_ref().map(|(file, _)| file),
            "line": location.as_ref().map(|(_, line)| line),
            "function": function,
            "bindings": found
                .iter()
                .map(|binding| binding.to_json(request.shape))
                .collect::<Vec<_>>(),
            "unread": unread,
            "frames": frames,
        }));
    }

    /// Tell `event` where [`EVENTS`] says.
    fn emit(&self, event: &Value) {
        if let Some(told) = &self.told {
            told.emit(event);
        }
    }
}

impl Told {
    /// Where `path` says to tell events: a session socket, told at once
    /// that this program's inspector listens on `socket`, or a file.
    #[cfg(unix)]
    fn open(path: std::path::PathBuf, socket: &std::path::Path) -> Option<Self> {
        use std::io::Write as _;
        use std::os::unix::fs::FileTypeExt as _;

        let is_socket =
            std::fs::metadata(&path).is_ok_and(|metadata| metadata.file_type().is_socket());
        if !is_socket {
            return Some(Self::File(path));
        }
        let mut session = std::os::unix::net::UnixStream::connect(&path).ok()?;
        let serve = json!({ "jsonrpc": "2.0", "method": "serve", "params": { "socket": socket.to_string_lossy() } });
        writeln!(session, "{serve}").ok()?;
        Some(Self::Session(Mutex::new(session)))
    }

    fn emit(&self, event: &Value) {
        use std::io::Write as _;
        match self {
            #[cfg(unix)]
            Self::Session(session) => {
                let told = json!({ "jsonrpc": "2.0", "method": "emit", "params": event });
                let _ = writeln!(
                    session.lock().unwrap_or_else(PoisonError::into_inner),
                    "{told}"
                );
            }
            Self::File(path) => {
                if let Ok(mut file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                {
                    let _ = file.write_all(format!("{event}\n").as_bytes());
                }
            }
        }
    }
}

/// The breakpoints as [`BREAKPOINTS`] keeps them: `{"revision",
/// "breakpoints"}`.
fn told(state: &State) -> Value {
    let specs: Vec<Value> = state.points.iter().map(stops::Point::to_json).collect();
    json!({ "revision": state.revision, "breakpoints": specs })
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
        Step::Pause => true,
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
        read(context, frame, 0, None).map_or_else(
            || "none".to_owned(),
            |found| {
                let found = match found {
                    Ok(found) => found,
                    Err(why) => return format!("unread <{why}>"),
                };
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
                .any(|saw| saw
                    == "main::inner: x=6 | main::outer: a=5 b=6 | main::run: n=5 | : none"),
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
        // A method's frame is named by its path, as Rust names it (#573).
        assert!(
            seen.iter().any(|saw| saw
                == "main::Counter::bump: self=Counter { count: 1 } by=4 | main::run: k=7 \
                    c=<`c` is borrowed by the call to `main::Counter::bump`>"),
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
                == "main::tick: count=3 step=2 | main::run: k=7 n=<`n` is borrowed by the call \
                    to `main::tick`>"),
            "{seen:?}"
        );
    }

    #[test]
    fn bindings_print_by_their_declared_type() {
        let seen = at_steps(
            "fn look(p: (i64, bool), o: std::cmp::Ordering) -> i64 {\n\
                 let pair = (1, \"a\");\n\
                 let ord = 3.cmp(&4);\n\
                 let big: u64 = u64::MAX;\n\
                 let half: f32 = 0.1;\n\
                 let pairs = vec![(1, 2)];\n\
                 let maybe = Some(ord);\n\
                 p.0\n\
             }\n\
             pub fn run() -> i64 { look((7, true), std::cmp::Ordering::Greater) }\n\
             run()",
            "look",
            |context| shown(context, 0),
        );
        assert!(
            seen.iter().any(|saw| saw
                == "p=(7, true) o=Greater pair=(1, \"a\") ord=Less big=18446744073709551615 \
                    half=0.1 pairs=[(1, 2)] maybe=Some(Less)"),
            "{seen:?}"
        );
    }

    #[test]
    fn a_receiver_lent_to_an_expression_on_itself_reads_or_says_why() {
        let seen = at_steps(
            "struct Counter { count: i64, items: Vec<i64> }\n\
             impl Counter {\n\
                 fn bump(&mut self, by: i64) { self.count += by; self.items.push(by); }\n\
             }\n\
             pub fn run() -> i64 { let mut c = Counter { count: 1, items: vec![] }; c.bump(4); c.count }\n\
             run()",
            "bump",
            |context| shown(context, 0),
        );
        // Every step lists `self`: read where the expression is on `self`
        // itself, and borrowed further into it.
        assert!(seen.iter().all(|saw| saw.starts_with("self=")), "{seen:?}");
        assert!(
            seen.iter()
                .any(|saw| saw == "self=Counter { count: 5, items: [] } by=4"),
            "{seen:?}"
        );
        assert!(
            seen.iter().any(
                |saw| saw == "self=<`self` is borrowed by the expression being evaluated> by=4"
            ),
            "{seen:?}"
        );
    }

    #[test]
    fn a_caller_frame_reads_its_receiver_under_a_chain_on_a_value() {
        let seen = at_steps(
            "struct C { k: i64 }\n\
             impl C {\n\
                 fn get(&self) -> i64 { self.k }\n\
                 fn g(&self) -> i64 { let z = 1; C { k: self.k + z }.get() }\n\
             }\n\
             pub fn run() -> i64 { let c = C { k: 2 }; c.g() }\n\
             run()",
            "get",
            every_frame,
        );
        assert!(
            seen.iter().any(|saw| saw
                == "main::C::get: self=C { k: 3 } | main::C::g: self=C { k: 2 } z=1 | main::run: \
                    c=<`c` is borrowed by the call to `main::C::g`>"),
            "{seen:?}"
        );
    }

    #[test]
    fn frames_the_compiler_wrote_are_left_out() {
        let seen = at_steps(
            "struct D { n: i64 }\n\
             impl Drop for D { fn drop(&mut self) { let m = self.n; } }\n\
             pub fn run() -> i64 { let k = 1; { let d = D { n: 9 }; } k }\n\
             run()",
            "drop",
            every_frame,
        );
        assert!(
            seen.iter()
                .any(|saw| saw
                    == "main::D::Drop::drop: self=D { n: 9 } | main::run: k=1 d=D { n: 9 }"),
            "{seen:?}"
        );
    }

    /// A crate value a test host keeps where a native call holds it.
    #[derive(Clone)]
    struct HeldHandle;

    #[test]
    fn a_crate_value_a_native_call_holds_says_why_it_cant_be_read() {
        super::name_undebuggable_with(|value| {
            value
                .read_lock::<HeldHandle>()
                .map(|_| super::Unreadable::Held)
        });
        let held = Dynamic::from_array(vec![Dynamic::from(1_i64), Dynamic::from(HeldHandle)]);
        assert_eq!(
            undebuggable(&held).map(super::Unreadable::reason),
            Some(
                "the program's crate values can't be read at this step: a native call it runs \
                 inside holds them"
                    .to_owned()
            )
        );
        assert_eq!(undebuggable(&Dynamic::from(1_i64)), None);
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
        let named = |value: &Dynamic| match undebuggable(value) {
            Some(super::Unreadable::NoDebug(name)) => Some(short_type_name(name)),
            _ => None,
        };
        assert_eq!(named(&Dynamic::from(Opaque)), Some("Opaque".to_owned()));
        let held = Dynamic::from_array(vec![Dynamic::from(1_i64), Dynamic::from(Opaque)]);
        assert_eq!(named(&held), Some("Opaque".to_owned()));
        let some = crate::option::some(Dynamic::from(Opaque));
        assert_eq!(named(&some), Some("Opaque".to_owned()));
        assert_eq!(named(&Dynamic::from(1_u8)), None);
        assert_eq!(named(&crate::option::none()), None);
    }

    /// One request on the inspector at `socket`, and its answer.
    #[cfg(unix)]
    fn ask(socket: &std::path::Path, request: &serde_json::Value) -> serde_json::Value {
        use std::io::{BufRead as _, Write as _};
        let mut stream = std::os::unix::net::UnixStream::connect(socket).unwrap();
        writeln!(stream, "{request}").unwrap();
        let mut line = String::new();
        std::io::BufReader::new(stream)
            .read_line(&mut line)
            .unwrap();
        serde_json::from_str(&line).unwrap_or(serde_json::Value::Null)
    }

    /// A program whose `update` and `view` native code calls, and whose
    /// `spin` runs while `going()` says so.
    const CALLED: &str = "struct Counter { value: i64 }\n\
        fn update(counter: &mut Counter, message: i64) {\n\
        \x20   match message {\n\
        \x20       0 => counter.value += 1,\n\
        \x20       _ => counter.value -= 1,\n\
        \x20   }\n\
        }\n\
        fn view(counter: &Counter) -> i64 {\n\
        \x20   vec![counter.value, 1]\n\
        \x20       .iter()\n\
        \x20       .sum::<i64>()\n\
        }\n\
        fn spin() -> i64 {\n\
        \x20   let mut turns = 0;\n\
        \x20   while going() {\n\
        \x20       turns += 1;\n\
        \x20   }\n\
        \x20   turns\n\
        }\n\
        fn make() -> Counter { Counter { value: 0 } }\n";

    /// [`CALLED`] compiled on an engine its inspector answers for, on a
    /// socket of its own; `going()` reads `going`.
    #[cfg(unix)]
    struct Called {
        directory: std::path::PathBuf,
        socket: std::path::PathBuf,
        _inspector: super::Inspector,
        engine: crate::Engine,
        ast: crate::AST,
    }

    #[cfg(unix)]
    impl Called {
        fn new(name: &str, going: &Arc<AtomicBool>) -> Self {
            let directory =
                std::env::temp_dir().join(format!("rawr-{name}-{}", std::process::id()));
            std::fs::create_dir_all(&directory).unwrap();
            let socket = directory.join("s");
            let inspector = super::Inspector::bind(&socket).unwrap();
            let mut engine = crate::Engine::new();
            let going = Arc::clone(going);
            engine.register_fn("going", move || going.load(Ordering::Acquire));
            inspector.attach(&mut engine);
            let ast = engine
                .compile(CALLED)
                .unwrap_or_else(|error| panic!("{error}"));
            inspector.locate_with(|position| Some(("main.rs".to_owned(), position.line()?)));
            inspector.compiled(&ast);
            let set = ask(
                &socket,
                &serde_json::json!({ "op": "break", "function": "update" }),
            );
            assert!(set.get("error").is_none(), "{set}");
            Self {
                directory,
                socket,
                _inspector: inspector,
                engine,
                ast,
            }
        }

        /// A counter, as native code holds one.
        fn make(&self) -> Dynamic {
            self.engine
                .call_fn::<Dynamic>(&mut crate::Scope::new(), &self.ast, "make", ())
                .unwrap_or_else(|error| panic!("{error}"))
        }

        /// Call `update` as native code does: an evaluation of its own.
        fn update(&self, counter: &mut Dynamic) {
            self.engine
                .call_fn_with_options::<()>(
                    crate::CallFnOptions::new().bind_this_ptr(counter),
                    &mut crate::Scope::new(),
                    &self.ast,
                    "update",
                    (0_i64,),
                )
                .unwrap_or_else(|error| panic!("{error}"));
        }

        fn view(&self, counter: &Dynamic) {
            let _ = self
                .engine
                .call_fn::<i64>(
                    &mut crate::Scope::new(),
                    &self.ast,
                    "view",
                    (counter.clone(),),
                )
                .unwrap_or_else(|error| panic!("{error}"));
        }
    }

    #[cfg(unix)]
    impl Drop for Called {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }

    /// What a tool sees stepping over from the breakpoint in `update` at
    /// `socket` until a step goes on into native code: each step's answer,
    /// then where the program pauses next, read by a name it never binds,
    /// which only a pause answers.
    #[cfg(unix)]
    fn step_out_of_update(socket: &std::path::Path) -> (Vec<serde_json::Value>, serde_json::Value) {
        use serde_json::json;
        /// Lets a program the tool left paused go on when one of its checks
        /// fails, so the native code that called it ends too.
        struct Release<'a>(&'a std::path::Path);
        impl Drop for Release<'_> {
            fn drop(&mut self) {
                if std::thread::panicking() {
                    let _ = ask(self.0, &json!({ "op": "clear" }));
                    let _ = ask(self.0, &json!({ "op": "continue" }));
                }
            }
        }
        let _release = Release(socket);
        let paused = ask(socket, &json!({ "name": "nothing" }));
        assert_eq!(paused["paused"]["function"], "update", "{paused}");
        let cleared = ask(socket, &json!({ "op": "clear" }));
        assert_eq!(cleared["cleared"], 1, "{cleared}");
        let mut steps = Vec::new();
        while steps
            .last()
            .is_none_or(|step: &serde_json::Value| step["native"] != true)
        {
            let step = ask(socket, &json!({ "op": "step", "kind": "over" }));
            assert!(steps.len() < 4, "{steps:?} then {step}");
            steps.push(step);
        }
        let next = ask(socket, &json!({ "name": "nothing" }));
        let continued = ask(socket, &json!({ "op": "continue" }));
        assert!(continued["paused"].is_null(), "{continued}");
        (steps, next)
    }

    #[cfg(unix)]
    #[test]
    fn a_step_out_of_a_call_from_native_code_is_answered_and_pauses_at_the_next_call() {
        let going = Arc::new(AtomicBool::new(true));
        let called = Called::new("step-next", &going);
        let mut counter = called.make();
        let socket = called.socket.clone();
        let tool = std::thread::spawn(move || step_out_of_update(&socket));
        while !tool.is_finished() {
            called.update(&mut counter);
            called.view(&counter);
        }
        let (steps, next) = tool.join().unwrap();
        let (last, within) = steps.split_last().unwrap();
        assert_eq!(*last, serde_json::json!({ "paused": null, "native": true }));
        assert!(
            within
                .iter()
                .all(|step| step["paused"]["function"] == "update"),
            "{steps:?}"
        );
        assert_eq!(next["paused"]["function"], "view", "{next}");
        assert_eq!(next["paused"]["reason"], "step", "{next}");
    }

    #[cfg(unix)]
    #[test]
    fn a_step_out_of_a_call_from_native_code_pauses_in_an_evaluation_already_running() {
        let going = Arc::new(AtomicBool::new(true));
        let called = Called::new("step-running", &going);
        let mut counter = called.make();
        let socket = called.socket.clone();
        std::thread::scope(|scope| {
            let (engine, ast) = (&called.engine, &called.ast);
            // Started before the step, it calls back only once woken.
            let spinning = scope.spawn(move || {
                engine
                    .call_fn::<i64>(&mut crate::Scope::new(), ast, "spin", ())
                    .unwrap_or_else(|error| panic!("{error}"))
            });
            let tool = scope.spawn(move || step_out_of_update(&socket));
            // Native code calls the program once, then waits.
            called.update(&mut counter);
            let seen = tool.join();
            going.store(false, Ordering::Release);
            assert!(spinning.join().unwrap() > 0);
            let (steps, next) = seen.unwrap();
            assert_eq!(steps.last().unwrap()["native"], true, "{steps:?}");
            assert_eq!(next["paused"]["function"], "spin", "{next}");
            assert_eq!(next["paused"]["reason"], "step", "{next}");
        });
    }

    /// A program held at its start runs nothing until released, and a
    /// breakpoint set on its first statement meanwhile pauses it there
    /// (#2274).
    #[cfg(unix)]
    #[test]
    fn a_held_program_pauses_at_a_breakpoint_set_on_its_first_statement() {
        use serde_json::json;
        let going = Arc::new(AtomicBool::new(true));
        let called = Called::new("hold-break", &going);
        called._inspector.hold();
        let socket = called.socket.clone();
        let tool = std::thread::spawn(move || {
            let set = ask(&socket, &json!({ "op": "break", "function": "make" }));
            assert!(set["paused"].is_null(), "{set}");
            let released = ask(&socket, &json!({ "op": "release" }));
            let paused = ask(&socket, &json!({ "name": "nothing" }));
            let continued = ask(&socket, &json!({ "op": "continue" }));
            (released, paused, continued)
        });
        let _ = called.make();
        let (released, paused, continued) = tool.join().unwrap();
        assert_eq!(released, json!({ "released": true }));
        assert_eq!(paused["paused"]["function"], "make", "{paused}");
        assert_eq!(paused["paused"]["reason"], "breakpoint", "{paused}");
        assert!(continued["paused"].is_null(), "{continued}");
    }

    /// Each change to the breakpoints is kept beside the socket with its
    /// revision, which the answer says too; a clear that takes none
    /// changes nothing, and a program started there again has them at the
    /// same revision (#2278).
    #[cfg(unix)]
    #[test]
    fn each_change_to_the_breakpoints_is_kept_with_its_revision() {
        use serde_json::{json, Value};
        let going = Arc::new(AtomicBool::new(true));
        let called = Called::new("kept", &going);
        let file = called.directory.join(super::BREAKPOINTS);
        let kept = || serde_json::from_slice::<Value>(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(
            kept(),
            json!({ "revision": 1, "breakpoints": [{ "function": "update", "id": 1 }] })
        );
        let set = ask(
            &called.socket,
            &json!({ "op": "break", "function": "view" }),
        );
        assert_eq!(set["revision"], 2, "{set}");
        assert_eq!(set["breakpoints"], kept()["breakpoints"], "{set}");
        let none = ask(
            &called.socket,
            &json!({ "op": "clear", "function": "spin" }),
        );
        assert_eq!(
            (&none["cleared"], &none["revision"]),
            (&json!(0), &json!(2))
        );
        assert_eq!(kept()["revision"], 2);
        let cleared = ask(
            &called.socket,
            &json!({ "op": "clear", "function": "update" }),
        );
        assert_eq!(cleared["revision"], 3, "{cleared}");
        assert_eq!(cleared["left"], json!([{ "function": "view", "id": 2 }]));
        assert_eq!(kept()["breakpoints"], cleared["left"]);
        let again = super::Inspector::bind(&called.directory.join("again")).unwrap();
        let state = again.0.state();
        assert_eq!((state.revision, state.points.len()), (3, 1));
    }

    /// A release before the program reaches its first statement lets it
    /// run without a pause; a second finds nothing held.
    #[cfg(unix)]
    #[test]
    fn a_program_released_before_it_starts_runs_without_a_pause() {
        use serde_json::json;
        let going = Arc::new(AtomicBool::new(true));
        let called = Called::new("hold-release", &going);
        called._inspector.hold();
        let released = ask(&called.socket, &json!({ "op": "release" }));
        assert_eq!(released, json!({ "released": true }));
        let _ = called.make();
        let again = ask(&called.socket, &json!({ "op": "release" }));
        assert_eq!(again, json!({ "released": false }));
    }

    /// A step that never pauses, out of a loop that runs on, is ended by a
    /// continue: both are answered, and the program runs on.
    #[cfg(unix)]
    #[test]
    fn a_continue_ends_a_step_that_never_pauses_and_answers_both() {
        use serde_json::json;
        let going = Arc::new(AtomicBool::new(true));
        let called = Called::new("step-ended", &going);
        let socket = called.socket.clone();
        let cleared = ask(&socket, &json!({ "op": "clear" }));
        assert_eq!(cleared["cleared"], 1, "{cleared}");
        let set = ask(&socket, &json!({ "op": "break", "function": "spin" }));
        assert!(set.get("error").is_none(), "{set}");
        std::thread::scope(|scope| {
            let (engine, ast) = (&called.engine, &called.ast);
            let spinning = scope.spawn(move || {
                engine
                    .call_fn::<i64>(&mut crate::Scope::new(), ast, "spin", ())
                    .unwrap_or_else(|error| panic!("{error}"))
            });
            let paused = ask(&socket, &json!({ "name": "nothing" }));
            assert_eq!(paused["paused"]["function"], "spin", "{paused}");
            ask(&socket, &json!({ "op": "clear" }));
            // Out of `spin`, which native code called and whose loop runs
            // while `going()` says so: the step pauses nowhere.
            let stepping = {
                let socket = socket.clone();
                scope.spawn(move || ask(&socket, &json!({ "op": "step", "kind": "out" })))
            };
            // The step took the program on once no pause is left.
            while !ask(&socket, &json!({ "op": "clear", "line": 0 }))["paused"].is_null() {
                std::thread::yield_now();
            }
            let continued = ask(&socket, &json!({ "op": "continue" }));
            let stepped = stepping.join().unwrap();
            going.store(false, Ordering::Release);
            assert!(spinning.join().unwrap() > 0);
            assert_eq!(continued, json!({ "paused": null }));
            assert_eq!(stepped, json!({ "paused": null }));
            // Nothing is paused or stepping now: a continue says so.
            let again = ask(&socket, &json!({ "op": "continue" }));
            assert_eq!(again["error"], "not paused", "{again}");
        });
    }

    /// Native code's call into a program that started without a debugger,
    /// on the engine of the generation it runs.
    #[cfg(unix)]
    fn tick(engine: &crate::Engine, ast: &crate::AST, count: i64) -> i64 {
        engine
            .call_fn::<i64>(&mut crate::Scope::new(), ast, "tick", (count,))
            .unwrap_or_else(|error| panic!("{error}"))
    }

    /// A program served its socket without a debugger. The first request
    /// asks it for one, which it registers at its next call from native
    /// code by compiling a generation on an engine attached to the
    /// inspector: the request reads the state native code kept.
    #[cfg(unix)]
    #[test]
    fn a_debugger_registered_at_the_next_call_reads_the_state_native_code_kept() {
        use serde_json::json;
        const TICK: &str = "fn tick(count: i64) -> i64 {\n\
            \x20   let next = count + 1;\n\
            \x20   next\n\
            }\n";
        let directory = std::env::temp_dir().join(format!("rawr-register-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let socket = directory.join("s");
        let inspector = super::Inspector::bind(&socket).unwrap();
        let (register, registered) = std::sync::mpsc::channel();
        inspector.register_on_request(move || {
            let _ = register.send(());
        });
        let compile = |inspector: Option<&super::Inspector>| {
            let mut engine = crate::Engine::new();
            if let Some(inspector) = inspector {
                inspector.attach(&mut engine);
            }
            let ast = engine
                .compile(TICK)
                .unwrap_or_else(|error| panic!("{error}"));
            (engine, ast)
        };
        let (mut engine, mut ast) = compile(None);
        let mut count = 0;
        for _ in 0..5 {
            count = tick(&engine, &ast, count);
        }
        let asking = socket.clone();
        let tool = std::thread::spawn(move || ask(&asking, &json!({ "name": "count" })));
        while !tool.is_finished() {
            if registered.try_recv().is_ok() {
                (engine, ast) = compile(Some(&inspector));
                inspector.locate_with(|position| Some(("main.rs".to_owned(), position.line()?)));
                inspector.compiled(&ast);
            }
            count = tick(&engine, &ast, count);
        }
        let read = tool.join().unwrap();
        assert_eq!(read["reached"], true, "{read}");
        let value: i64 = read["bindings"][0]["value"]
            .as_str()
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(|| panic!("{read}"));
        assert!(value >= 5, "{read}");
        // One request registered it, once.
        inspector.register_on_request(|| panic!("an attached inspector registers nothing"));
        let again = ask(&socket, &json!({ "op": "clear" }));
        assert!(again.get("error").is_none(), "{again}");
        assert!(registered.try_recv().is_err());
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A program's owner thread, which runs no async step here.
    #[cfg(unix)]
    struct Owner;

    #[cfg(unix)]
    impl crate::future::Host for Owner {
        fn invoke(
            &self,
            _: &crate::item::Value,
            _: &mut [Dynamic],
        ) -> crate::RawrResultOf<Dynamic> {
            unreachable!("the scripted loop calls the program through its engine")
        }
    }

    /// A break set on a program that runs without a debugger is answered
    /// before the debugger's generation compiles, on another thread as a
    /// runner's protocol compiles it. The call native code makes at once
    /// after the answer waits for that generation and pauses at the
    /// breakpoint (#2296).
    #[cfg(unix)]
    #[test]
    fn a_call_right_after_a_break_on_a_program_without_a_debugger_pauses() {
        use serde_json::json;
        const TICK: &str = "fn tick(count: i64) -> i64 {\n\
            \x20   let next = count + 1;\n\
            \x20   next\n\
            }\n";
        let directory = std::env::temp_dir().join(format!("rawr-expect-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let socket = directory.join("s");
        let inspector = super::Inspector::bind(&socket).unwrap();
        let compile = |inspector: Option<&super::Inspector>| {
            let mut engine = crate::Engine::new();
            if let Some(inspector) = inspector {
                inspector.attach(&mut engine);
            }
            let ast = engine
                .compile(TICK)
                .unwrap_or_else(|error| panic!("{error}"));
            (engine, ast)
        };
        // The generation the owner thread admits, which its next call runs.
        let admitted: Arc<Mutex<Option<(crate::Engine, crate::AST)>>> = Arc::default();
        let (register, registered) = std::sync::mpsc::channel();
        inspector.register_on_request(move || {
            let _ = register.send(crate::future::expect());
        });
        let returned = AtomicBool::new(false);
        std::thread::scope(|scope| {
            let protocol = {
                let (inspector, admitted) = (inspector.clone(), Arc::clone(&admitted));
                scope.spawn(move || {
                    let expected = registered.recv().unwrap();
                    let (engine, ast) = compile(Some(&inspector));
                    inspector
                        .locate_with(|position| Some(("main.rs".to_owned(), position.line()?)));
                    inspector.compiled(&ast);
                    crate::future::offer(move || {
                        *admitted.lock().unwrap() = Some((engine, ast));
                    });
                    drop(expected);
                })
            };
            let (call, calls) = std::sync::mpsc::channel::<()>();
            let (done, finished) = std::sync::mpsc::channel();
            let native = {
                let (inspector, admitted, returned) = (&inspector, &admitted, &returned);
                scope.spawn(move || {
                    crate::future::serve(std::rc::Rc::new(Owner));
                    let (mut engine, mut ast) = compile(None);
                    let mut count = 0;
                    for () in calls {
                        let entry = crate::future::entry();
                        if let Some(next) = admitted.lock().unwrap().take() {
                            (engine, ast) = next;
                        }
                        count = tick(&engine, &ast, count);
                        drop(entry);
                        returned.store(true, Ordering::Release);
                        // A wait on `changed` looks at `returned` under
                        // this lock.
                        drop(inspector.0.state());
                        inspector.0.changed.notify_all();
                        done.send(count).unwrap();
                    }
                })
            };
            for _ in 0..3 {
                call.send(()).unwrap();
                finished.recv().unwrap();
            }
            returned.store(false, Ordering::Release);
            let set = ask(&socket, &json!({ "op": "break", "function": "tick" }));
            assert_eq!(set["compiled"], false, "{set}");
            call.send(()).unwrap();
            let paused = {
                let state = inspector.0.state();
                let state = inspector
                    .0
                    .changed
                    .wait_while(state, |state| {
                        state.paused.is_none() && !returned.load(Ordering::Acquire)
                    })
                    .unwrap();
                state.paused.clone()
            };
            assert_eq!(
                paused.as_ref().map(|paused| &paused["function"]),
                Some(&json!("tick")),
                "the call after the break's answer runs on the debugger's generation"
            );
            ask(&socket, &json!({ "op": "clear" }));
            let continued = ask(&socket, &json!({ "op": "continue" }));
            assert_eq!(continued, json!({ "paused": null }));
            assert_eq!(finished.recv().unwrap(), 4);
            drop(call);
            native.join().unwrap();
            protocol.join().unwrap();
        });
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A program with a `&mut` parameter and a receiver, written to a file
    /// of its own so a breakpoint can be set on its lines.
    const STEPPED: &str = "struct Counter { value: u64, history: Vec<u64> }\n\
        fn tick(counter: &mut Counter, step: u64) {\n\
        \x20   counter.value += step;\n\
        }\n\
        impl Counter {\n\
        \x20   fn update(&mut self, message: u64) {\n\
        \x20       match message {\n\
        \x20           0 => {\n\
        \x20               self.history.push(self.value);\n\
        \x20               self.value += 1;\n\
        \x20           }\n\
        \x20           _ => self.value -= 1,\n\
        \x20       }\n\
        \x20   }\n\
        }\n\
        pub fn run() -> u64 {\n\
        \x20   let mut c = Counter { value: 0, history: vec![] };\n\
        \x20   tick(&mut c, 2);\n\
        \x20   c.update(0);\n\
        \x20   c.value\n\
        }\n";

    /// Where [`STEPPED`]'s `run` pauses: at a breakpoint on `line`, then
    /// after each step of `steps`.
    #[cfg(unix)]
    fn stepped(name: &str, line: usize, steps: &[&str]) -> Vec<serde_json::Value> {
        stepped_in(STEPPED, name, line, steps)
    }

    /// Where `source`'s `run`, which gives 3, pauses: at a breakpoint on
    /// `line`, then after each step of `steps`.
    #[cfg(unix)]
    fn stepped_in(source: &str, name: &str, line: usize, steps: &[&str]) -> Vec<serde_json::Value> {
        use serde_json::json;
        let directory = std::env::temp_dir().join(format!("rawr-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let file = directory.join("main.rs");
        std::fs::write(&file, source).unwrap();
        let socket = directory.join("s");
        let inspector = super::Inspector::bind(&socket).unwrap();
        let mut engine = crate::Engine::new();
        inspector.attach(&mut engine);
        // Compiled as a runner compiles: its steps placed by the compiler's
        // positions, in the file written above.
        let executable = engine
            .compile_executable(source)
            .unwrap_or_else(|error| panic!("{error}"));
        let located = executable.located().unwrap();
        let (ast, _) = executable.into_ast_with_positions();
        let path = file.to_string_lossy().into_owned();
        let at = path.clone();
        inspector.locate_with(move |position| {
            located.line(position).map(|(_, line)| (at.clone(), line))
        });
        inspector.compiled(&ast);
        let set = ask(
            &socket,
            &json!({ "op": "break", "file": path, "line": line }),
        );
        assert!(set.get("error").is_none(), "{set}");
        let seen = std::thread::scope(|scope| {
            let running = scope.spawn(|| {
                engine
                    .call_fn::<u64>(&mut crate::Scope::new(), &ast, "run", ())
                    .unwrap_or_else(|error| panic!("{error}"))
            });
            let mut seen = vec![ask(&socket, &json!({ "name": "nothing" }))["paused"].clone()];
            for kind in steps {
                seen.push(ask(&socket, &json!({ "op": "step", "kind": kind }))["paused"].clone());
            }
            ask(&socket, &json!({ "op": "clear" }));
            ask(&socket, &json!({ "op": "continue" }));
            assert_eq!(running.join().unwrap(), 3);
            seen
        });
        let _ = std::fs::remove_dir_all(&directory);
        seen
    }

    /// A compile that moves a breakpoint's line keeps the breakpoint's id,
    /// so a shell tells it moved, not cleared and set again (#2299).
    #[cfg(unix)]
    #[test]
    fn a_breakpoint_a_compile_moves_keeps_its_id() {
        use serde_json::{json, Value};
        let directory = std::env::temp_dir().join(format!("rawr-moved-id-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let file = directory.join("main.rs");
        std::fs::write(&file, STEPPED).unwrap();
        let socket = directory.join("s");
        let inspector = super::Inspector::bind(&socket).unwrap();
        let mut engine = crate::Engine::new();
        inspector.attach(&mut engine);
        let path = file.to_string_lossy().into_owned();
        let compile = |source: &str| {
            let executable = engine
                .compile_executable(source)
                .unwrap_or_else(|error| panic!("{error}"));
            let located = executable.located().unwrap();
            let (ast, _) = executable.into_ast_with_positions();
            let at = path.clone();
            inspector.locate_with(move |position| {
                located.line(position).map(|(_, line)| (at.clone(), line))
            });
            inspector.compiled(&ast);
        };
        compile(STEPPED);
        let set = ask(&socket, &json!({ "op": "break", "file": path, "line": 18 }));
        assert!(set["set"]["id"].is_u64(), "{set}");
        let moved = format!("\n{STEPPED}");
        std::fs::write(&file, &moved).unwrap();
        compile(&moved);
        let kept: Value =
            serde_json::from_slice(&std::fs::read(directory.join(super::BREAKPOINTS)).unwrap())
                .unwrap();
        assert_eq!(kept["breakpoints"][0]["line"], 19, "{kept}");
        assert_eq!(kept["breakpoints"][0]["id"], set["set"]["id"], "{kept}");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[cfg(unix)]
    #[test]
    fn a_step_into_a_function_with_a_mut_parameter_pauses_at_its_first_line() {
        let seen = stepped("step-into-mut", 18, &["into"]);
        assert_eq!(seen[0]["line"], 18, "{seen:?}");
        assert_eq!(seen[1]["line"], 3, "{seen:?}");
        assert_eq!(seen[1]["reason"], "step", "{seen:?}");
    }

    #[cfg(unix)]
    #[test]
    fn a_step_over_a_breakpoint_line_pauses_at_the_next_line() {
        let seen = stepped("step-over-break", 9, &["over", "over"]);
        assert_eq!(seen[0]["line"], 9, "{seen:?}");
        assert_eq!(seen[1]["line"], 10, "{seen:?}");
        assert_eq!(seen[1]["reason"], "step", "{seen:?}");
        // Out of the arm's block, whose value no statement wrote, to the
        // caller's next line.
        assert_eq!(seen[2]["line"], 20, "{seen:?}");
        // A step into the method from its caller passes its signature too.
        let seen = stepped("step-into-method", 19, &["into"]);
        assert_eq!(seen[1]["line"], 7, "{seen:?}");
    }

    /// Parameters the lowering binds again where the body starts: a `mut`
    /// binding of a `&mut`, a pattern that only one variant matches and a
    /// closure's pattern. A `&mut` of a local is stored back after a call.
    const REBOUND: &str = "struct Counter { value: u64 }\n\
        enum Wrapped { Only(u64) }\n\
        fn bump(counter: &mut Counter, mut by: &mut u64) {\n\
        \x20   counter.value += *by;\n\
        }\n\
        fn take(Wrapped::Only(step): Wrapped, counter: &mut Counter) {\n\
        \x20   counter.value += step;\n\
        }\n\
        pub fn run() -> u64 {\n\
        \x20   let mut c = Counter { value: 0 };\n\
        \x20   let mut one = 1;\n\
        \x20   bump(&mut c, &mut one);\n\
        \x20   take(Wrapped::Only(2), &mut c);\n\
        \x20   let f = |(a, b): (u64, u64)| {\n\
        \x20       let sum = a + b;\n\
        \x20       sum\n\
        \x20   };\n\
        \x20   c.value + f((0, 0))\n\
        }\n";

    #[cfg(unix)]
    #[test]
    fn a_step_into_a_function_that_binds_a_parameter_again_pauses_at_its_first_line() {
        let steps = ["into", "over", "into", "over", "over", "into"];
        let seen = stepped_in(REBOUND, "step-into-rebound", 12, &steps);
        assert_eq!(seen[0]["line"], 12, "{seen:?}");
        // `mut by: &mut u64` is `let mut by = ..;` at the parameter, which
        // the lowering wrote.
        assert_eq!(seen[1]["line"], 4, "{seen:?}");
        // Out of the call, past the stores of `&mut one` back into `one`.
        assert_eq!(seen[2]["line"], 13, "{seen:?}");
        // `Wrapped::Only(step)` is matched as a `let` at the parameter.
        assert_eq!(seen[3]["line"], 7, "{seen:?}");
        assert_eq!(seen[4]["line"], 14, "{seen:?}");
        assert_eq!(seen[5]["line"], 18, "{seen:?}");
        // `(a, b)` is bound from the closure's argument at its header.
        assert_eq!(seen[6]["line"], 15, "{seen:?}");
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
