//! A plain program in the browser: a snippet rawr-iced does not claim runs
//! on the base language, as the `?play` server runs it, and its output comes
//! back as the server's text result.

use std::cell::RefCell;
use std::sync::{Arc, Mutex};

use rawr::{CallFnOptions, Dynamic, Engine, Scope};
use rawr_snippet::{plain, SOURCE_NAME};

/// Most output kept, in bytes, as on the server.
const MAX_OUTPUT: usize = 64 * 1024;
/// What ends output cut at [`MAX_OUTPUT`].
const TRUNCATED: &str = "… output truncated";
/// The status of a program that panicked, as Rust's.
pub(crate) const PANIC_STATUS: i32 = 101;
/// Time before a program is stopped, in milliseconds. The page waits for
/// the program, so a long one is cut rather than left to hang the tab.
pub(crate) const TIME_MS: f64 = 10_000.0;
pub(crate) const TIME_LIMIT: &str = "time limit exceeded";
pub(crate) const OUTPUT_LIMIT: &str = "output limit exceeded";
/// The server's limits on call depth and data sizes.
const CALL_DEPTH: usize = 32;
const DATA_SIZE: usize = 64 * 1024;
/// The note on a final iterator, as rustc's warning on an unused one.
const UNUSED_ITERATOR: &str =
    "This iterator is never used. Iterators are lazy and do nothing unless consumed.";

thread_local! {
    /// Where a running program's output also goes as it is written: the
    /// page's callback, given `(stream, text)`. None for a program the page
    /// waits for.
    static STREAM: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
}

/// Send the output written from now on to `callback` as well.
pub(crate) fn stream(callback: Option<js_sys::Function>) {
    STREAM.with(|stream| *stream.borrow_mut() = callback);
}

/// A program's output, in the order it was written.
#[derive(Default)]
pub(crate) struct Output {
    /// Runs of text, each `(stderr, text)`.
    chunks: Vec<(bool, String)>,
    pub(crate) status: i32,
    pub(crate) truncated: bool,
    pub(crate) limit: bool,
    note: Option<&'static str>,
}

impl Output {
    pub(crate) fn push(&mut self, stderr: bool, text: &str) {
        let used: usize = self.chunks.iter().map(|(_, text)| text.len()).sum();
        if self.truncated || text.is_empty() {
            return;
        }
        let room = MAX_OUTPUT.saturating_sub(used);
        let text = if text.len() > room {
            self.truncated = true;
            let mut end = room;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            &text[..end]
        } else {
            text
        };
        match self.chunks.last_mut() {
            Some((last, chunk)) if *last == stderr => chunk.push_str(text),
            _ => self.chunks.push((stderr, text.to_owned())),
        }
        STREAM.with(|stream| {
            if let Some(callback) = &*stream.borrow() {
                let name = if stderr { "stderr" } else { "stdout" };
                let _ = callback.call2(&wasm_bindgen::JsValue::NULL, &name.into(), &text.into());
            }
        });
    }

    /// The server's text result, with the truncation marker when output
    /// was cut.
    pub(crate) fn finish(mut self) -> String {
        if self.truncated {
            let marker = format!("\n{TRUNCATED}\n");
            match self.chunks.last_mut() {
                Some((true, chunk)) => chunk.push_str(&marker),
                _ => self.chunks.push((true, marker)),
            }
            self.status = 1;
            self.limit = true;
        }
        self.to_json()
    }

