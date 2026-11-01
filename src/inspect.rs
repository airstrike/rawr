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
#![cfg(feature = "debugging")]

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, PoisonError};
use std::time::Duration;

use serde_json::{json, Value};

use crate::debugger::{DebuggerCommand, DebuggerEvent};
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

struct Request {
    id: u64,
    name: Option<String>,
    file: Option<String>,
    reply: mpsc::Sender<Value>,
}

#[derive(Default)]
struct Shared {
    pending: Mutex<Vec<Request>>,
    wake: Arc<AtomicBool>,
    /// The callback is answering: steps of the `Debug` code it runs are not
    /// answered.
    answering: AtomicBool,
}

impl Shared {
    fn pending(&self) -> std::sync::MutexGuard<'_, Vec<Request>> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Serve requests for the values of the program `engine` runs on a Unix
/// socket at `socket`, replacing a stale one there. `locate` maps a step's
/// position to its file and line.
///
/// # Errors
///
/// When the socket can't be bound.
#[cfg(unix)]
pub fn serve(
    engine: &mut Engine,
    socket: &std::path::Path,
    locate: Box<Locate>,
) -> std::io::Result<()> {
    use std::os::unix::net::UnixListener;

    let _ = std::fs::remove_file(socket);
    let listener = UnixListener::bind(socket)?;
    let shared = Arc::new(Shared::default());
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
    engine.wake_debugger_on(Arc::clone(&shared.wake));
    #[allow(deprecated)]
    engine.register_debugger(
        |_, debugger| debugger,
        move |mut context, event, _node, _source, position| {
            if matches!(event, DebuggerEvent::Wake) {
                shared.answer(&mut context, position, &*locate);
            }
            Ok(DebuggerCommand::Continue)
        },
    );
    Ok(())
}

impl Shared {
    /// Take one request from `stream` and answer it.
    #[cfg(unix)]
    fn handle(&self, stream: std::os::unix::net::UnixStream) {
        use std::io::{BufRead as _, BufReader, Write as _};

        static NEXT: AtomicU64 = AtomicU64::new(0);
        let mut line = String::new();
        let Ok(reading) = stream.try_clone() else {
            return;
        };
        if BufReader::new(reading).read_line(&mut line).is_err() {
            return;
        }
        let request: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
        let text = |key: &str| request.get(key).and_then(Value::as_str).map(str::to_owned);
        let timeout = request
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .map_or(TIMEOUT, Duration::from_millis);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let (reply, answer) = mpsc::channel();
        self.pending().push(Request {
            id,
            name: text("name"),
            file: text("file"),
            reply,
        });
        self.wake.store(true, Ordering::Release);
        let answer = answer.recv_timeout(timeout).unwrap_or_else(|_| {
            let mut pending = self.pending();
            pending.retain(|request| request.id != id);
            if pending.is_empty() {
                self.wake.store(false, Ordering::Release);
            }
            drop(pending);
            // An answer made while the request was being given up.
            answer
                .try_recv()
                .unwrap_or_else(|_| json!({ "reached": false }))
        });
        let mut stream = stream;
        let _ = writeln!(stream, "{answer}");
    }

    /// At a step of the program: answer each waiting request this step
    /// matches.
    fn answer(&self, context: &mut EvalContext, position: Position, locate: &Locate) {
        if self.answering.swap(true, Ordering::AcqRel) {
            return;
        }
        let mut location = None;
        let mut located = false;
        let mut matched = Vec::new();
        {
            let mut pending = self.pending();
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
            let function = function(context);
            for request in matched {
                let found = match &request.name {
                    Some(name) => binding(context, name).into_iter().collect(),
                    None => bindings(context),
                };
                let _ = request.reply.send(json!({
                    "reached": true,
                    "file": location.as_ref().map(|(file, _)| file),
                    "line": location.as_ref().map(|(_, line)| line),
                    "function": function,
                    "bindings": found.iter().map(Binding::to_json).collect::<Vec<_>>(),
                }));
            }
        }
        self.answering.store(false, Ordering::Release);
    }
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
