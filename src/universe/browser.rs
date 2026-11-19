//! A universe in a browser (#1395): the entry of a generated runner built
//! for wasm32. Its `main` hands its bindings to [`serve`] as the page
//! instantiates the module, and the page then calls [`prepare`] and [`run`],
//! and [`replace`] for each edit, which `wasm-bindgen` exports. A page that
//! runs a whole package, as `rawr run --web` serves one, calls [`start`]
//! with the package's files and [`replace_files`] for each save.
//!
//! One module instance runs one program. A program that hands its loop to
//! native code, as iced's `run` hands it to its web shell, ratzilla's
//! `draw_web` to the page's animation frames, or `spawn_local` an async
//! loop of the program's own that awaits its input, returns from `main` at
//! once and keeps running on the page's events, and this instance belongs
//! to it from then on. An edit reloads it in place, its state kept, as `rawr run`
//! reloads a save (#1455). The page instantiates a fresh copy of the
//! compiled module for the next run, and for an edit that can't reload.
//!
//! A `main` that loops in script code and sleeps hands the page its time
//! instead: [`run`] starts `main` on a stack the page can suspend
//! (`WebAssembly.promising`), each `std::thread::sleep` there suspends it
//! while a timer of the page's runs, and the page's events run meanwhile.
//! [`run`]'s answer then waits for `main` to return; [`sleeping`] says at
//! once that it runs on. An edit reloads it while it sleeps, and `main`
//! goes on in the new code from its next statement (`island::follow`).
//!
//! Built with the `browser-debugger` feature, the universe runs every
//! program under a debugger the page asks (`super::debugger`).

// wasm-bindgen marks its JSPI attribute and call deprecated, as experimental.
#![allow(deprecated)]

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use serde_json::{json, Value};
use wasm_bindgen::prelude::{wasm_bindgen, JsError, JsValue};

use super::{Bindings, Error, Outcome, Program, Universe};
use crate::{future, Access, Output};

/// Where a snippet's package lies, as its paths name it.
const ROOT: &str = "/universe";
/// Most output kept, in bytes, as `?play`'s server keeps it.
const MAX_OUTPUT: usize = 64 * 1024;

#[wasm_bindgen]
extern "C" {
    /// A page's function that takes what the program writes as it writes
    /// it: `(stream, text)`, the stream `"stdout"` or `"stderr"`.
    pub type Sink;

    #[wasm_bindgen(method, js_name = call)]
    fn write(this: &Sink, context: &JsValue, stream: &str, text: &str);
}

thread_local! {
    /// The bindings the runner's `main` served.
    static BINDINGS: Cell<Option<&'static dyn Bindings>> = const { Cell::new(None) };
    /// The universe [`prepare`] made, which [`run`] takes.
    static UNIVERSE: RefCell<Option<Universe>> = const { RefCell::new(None) };
    /// The program [`run`] loaded, with its universe: it keeps running once
    /// `main` returns.
    static PROGRAM: RefCell<Option<(Universe, Program)>> = const { RefCell::new(None) };
    /// Whether native code called into the program since its `main`
    /// returned: only then does it run on, and an edit reload it.
    static CALLED: Cell<bool> = const { Cell::new(false) };
    /// The page's function [`set_output`] named, which takes the program's
    /// output in place of the answers.
    static SINK: RefCell<Option<Sink>> = const { RefCell::new(None) };
    /// Whether `main` runs: it hasn't returned, so outside its own calls it
    /// waits in a sleep.
    static MAIN: Cell<bool> = const { Cell::new(false) };
    /// Whether this call runs on the stack [`run`] started `main` on, which
    /// a sleep may suspend. A call from the page while `main` sleeps runs
    /// on a stack of its own.
    static SUSPENDABLE: Cell<bool> = const { Cell::new(false) };
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = setTimeout)]
    fn set_timeout(callback: &js_sys::Function, milliseconds: f64);
}

/// What the program wrote, and the message of the panic that stopped it.
#[derive(Default)]
struct Written {
    chunks: Vec<(Output, String)>,
    bytes: usize,
    panic: Option<String>,
}

static WRITTEN: Mutex<Written> = Mutex::new(Written {
    chunks: Vec::new(),
    bytes: 0,
    panic: None,
});

