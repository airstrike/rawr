//! A universe in a browser (#1395): the entry of a generated runner built
//! for wasm32. Its `main` hands its bindings to [`serve`] as the page
//! instantiates the module, and the page then calls [`prepare`] and [`run`],
//! and [`replace`] for each edit, which `wasm-bindgen` exports.
//!
//! One module instance runs one program. A program that hands its loop to
//! native code, as iced's `run` hands it to its web shell, ratzilla's
//! `draw_web` to the page's animation frames, or `spawn_local` an async
//! loop of the program's own that awaits its input, returns from `main` at
//! once and keeps running on the page's events, and this instance belongs
//! to it from then on. An edit reloads it in place, its state kept, as `rawr run`
//! reloads a save (#1455). The page instantiates a fresh copy of the
//! compiled module for the next run, and for an edit that can't reload.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use serde_json::{json, Value};
use wasm_bindgen::prelude::{wasm_bindgen, JsError};

use super::{Bindings, Outcome, Program, Universe};
use crate::{future, Access, Output};

/// Where a snippet's package lies, as its paths name it.
const ROOT: &str = "/universe";
/// Most output kept, in bytes, as `?play`'s server keeps it.
const MAX_OUTPUT: usize = 64 * 1024;

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
/// wasm32 aborts the instance.
pub fn serve(bindings: &'static dyn Bindings) {
    std::panic::set_hook(Box::new(|info| {
        written().panic = Some(info.to_string());
    }));
    BINDINGS.set(Some(bindings));
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
    universe.on_output(|stream, text| written().push(stream, text));
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
/// # Errors
///
/// Returns the error installing the bindings gave.
#[wasm_bindgen]
pub fn run(manifest: &str, source: &str) -> Result<String, JsError> {
    prepare()?;
    let Some(universe) = UNIVERSE.take() else {
        return Err(JsError::new("rawr: this instance has run its program"));
    };
    let root = PathBuf::from(ROOT);
    let files = vec![(root.join("Cargo.toml"), Box::from(manifest))];
    let program = match universe.program(root.join("src").join("main.rs"), source, files) {
        Ok(program) => program,
        Err(error) => {
            let text = error.to_string();
            return Ok(json!({ "kind": "error", "text": text }).to_string());
        }
    };
    let status = program.run();
    PROGRAM.set(Some((universe, program)));
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
/// program runs on: the page runs it in a fresh one.
#[wasm_bindgen]
pub fn replace(source: &str) -> String {
    let outcome = PROGRAM.with_borrow(|running| match running {
        None => Outcome::Restarting {
            reason: "no program runs".to_owned(),
        },
        Some(_) if !CALLED.get() => Outcome::Restarting {
            reason: "the program has ended".to_owned(),
        },
        Some((_, program)) => program.replace_now(source),
    });
    let answer = match outcome {
        Outcome::Unchanged => json!({ "kind": "unchanged" }),
        // Queued: the program's next call from native code takes it.
        Outcome::Swapped | Outcome::Queued => {
            json!({ "kind": "swapped", "output": taken_output() })
        }
        Outcome::Failed { message } => json!({ "kind": "error", "text": message }),
        Outcome::Restarting { reason } => json!({ "kind": "restart", "reason": reason }),
    };
    answer.to_string()
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
        .map(|(stream, text)| {
            let stream = if *stream == Output::Stderr {
                "stderr"
            } else {
                "stdout"
            };
            json!({ "stream": stream, "text": text })
        })
        .collect()
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
