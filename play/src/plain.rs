//! Plain programs: a snippet runs on the base language, as `rawr run` runs
//! a file, and its output comes back as text. The snippet is parsed once,
//! and the program is compiled from that parse.
//!
//! A snippet with items (`fn main`, a struct, ..) runs as written: its top
//! level, then `fn main` if there is one. A snippet of statements alone is
//! a block in a `fn main`, and the block's value is printed with `{:?}`
//! unless it is `()`.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use rawr::play::deadline;
use rawr::{CallFnOptions, Dynamic, Engine, Scope};

use crate::{Limits, PlayError, SOURCE_NAME};
pub use rawr_snippet::plain::program;
use rawr_snippet::plain::quiet_program;

/// Most output kept, in bytes; a program printing more is stopped.
pub const MAX_OUTPUT: usize = 64 * 1024;

/// What ends output cut at [`MAX_OUTPUT`].
pub const TRUNCATED: &str = "… output truncated";

/// The status of a program that panicked, as Rust's.
pub const PANIC_STATUS: i32 = 101;

/// What a run stopped at its deadline reports.
pub const TIME_LIMIT: &str = "time limit exceeded";
const OUTPUT_LIMIT: &str = "output limit exceeded";

/// Where a piece of output went.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stream {
    /// `print!`, `println!` and a final value.
    Stdout,
    /// `eprint!`, `eprintln!`, a `main` that returned `Err`, a runtime error.
    Stderr,
}

impl Stream {
    /// Its name in [`Output::to_json`].
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }
}

/// A plain program's output, in the order it was written.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Output {
    /// Runs of text, each from one stream; neighbours differ in stream.
    pub chunks: Vec<(Stream, String)>,
    /// 0, 1 when `main` returned `Err` or a limit stopped it, or 101 when
    /// the program panicked.
    pub status: i32,
    /// Whether the output passed [`MAX_OUTPUT`] and was cut.
    pub truncated: bool,
    /// Whether a limit (time, operations, depth, size, output) stopped it.
    pub limit: bool,
    /// A remark on the run itself, such as options it ignored.
    pub note: Option<String>,
    /// What an iced program drew (#434): the PNG of its settled frame, or a
    /// GIF when it animated (#527).
    pub frame: Option<Vec<u8>>,
}

/// The kind of image `bytes` are, as a file extension: `png` or `gif`.
#[must_use]
pub fn image_kind(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else {
        None
    }
}

impl Output {
    /// Add `text` written to `stream`, cut at [`MAX_OUTPUT`].
    pub fn write(&mut self, stream: Stream, text: &str) {
        self.push(stream, text);
    }

    fn push(&mut self, stream: Stream, text: &str) {
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
            Some((last, chunk)) if *last == stream => chunk.push_str(text),
            _ => self.chunks.push((stream, text.to_owned())),
        }
    }

    /// The text of every chunk, in order.
    #[must_use]
    pub fn text(&self) -> String {
        self.chunks.iter().map(|(_, text)| text.as_str()).collect()
    }

    /// `{"kind":"text","status":0,"output":[{"stream":"stdout","text":".."}],
    /// "truncated":false,"limit":false}`: what `rawr-play` writes. A
    /// program that drew adds `"frame":{"type":"image/png","data":".."}`,
    /// the image in base64.
    #[must_use]
    pub fn to_json(&self) -> String {
        use base64::Engine as _;
        let mut json = serde_json::json!({
            "kind": "text",
            "status": self.status,
            "output": self
                .chunks
                .iter()
                .map(|(stream, text)| serde_json::json!({ "stream": stream.name(), "text": text }))
                .collect::<Vec<_>>(),
            "truncated": self.truncated,
            "limit": self.limit,
            "note": self.note,
        });
        if let Some(bytes) = &self.frame {
            json["frame"] = serde_json::json!({
                "type": format!("image/{}", image_kind(bytes).unwrap_or("png")),
                "data": base64::engine::general_purpose::STANDARD.encode(bytes),
            });
        }
        json.to_string()
    }

    /// Read [`Output::to_json`] back.
    pub fn from_json(bytes: &[u8]) -> Result<Self, String> {
        let bad = || "a text result that cannot be read".to_owned();
        let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| bad())?;
        if value["kind"] != "text" {
            return Err(bad());
        }
        let chunks = value["output"]
            .as_array()
            .ok_or_else(bad)?
            .iter()
            .map(|chunk| {
                let stream = match chunk["stream"].as_str() {
                    Some("stdout") => Stream::Stdout,
                    Some("stderr") => Stream::Stderr,
                    _ => return Err(bad()),
                };
                Ok((stream, chunk["text"].as_str().ok_or_else(bad)?.to_owned()))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            chunks,
            status: value["status"]
                .as_i64()
                .and_then(|status| i32::try_from(status).ok())
                .ok_or_else(bad)?,
            truncated: value["truncated"].as_bool().unwrap_or(false),
            limit: value["limit"].as_bool().unwrap_or(false),
            note: value["note"].as_str().map(str::to_owned),
            frame: match value["frame"]["data"].as_str() {
                Some(data) => {
                    use base64::Engine as _;
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(data)
                        .map_err(|_| bad())?;
                    Some(bytes).filter(|bytes| image_kind(bytes).is_some())
                }
                None => None,
            },
        })
    }
}