/// Serve `bindings` to the page: what a runner's `main` calls on wasm32.
/// A panic's message is kept for [`panic_message`], since a panic on
/// wasm32 aborts the instance, and goes to the page's sink as it happens.
pub fn serve(bindings: &'static dyn Bindings) {
    std::panic::set_hook(Box::new(|info| {
        let message = info.to_string();
        SINK.with_borrow(|sink| {
            if let Some(sink) = sink {
                sink.write(&JsValue::NULL, "stderr", &format!("{message}\n"));
            }
        });
        written().panic = Some(message);
    }));
    future::on_sleep(sleep);
    BINDINGS.set(Some(bindings));
}

/// Wait out a sleep of the program's: on `main`'s own stack, suspend it
/// while a timer of the page's runs, so the page runs its events
/// meanwhile; elsewhere, as a call from the page can't be suspended, wait
/// on the clock.
fn sleep(duration: std::time::Duration) {
    if !SUSPENDABLE.replace(false) {
        future::wait(duration);
        return;
    }
    let milliseconds = duration.as_secs_f64() * 1000.0;
    let timer = js_sys::Promise::new(&mut |resolve, _| set_timeout(&resolve, milliseconds));
    // A page without JSPI refuses to suspend, with its reason: the program
    // stops here, and the reason is what stopped it.
    if let Err(reason) = js_sys::futures::jspi_block_on_promise(&timer) {
        let reason = reason
            .as_string()
            .unwrap_or_else(|| "rawr: this page can't suspend the program".to_owned());
        written().panic = Some(reason.clone());
        MAIN.set(false);
        wasm_bindgen::throw_str(&reason);
    }
    SUSPENDABLE.set(true);
}

/// Suspend the stack this call runs on, `main`'s or an entered callback's,
/// until `promise` settles, the page running its events meanwhile: what a
/// pause waits in. Other stacks run meanwhile, so the flag that says which
/// stack runs is cleared, and this stack's own comes back on resume. `Err`
/// is the page's reason it can't, as a page without JSPI gives.
#[cfg(feature = "browser-debugger")]
pub(super) fn suspend(promise: &js_sys::Promise) -> Result<(), JsValue> {
    let suspendable = SUSPENDABLE.replace(false);
    let suspended = js_sys::futures::jspi_block_on_promise(promise);
    SUSPENDABLE.set(suspendable);
    suspended.map(drop)
}

/// Whether the prepared universe has run no program yet: what a hold
/// before `run` needs.
#[cfg(feature = "browser-debugger")]
pub(super) fn unstarted() -> bool {
    UNIVERSE.with_borrow(Option::is_some)
}

/// Whether this call runs on `main`'s stack, which [`suspend`] suspends.
#[cfg(feature = "browser-debugger")]
pub(super) fn on_main_stack() -> bool {
    SUSPENDABLE.get()
}

/// Send what the program writes to `sink` as it writes it, rather than
/// with the answer of the call it wrote it in: a program that runs on the
/// page's events writes between calls from the page. What it wrote before
/// goes to `sink` first.
#[wasm_bindgen]
pub fn set_output(sink: Sink) {
    for (stream, text) in std::mem::take(&mut written().chunks) {
        sink.write(&JsValue::NULL, stream_name(stream), &text);
    }
    SINK.set(Some(sink));
}

/// Whether the program's `main` runs on, waiting in a sleep: what the page
/// asks once [`run`] hands it the answer's promise.
#[wasm_bindgen]
pub fn sleeping() -> bool {
    MAIN.get()
}

/// Install the served bindings in a universe ahead of [`run`], so a Run
/// pays only for the snippet's own compile.
///
/// # Errors
///
/// Returns the error installing the bindings gave, or that the runner
/// served none.
#[wasm_bindgen]
pub fn prepare() -> Result<(), JsError> {
    if UNIVERSE.with_borrow(Option::is_some) {
        return Ok(());
    }
    let Some(bindings) = BINDINGS.get() else {
        return Err(JsError::new(
            "rawr: this module's runner served no bindings",
        ));
    };
    // A snippet reaches none of the page's files or environment.
    let mut universe = Universe::new(bindings, Access::none())?;
    #[cfg(feature = "browser-debugger")]
    super::debugger::attach(&mut universe);
    universe.on_output(|stream, text| {
        let sent = SINK.with_borrow(|sink| {
            sink.as_ref()
                .map(|sink| sink.write(&JsValue::NULL, stream_name(stream), text))
        });
        if sent.is_none() {
            written().push(stream, text);
        }
    });
    UNIVERSE.set(Some(universe));
    Ok(())
}