    /// The server's text result: `{"kind":"text","status":0,"output":[..]}`.
    fn to_json(&self) -> String {
        let output: Vec<String> = self
            .chunks
            .iter()
            .map(|(stderr, text)| {
                let stream = if *stderr { "stderr" } else { "stdout" };
                format!(r#"{{"stream":"{stream}","text":{}}}"#, json(text))
            })
            .collect();
        format!(
            r#"{{"kind":"text","status":{},"output":[{}],"truncated":{},"limit":{},"note":{}}}"#,
            self.status,
            output.join(","),
            self.truncated,
            self.limit,
            self.note.map_or_else(|| "null".to_owned(), json),
        )
    }
}

/// `text` as a JSON string.
fn json(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A compile error, as `{"kind":"error","text":".."}`.
pub fn error(text: &str) -> String {
    format!(r#"{{"kind":"error","text":{}}}"#, json(text))
}

/// Run a plain snippet once; its output as the server's text result, or a
/// compile error as [`error`].
pub fn run(source: &str) -> String {
    let sink = Arc::new(Mutex::new(Output::default()));
    let engine = engine(&sink);
    let mut program = plain::program(source);
    let mut note = None;
    let compiled = match compile(&engine, &program) {
        Ok(compiled) => Ok(compiled),
        Err(failure) => match failure
            .0
            .as_ref()
            .and_then(|report| unprintable(report, source))
        {
            // The final value has no `Debug`: run it without printing it.
            Some(owner) => {
                note = owner
                    .starts_with("impl Iterator")
                    .then_some(UNUSED_ITERATOR);
                program = plain::quiet_program(source);
                compile(&engine, &program)
            }
            None => Err(failure),
        },
    };
    let (ast, positions, evidence) = match compiled {
        Ok(compiled) => compiled,
        Err((Some(report), _)) => {
            return error(&rawr_snippet::report(
                &rawr::diagnostic::render(&report),
                &program,
                source,
            ));
        }
        Err((None, text)) => return error(&text),
    };
    let mut scope = Scope::new();
    let ran = engine
        .eval_ast_with_scope::<Dynamic>(&mut scope, &ast)
        .and_then(|value| {
            let main = ast
                .iter_functions()
                .any(|function| function.name == "main" && function.params.is_empty());
            if !main {
                // A script's final value, as `rawr run` shows it.
                if !value.is_unit() {
                    sink.lock().unwrap().push(false, &format!("{value:?}\n"));
                }
                return Ok(None);
            }
            let returned = engine.call_fn_with_options::<Dynamic>(
                CallFnOptions::new().eval_ast(false).rewind_scope(false),
                &mut scope,
                &ast,
                "main",
                (),
            )?;
            Ok(match returned.try_cast::<Result<Dynamic, Dynamic>>() {
                Some(Err(error)) => Some(format!("Error: {error:?}\n")),
                _ => None,
            })
        });
    drop(engine);
    let mut output = std::mem::take(&mut *sink.lock().unwrap());
    output.note = note;
    match ran {
        Ok(None) => {}
        Ok(Some(error)) => {
            output.push(true, &error);
            output.status = 1;
        }
        Err(error) => {
            output.status = 1;
            let text = error.to_string();
            if output.truncated || text.contains(OUTPUT_LIMIT) {
                output.limit = true;
            } else if let Some(reason) = limit(&text) {
                output.limit = true;
                output.push(true, &format!("{reason}\n"));
            } else {
                // A panic: Rust's exit status, at the user's own span.
                output.status = PANIC_STATUS;
                let original = positions.translate(&error).filter(|span| {
                    let start = span.start();
                    start
                        .line()
                        .zip(start.position())
                        .is_some_and(|(line, column)| {
                            rawr_snippet::in_source(&program, source, line, column)
                        })
                });
                let message = rawr::diagnostic::Report::from_evaluation_located(
                    evidence.clone(),
                    original,
                    &error,
                )
                .or_else(|_| {
                    rawr::diagnostic::Report::from_evaluation_located(evidence, None, &error)
                })
                .map_or_else(
                    |_| rawr_snippet::without_positions(&text),
                    |report| panic(&report, &program, source),
                );
                output.push(true, &format!("{}\n", message.trim_end()));
            }
        }
    }
    output.finish()
}

/// `report`, a run-time failure, as a Cargo-built program prints its panic,
/// at the snippet's own line and column.
pub(crate) fn panic(report: &rawr::diagnostic::Report, program: &str, source: &str) -> String {
    rawr_snippet::panic(&rawr::diagnostic::panic(report), program, source)
}

type Compiled = (
    rawr::AST,
    rawr::program::Positions,
    rawr::diagnostic::Source,
);

/// Why a program did not compile: its report, or the error's text when no
/// report could be made.
type Failure = (Option<rawr::diagnostic::Report>, String);

fn compile(engine: &Engine, program: &str) -> Result<Compiled, Failure> {
    let evidence = rawr::diagnostic::Source::new(SOURCE_NAME, program)
        .map_err(|_| (None, "the snippet cannot be read".to_owned()))?;
    engine
        .parse_unit(program)
        .and_then(|unit| engine.compile_unit_with_positions(unit, evidence.clone()))
        .map(|(ast, positions)| (ast, positions, evidence.clone()))
        .map_err(|error| {
            (
                rawr::diagnostic::Report::from_parse(evidence, &error).ok(),
                error.to_string(),
            )
        })
}

/// The type of a statements-only snippet's final value when
/// [`plain::program`] cannot print it, as the server finds it.
fn unprintable(report: &rawr::diagnostic::Report, source: &str) -> Option<String> {
    let lines = rawr_snippet::without_tests(source).split('\n').count();
    let after = report.labels().iter().any(|label| {
        label.kind() == rawr::diagnostic::LabelKind::Primary
            && label.span().start().line().is_some_and(|line| line > lines)
    });
    let owner = report
        .message()
        .strip_suffix(" does not provide `Debug`")?
        .strip_prefix('`')?
        .strip_suffix('`')?;
    (report.code() == rawr::diagnostic::code::RESOLVE_CAPABILITY && after).then(|| owner.to_owned())
}

/// The server's wording for an error that is a limit.
pub(crate) fn limit(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    Some(
        if lower.contains(TIME_LIMIT) || lower.contains("terminated") {
            TIME_LIMIT.to_owned()
        } else if lower.contains("stack overflow") {
            format!(
                "call depth limit exceeded: deeper than {CALL_DEPTH} calls (endless recursion?)"
            )
        } else if lower.contains("too large") {
            let what = rawr_snippet::without_positions(text.rsplit(": ").next().unwrap_or(text));
            format!(
                "size limit exceeded: {what} (strings up to {DATA_SIZE} bytes, arrays and maps up to {DATA_SIZE} items)"
            )
        } else {
            return None;
        },
    )
}

/// A base Engine with the server's limits but its operation count, and a
/// wall-clock deadline instead, whose output goes to `sink`.
fn engine(sink: &Arc<Mutex<Output>>) -> Engine {
    let mut engine = Engine::new();
    sandbox(&mut engine, sink);
    engine
}

/// Give `engine` the server's limits but its operation count, and a
/// wall-clock deadline [`TIME_MS`] from now instead, and send its output
/// to `sink`.
pub(crate) fn sandbox(engine: &mut Engine, sink: &Arc<Mutex<Output>>) {
    let deadline = crate::now() + TIME_MS;
    let depth = engine.max_call_levels().min(CALL_DEPTH);
    let (output, print, debug, progress) = (sink.clone(), sink.clone(), sink.clone(), sink.clone());
    engine
        .set_max_call_levels(depth)
        .set_max_string_size(DATA_SIZE)
        .set_max_array_size(DATA_SIZE)
        .set_max_map_size(DATA_SIZE)
        .on_progress(move |count| {
            // Reading the clock costs more than an operation does.
            if count % 1024 == 0 && crate::now() >= deadline {
                Some(TIME_LIMIT.into())
            } else if progress.lock().unwrap().truncated {
                Some(OUTPUT_LIMIT.into())
            } else {
                None
            }
        })
        .on_output(move |stream, text| {
            output
                .lock()
                .unwrap()
                .push(stream == rawr::Output::Stderr, text);
        })
        // Untyped `fn f(x) { .. }` bodies reach the base language's
        // `print` and `debug`: their text goes with the rest.
        .on_print(move |text| print.lock().unwrap().push(false, &format!("{text}\n")))
        .on_debug(move |text, _, _| debug.lock().unwrap().push(true, &format!("{text}\n")))
        .set_module_resolver(rawr::module_resolvers::DummyModuleResolver::new());
    for symbol in ["import", "eval"] {
        engine.disable_symbol(symbol);
    }
    let refuse = || Err::<(), Box<rawr::EvalAltResult>>("sleep is not available in ?play".into());
    engine
        .register_fn("sleep", move |_: rawr::INT| refuse())
        .register_fn("sleep", move |_: rawr::FLOAT| refuse());
}