/// A snippet parsed once as a plain program.
pub struct Parsed {
    engine: Engine,
    sink: Arc<Mutex<Output>>,
    program: String,
    unit: rawr::parse::Unit,
}

impl Parsed {
    /// Wrap and parse `source` on a sandboxed base Engine.
    pub fn new(
        source: &str,
        limits: &Limits,
        deadline: deadline::Watch,
    ) -> Result<Self, PlayError> {
        let program = program(source);
        let sink = Arc::new(Mutex::new(Output::default()));
        let engine = engine(limits, deadline, &sink);
        let unit = engine.parse_unit(&program).map_err(|error| {
            let report = diagnostic(&program)
                .ok_or(rawr::diagnostic::Error::EmptySourceIdentity)
                .and_then(|source| rawr::diagnostic::Report::from_parse(source, &error));
            match report {
                Ok(report) => PlayError::Compile(crate::report(&report, &program, source)),
                Err(_) => PlayError::Compile(error.to_string()),
            }
        })?;
        Ok(Self {
            engine,
            sink,
            program,
            unit,
        })
    }

    /// Compile the parse and run it once; `source` is the snippet as the
    /// user wrote it, for diagnostics.
    pub fn run(self, source: &str, limits: &Limits) -> Result<Output, PlayError> {
        let Self {
            engine,
            sink,
            mut program,
            unit,
        } = self;
        let unreadable = || PlayError::Compile("the snippet cannot be read".to_owned());
        let mut evidence = diagnostic(&program).ok_or_else(unreadable)?;
        let mut note = None;
        let (ast, positions) = match engine.compile_unit_with_positions(unit, evidence.clone()) {
            Ok(compiled) => compiled,
            Err(error) => {
                let report = rawr::diagnostic::Report::from_parse(evidence.clone(), &error);
                let owner = report
                    .as_ref()
                    .ok()
                    .and_then(|report| unprintable(report, source));
                let Some(owner) = owner else {
                    return Err(match report {
                        Ok(report) => PlayError::Compile(crate::report(&report, &program, source)),
                        Err(_) => PlayError::Compile(error.to_string()),
                    });
                };
                // The final value has no `Debug`: run it without printing it.
                note = unused_note(&owner);
                program = quiet_program(source);
                evidence = diagnostic(&program).ok_or_else(unreadable)?;
                let compiled = engine
                    .parse_unit(&program)
                    .and_then(|unit| engine.compile_unit_with_positions(unit, evidence.clone()));
                compiled.map_err(|error| {
                    match rawr::diagnostic::Report::from_parse(evidence.clone(), &error) {
                        Ok(report) => PlayError::Compile(crate::report(&report, &program, source)),
                        Err(_) => PlayError::Compile(error.to_string()),
                    }
                })?
            }
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
                        sink.lock()
                            .unwrap()
                            .push(Stream::Stdout, &format!("{value:?}\n"));
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
        let mut output = Arc::try_unwrap(sink)
            .map(|sink| sink.into_inner().unwrap())
            .unwrap_or_else(|sink| sink.lock().unwrap().clone());
        output.note = note.map(str::to_owned);
        match ran {
            Ok(None) => {}
            Ok(Some(error)) => {
                output.push(Stream::Stderr, &error);
                output.status = 1;
            }
            Err(error) => {
                output.status = 1;
                let text = error.to_string();
                if output.truncated || text.contains(OUTPUT_LIMIT) {
                    output.limit = true;
                } else {
                    let message = match crate::limit(&text, limits) {
                        Some(reason) => {
                            output.limit = true;
                            reason
                        }
                        None => {
                            // A panic: Rust's exit status.
                            output.status = PANIC_STATUS;
                            // The user's span of the error. A position in
                            // the wrapping is dropped.
                            let original = positions.translate(&error).filter(|span| {
                                let start = span.start();
                                start
                                    .line()
                                    .zip(start.position())
                                    .is_some_and(|(line, column)| {
                                        rawr_snippet::in_source(&program, source, line, column)
                                    })
                            });
                            rawr::diagnostic::Report::from_evaluation_located(
                                evidence.clone(),
                                original,
                                &error,
                            )
                            .or_else(|_| {
                                rawr::diagnostic::Report::from_evaluation_located(
                                    evidence, None, &error,
                                )
                            })
                            .map_or_else(
                                |_| rawr_snippet::without_positions(&text),
                                |report| crate::panic(&report, &program, source),
                            )
                        }
                    };
                    output.push(Stream::Stderr, &format!("{}\n", message.trim_end()));
                }
            }
        }
        if output.truncated {
            // Past the cap, on a line of its own.
            let marker = format!("\n{TRUNCATED}\n");
            match output.chunks.last_mut() {
                Some((Stream::Stderr, chunk)) => chunk.push_str(&marker),
                _ => output.chunks.push((Stream::Stderr, marker)),
            }
            output.status = 1;
            output.limit = true;
        }
        Ok(output)
    }
}

fn diagnostic(program: &str) -> Option<rawr::diagnostic::Source> {
    rawr::diagnostic::Source::new(SOURCE_NAME, program).ok()
}

/// A base Engine with the untrusted-snippet limits whose output goes to
/// `sink`; it stops a program at the deadline or once `sink` is full.
fn engine(limits: &Limits, deadline: deadline::Watch, sink: &Arc<Mutex<Output>>) -> Engine {
    let mut engine = rawr::future::facade::engine();
    sandbox(&mut engine, limits, deadline, sink);
    engine
}

/// The untrusted-snippet limits on `engine`, its output going to `sink`.
fn sandbox(
    engine: &mut Engine,
    limits: &Limits,
    deadline: deadline::Watch,
    sink: &Arc<Mutex<Output>>,
) {
    let deadline = Mutex::new(deadline);
    let depth = engine.max_call_levels().min(limits.call_depth);
    let (output, progress) = (sink.clone(), sink.clone());
    engine
        .set_max_operations(limits.operations)
        .set_max_call_levels(depth)
        .set_max_string_size(limits.string_size)
        .set_max_array_size(limits.array_size)
        .set_max_map_size(limits.map_size)
        .on_progress(move |_| {
            if deadline.lock().unwrap().passed(Instant::now()) {
                Some(TIME_LIMIT.into())
            } else if progress.lock().unwrap().truncated {
                Some(OUTPUT_LIMIT.into())
            } else {
                None
            }
        })
        .on_output(move |stream, text| {
            let stream = if stream == rawr::Output::Stderr {
                Stream::Stderr
            } else {
                Stream::Stdout
            };
            output.lock().unwrap().push(stream, text);
        });
    let refuse = || Err::<(), Box<rawr::EvalAltResult>>("sleep is not available in ?play".into());
    engine
        .register_fn("sleep", move |_: rawr::INT| refuse())
        .register_fn("sleep", move |_: rawr::FLOAT| refuse());
}

/// The type of a statements-only snippet's final value when [`program`]
/// cannot print it: the report says it has no `Debug`, at a line after the
/// snippet's own.
fn unprintable(report: &rawr::diagnostic::Report, source: &str) -> Option<String> {
    let lines = rawr_snippet::without_tests(source).split('\n').count();
    let after = report.labels().iter().any(|label| {
        label.kind() == rawr::diagnostic::LabelKind::Primary
            && label.span().start().line().is_some_and(|line| line > lines)
    });
    let owner = report
        .message()
        .strip_suffix(" doesn't implement `Debug`")?
        .strip_prefix('`')?
        .strip_suffix('`')?;
    (report.code() == rawr::diagnostic::code::RESOLVE_CAPABILITY && after).then(|| owner.to_owned())
}

/// The note on a final value of type `owner` that is not printed, where
/// rustc would warn.
fn unused_note(owner: &str) -> Option<&'static str> {
    owner
        .starts_with("impl Iterator")
        .then_some(UNUSED_ITERATOR)
}

/// The note on a final iterator, as rustc's warning on an unused one.
pub const UNUSED_ITERATOR: &str =
    "This iterator is never used. Iterators are lazy and do nothing unless consumed.";