/// Compile `source` as the crate root of the package whose `Cargo.toml` is
/// `manifest`, against the served bindings, and run its `main` as Rust runs
/// it. The answer is `?play`'s server's JSON text result:
/// `{"kind":"text","status":..,"output":[..]}`, or
/// `{"kind":"error","text":..}` when the snippet doesn't compile. The
/// instance runs one program: the universe is the program's from then on.
///
/// The page gets the answer as a promise: `main` runs on a stack a sleep
/// suspends, and the promise settles once `main` returns. Meanwhile
/// [`sleeping`] is true.
///
/// # Errors
///
/// Returns the error installing the bindings gave.
#[wasm_bindgen(jspi)]
pub fn run(manifest: &str, source: &str) -> Result<String, JsError> {
    let root = PathBuf::from(ROOT);
    let files = vec![(root.join("Cargo.toml"), Box::from(manifest))];
    let features = crate::project::cargo::features::Enabled::default();
    SUSPENDABLE.set(true);
    let answer = started(
        root.join("src").join("main.rs"),
        Box::from(source),
        files,
        features,
    );
    SUSPENDABLE.set(false);
    answer
}

/// Compile the package `files` holds, a JSON array of `[path, text]`
/// pairs, with its crate root at `entry`, and run its `main`, as [`run`]
/// runs a snippet's, with the same answers. Errors show the package's
/// paths from `directory`, where the command that serves the page started,
/// as that command shows them. `cfg`, a JSON array of lines as `rustc
/// --print cfg` writes them, holds the features and options Cargo resolved
/// for the package built for this target, which its `cfg(..)`s read.
///
/// # Errors
///
/// Returns the error installing the bindings gave, or that `files` or
/// `cfg` isn't such an array.
#[wasm_bindgen]
pub fn start(entry: &str, files: &str, directory: &str, cfg: &str) -> Result<String, JsError> {
    crate::diagnostic::show_paths_from(PathBuf::from(directory));
    let entry = PathBuf::from(entry);
    let mut files = package(files)?;
    let Some(at) = files.iter().position(|(path, _)| *path == entry) else {
        return Err(JsError::new(&format!(
            "rawr: the package's files have no crate root `{}`",
            entry.display()
        )));
    };
    let (_, text) = files.swap_remove(at);
    let lines = serde_json::from_str::<Vec<String>>(cfg)
        .map_err(|error| JsError::new(&format!("rawr: the package's cfg doesn't read: {error}")))?;
    let features =
        crate::project::cargo::features::Enabled::of_lines(lines.iter().map(String::as_str));
    started(entry, text, files, features)
}

/// Load the program whose crate root at `entry` holds `text`, with `files`
/// for the rest of its package and `features` for its `cfg(..)`s, and run
/// its `main`.
fn started(
    entry: PathBuf,
    text: Box<str>,
    files: Vec<(PathBuf, Box<str>)>,
    features: crate::project::cargo::features::Enabled,
) -> Result<String, JsError> {
    prepare()?;
    let Some(universe) = UNIVERSE.take() else {
        return Err(JsError::new("rawr: this instance has run its program"));
    };
    let program = match universe.program_with_features(entry, text, files, features) {
        Ok(program) => program,
        Err(error) => return Ok(failure(&error).to_string()),
    };
    // The program is the page's before `main` runs, so an edit made while
    // `main` sleeps reloads it.
    PROGRAM.set(Some((universe, program)));
    MAIN.set(true);
    let status =
        PROGRAM.with_borrow(|running| running.as_ref().map_or(0, |(_, program)| program.run()));
    MAIN.set(false);
    // The next call from native code into the program takes this offer
    // first, which says the program runs on past `main`.
    future::offer(|| CALLED.set(true));
    let output = taken_output();
    Ok(json!({ "kind": "text", "status": status, "output": output }).to_string())
}

/// Give the running program `source` as its crate root's new text, and
/// take it at once, its state kept, as `rawr run` takes a save. The answer
/// is JSON: `{"kind":"swapped","output":[..]}` with what the program wrote
/// since, `{"kind":"unchanged"}` for the text it runs or the one refused
/// last, `{"kind":"error","text":..}` for a compile error, which leaves the
/// program running its last good text, and `{"kind":"restart","reason":..}`
/// when the text can't replace the running one in this instance, or no
/// program runs on: the page runs it in a fresh one. An error at a use of a
/// crate's API the bindings don't cover says `"uncovered":true`: a runner
/// built for the new text runs it.
#[wasm_bindgen]
pub fn replace(source: &str) -> String {
    replaced(|program| program.replace_now(source))
}

/// Give the running program the package `files` holds, a JSON array of
/// `[path, text]` pairs, as its new text, and take it at once, as
/// [`replace`] takes a new crate root, with the same answers.
///
/// # Errors
///
/// Returns that `files` isn't such an array.
#[wasm_bindgen]
pub fn replace_files(files: &str) -> Result<String, JsError> {
    let files = package(files)?;
    Ok(replaced(|program| program.replace_package_now(files)))
}

/// The answer to an edit `offer` gives the running program.
fn replaced(offer: impl FnOnce(&Program) -> Outcome) -> String {
    let outcome = PROGRAM.with_borrow(|running| match running {
        None => Outcome::Restarting {
            reason: "no program runs".to_owned(),
        },
        Some(_) if !CALLED.get() && !MAIN.get() => Outcome::Restarting {
            reason: "the program has ended".to_owned(),
        },
        Some((_, program)) => offer(program),
    });
    let answer = match outcome {
        Outcome::Unchanged => json!({ "kind": "unchanged" }),
        // Queued: the program's next call from native code takes it.
        Outcome::Swapped | Outcome::Queued => {
            json!({ "kind": "swapped", "output": taken_output() })
        }
        Outcome::Failed { message } => json!({ "kind": "error", "text": message }),
        Outcome::Uncovered { message } => {
            json!({ "kind": "error", "text": message, "uncovered": true })
        }
        Outcome::Restarting { reason } => json!({ "kind": "restart", "reason": reason }),
    };
    answer.to_string()
}

/// The answer for a program that didn't load.
fn failure(error: &Error) -> Value {
    let text = error.to_string();
    match error {
        Error::Uncovered(_) => json!({ "kind": "error", "text": text, "uncovered": true }),
        _ => json!({ "kind": "error", "text": text }),
    }
}

/// The `[path, text]` pairs of a JSON array.
fn package(files: &str) -> Result<Vec<(PathBuf, Box<str>)>, JsError> {
    serde_json::from_str::<Vec<(String, String)>>(files)
        .map(|files| {
            files
                .into_iter()
                .map(|(path, text)| (PathBuf::from(path), text.into_boxed_str()))
                .collect()
        })
        .map_err(|error| JsError::new(&format!("rawr: the package's files don't read: {error}")))
}

/// The message of the panic that aborted this instance, if one did.
#[wasm_bindgen]
pub fn panic_message() -> Option<String> {
    written().panic.clone()
}

fn written() -> std::sync::MutexGuard<'static, Written> {
    WRITTEN.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What the program wrote since the last answer, as `?play`'s server gives
/// output: `{"stream":"stdout"|"stderr","text":..}` chunks.
fn taken_output() -> Vec<Value> {
    let written = std::mem::take(&mut *written());
    written
        .chunks
        .iter()
        .map(|(stream, text)| json!({ "stream": stream_name(*stream), "text": text }))
        .collect()
}

/// The page's name of `stream`.
fn stream_name(stream: Output) -> &'static str {
    if stream == Output::Stderr {
        "stderr"
    } else {
        "stdout"
    }
}

impl Written {
    /// Keep `text`, up to [`MAX_OUTPUT`] bytes in all.
    fn push(&mut self, stream: Output, text: &str) {
        let room = MAX_OUTPUT.saturating_sub(self.bytes);
        let mut end = text.len().min(room);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let text = &text[..end];
        if text.is_empty() {
            return;
        }
        self.bytes += text.len();
        match self.chunks.last_mut() {
            Some((last, chunk)) if *last == stream => chunk.push_str(text),
            _ => self.chunks.push((stream, text.to_owned())),
        }
    }
}
