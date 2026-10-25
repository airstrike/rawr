//! Plain programs: a snippet that does not use iced runs on the base
//! language, as `rawr run` runs a file, and its output comes back as text.
//!
//! The snippet is parsed once. That parse gives the [`Facts`] the decision
//! reads (rawr-iced's own [`Claims`]), and a plain program is compiled from
//! that same parse. A snippet rawr-iced claims is handed to the renderer,
//! which wraps it as a view and checks it itself, as `rawr` hands a program
//! to the `rawr-iced` binary.
//!
//! A snippet with items (`fn main`, a struct, ..) runs as written: its top
//! level, then `fn main` if there is one. A snippet of statements alone is
//! a block in a `fn main`, and the block's value is printed with `{:?}`
//! unless it is `()`.
//!
//! An `async fn main`, as `#[tokio::main]` marks it, runs on rawr-iced's
//! tokio (see [`run_async`]): its futures, timers, channels and
//! `spawn_blocking` are that tokio's, on the simulated clock iced apps
//! record on. Its output is text, as any plain program's.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use rawr::layer::{Claims, Facts};
use rawr::{CallFnOptions, Dynamic, Engine, Scope};

use crate::{Limits, PlayError, SOURCE_NAME};

/// Most output kept, in bytes; a program printing more is stopped.
pub const MAX_OUTPUT: usize = 64 * 1024;

/// What ends output cut at [`MAX_OUTPUT`].
pub const TRUNCATED: &str = "… output truncated";

/// The status of a program that panicked, as Rust's.
pub const PANIC_STATUS: i32 = 101;

const TIME_LIMIT: &str = "time limit exceeded";
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
}

/// The note on a plain program run with messages.
pub const MESSAGES_IGNORED: &str = "messages are ignored for programs without iced";

impl Output {
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
    /// "truncated":false,"limit":false}`: what `rawr-play` writes for a
    /// plain program where it writes an image for an iced one. It starts
    /// with `{`, which no image does.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::json!({
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
        })
        .to_string()
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
        })
    }
}

/// A snippet parsed once as a plain program, with its facts.
pub struct Parsed {
    engine: Engine,
    sink: Arc<Mutex<Output>>,
    program: String,
    unit: rawr::parse::Unit,
    facts: Facts,
}

impl Parsed {
    /// Wrap and parse `source` on a sandboxed base Engine.
    pub fn new(source: &str, limits: &Limits, deadline: Instant) -> Result<Self, PlayError> {
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
        let facts = Facts::of_unit(&unit);
        Ok(Self {
            engine,
            sink,
            program,
            unit,
            facts,
        })
    }

    /// Whether `claims` (rawr-iced's) takes this snippet: it names one of
    /// their crates or uses a name of their prelude.
    #[must_use]
    pub fn claimed(&self, claims: &Claims) -> bool {
        claims.claim_snippet(&self.facts)
    }

    /// Compile the parse and run it once; `source` is the snippet as the
    /// user wrote it, for diagnostics.
    pub fn run(self, source: &str, limits: &Limits) -> Result<Output, PlayError> {
        let Self {
            engine,
            sink,
            mut program,
            unit,
            ..
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
                    let message = match crate::classify(&text, limits) {
                        PlayError::Limit(reason) => {
                            output.limit = true;
                            // The renderer's wording names a render.
                            reason.replace(crate::TIME_LIMIT, TIME_LIMIT)
                        }
                        _ => {
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
                                        crate::in_source(&program, source, line, column)
                                    })
                            });
                            rawr::diagnostic::Report::from_evaluation_located(
                                evidence, original, &error,
                            )
                            .map_or_else(
                                |_| crate::without_positions(&text),
                                |report| crate::report(&report, &program, source),
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
fn engine(limits: &Limits, deadline: Instant, sink: &Arc<Mutex<Output>>) -> Engine {
    let mut engine = Engine::new();
    sandbox(&mut engine, limits, deadline, sink);
    engine
}

/// The untrusted-snippet limits on `engine`, its output going to `sink`.
fn sandbox(engine: &mut Engine, limits: &Limits, deadline: Instant, sink: &Arc<Mutex<Output>>) {
    let depth = engine.max_call_levels().min(limits.call_depth);
    let (output, print, debug, progress) = (sink.clone(), sink.clone(), sink.clone(), sink.clone());
    engine
        .set_max_operations(limits.operations)
        .set_max_call_levels(depth)
        .set_max_string_size(limits.string_size)
        .set_max_array_size(limits.array_size)
        .set_max_map_size(limits.map_size)
        .on_progress(move |_| {
            if Instant::now() >= deadline {
                Some(crate::TIME_LIMIT.into())
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
        })
        // Untyped `fn f(x) { .. }` bodies reach the base language's
        // `print` and `debug`: their text goes with the rest.
        .on_print(move |text| {
            print
                .lock()
                .unwrap()
                .push(Stream::Stdout, &format!("{text}\n"))
        })
        .on_debug(move |text, _, _| {
            debug
                .lock()
                .unwrap()
                .push(Stream::Stderr, &format!("{text}\n"))
        })
        .set_module_resolver(rawr::module_resolvers::DummyModuleResolver::new());
    for symbol in ["import", "eval"] {
        engine.disable_symbol(symbol);
    }
    let refuse = || Err::<(), Box<rawr::EvalAltResult>>("sleep is not available in ?play".into());
    engine
        .register_fn("sleep", move |_: rawr::INT| refuse())
        .register_fn("sleep", move |_: rawr::FLOAT| refuse());
}

/// Where a snippet's `async fn main` is, as rawr-play rewrites it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AsyncMain {
    /// The byte offset of the name `main`.
    pub(crate) name: usize,
    /// The span of a `#[tokio::main]` (or `#[tokio::main(..)]`) on it.
    pub(crate) attribute: Option<std::ops::Range<usize>>,
    /// Whether it returns a value (a `Result`), which `?` and an `Err`
    /// end the program with.
    pub(crate) returns: bool,
}

/// The `async fn main` at the top level of `source`, with tests blanked.
#[must_use]
pub fn async_main(source: &str) -> Option<AsyncMain> {
    let mut offset = 0;
    let mut attribute = None;
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if indent == 0 && trimmed.starts_with("#[tokio::main") {
            let end = trimmed
                .find(']')
                .map_or(trimmed.trim_end().len(), |end| end + 1);
            attribute = Some(offset..offset + end);
        }
        let rest = trimmed.strip_prefix("pub ").unwrap_or(trimmed);
        if indent == 0
            && let Some(rest) = rest.strip_prefix("async fn main")
            && rest.trim_start().starts_with('(')
        {
            let name = offset + (line.len() - rest.len()) - "main".len();
            let signature = &source[name..];
            let signature = &signature[..signature.find('{').unwrap_or(signature.len())];
            return Some(AsyncMain {
                name,
                attribute,
                returns: signature.contains("->"),
            });
        }
        if !trimmed.is_empty() && !trimmed.starts_with("#[") && !trimmed.starts_with("//") {
            attribute = None;
        }
        offset += line.len();
    }
    None
}

/// The program an `async fn main` runs as: the snippet with its `main`
/// renamed `rawr_play_main` and its `#[tokio::main]` blanked, so every
/// line keeps its number, and after it an app whose boot Task awaits it.
fn async_program(source: &str, main: &AsyncMain) -> String {
    let mut program = source.to_owned();
    program.insert_str(main.name, "rawr_play_");
    if let Some(attribute) = &main.attribute {
        program.replace_range(attribute.clone(), &" ".repeat(attribute.len()));
    }
    let run = if main.returns {
        "match rawr_play_main().await { Ok(_) => 0, Err(error) => { eprintln!(\"Error: {error:?}\"); 1 } }"
    } else {
        "rawr_play_main().await; 0"
    };
    program.push_str(&format!(
        "\n#[derive(Default)]\nstruct RawrPlayAsync {{ status: i64 }}\n\
         #[derive(Debug, Clone)]\nenum RawrPlayDone {{ Done(i64) }}\n\
         impl RawrPlayAsync {{\n\
             fn start() -> (Self, iced::Task<RawrPlayDone>) {{ (Self {{ status: -1 }}, iced::Task::perform(rawr_play_run(), RawrPlayDone::Done)) }}\n\
             fn update(&mut self, message: RawrPlayDone) {{ match message {{ RawrPlayDone::Done(status) => self.status = status }} }}\n\
             fn view(&self) -> impl iced::Widget<RawrPlayDone> {{ iced::widget::text(\"\") }}\n\
             fn title(&self) -> String {{ self.status.to_string() }}\n\
         }}\n\
         async fn rawr_play_run() -> i64 {{ {run} }}\n\
         pub fn main() -> iced::Result {{ iced::application(RawrPlayAsync::start, RawrPlayAsync::update, RawrPlayAsync::view).title(RawrPlayAsync::title).run() }}\n"
    ));
    program
}

/// Run a snippet whose `main` is `async`, as `#[tokio::main]` runs it, and
/// return its output as text. It runs on rawr-iced's tokio, whose timers
/// follow the simulated clock iced apps record on: the clock moves only
/// while a task waits on it, so a sleep costs no wall time. No window is
/// made, and nothing is drawn.
pub fn run_async(
    source: &str,
    main: &AsyncMain,
    limits: &Limits,
    deadline: Instant,
) -> Result<Output, PlayError> {
    let source = crate::without_tests(source);
    let program = async_program(&source, main);
    let sink = Arc::new(Mutex::new(Output::default()));
    let configured = {
        let (limits, sink) = (limits.clone(), sink.clone());
        move |engine: &mut Engine| sandbox(engine, &limits, deadline, &sink)
    };
    let compiler = rawr_iced::application::Compiler::configured(configured)
        .map_err(|error| PlayError::Render(error.to_string()))?;
    // The program boots on the simulated clock, so the times it reads are
    // on it.
    let _clock = rawr_iced::snapshot::clock::freeze();
    let mut adapter = compiler
        .check_named_source(SOURCE_NAME, &program)
        .and_then(rawr_iced::application::Checked::prepare)
        .map_err(|error| {
            PlayError::Compile(crate::report(
                &error.report(diagnostic(&program)),
                &program,
                &source,
            ))
        })?
        .into_adapter();
    let finished =
        rawr_iced::snapshot::run_tasks(&mut adapter, &rawr_iced::snapshot::Deadline::new(deadline));
    let status = adapter
        .refresh_title()
        .ok()
        .and_then(|status| status.parse::<i32>().ok());
    let failures = adapter
        .diagnostics()
        .iter()
        .map(|entry| entry.report().clone())
        .collect::<Vec<_>>();
    drop(adapter);
    drop(compiler);
    let mut output = sink.lock().unwrap().clone();
    if let Some(report) = failures.first() {
        output.status = 1;
        let text = rawr::diagnostic::render(report);
        if output.truncated || text.contains(OUTPUT_LIMIT) {
            output.limit = true;
        } else {
            let message = match crate::classify(&text, limits) {
                PlayError::Limit(reason) => {
                    output.limit = true;
                    reason.replace(crate::TIME_LIMIT, TIME_LIMIT)
                }
                _ => {
                    output.status = PANIC_STATUS;
                    crate::report(report, &program, &source)
                }
            };
            output.push(Stream::Stderr, &format!("{}\n", message.trim_end()));
        }
    } else if !finished {
        output.status = 1;
        output.limit = true;
        output.push(Stream::Stderr, &format!("{TIME_LIMIT}\n"));
    } else if status == Some(1) {
        output.status = 1;
    }
    if output.truncated {
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

/// The program a snippet runs as. One with items runs as written. One of
/// statements alone becomes a block in `fn main`, as Rust reads a block: a
/// final expression without `;` is its value, which is printed with `{:?}`
/// unless it is `()`. The block opens on the first line and closes after
/// the snippet, so its lines and columns keep their numbers.
#[must_use]
pub fn program(source: &str) -> String {
    wrapped(source, true)
}

/// [`program`], with a final value that is evaluated and never printed:
/// the program for a value whose type has no `Debug`, or never ends.
fn quiet_program(source: &str) -> String {
    wrapped(source, false)
}

fn wrapped(source: &str, show: bool) -> String {
    let source = crate::without_tests(source);
    if has_item(&source) {
        return source;
    }
    if show {
        format!(
            "fn main() {{ let __v = {{ {source}\n}};\n\
             let __d = format!(\"{{:?}}\", __v); if __d != \"()\" {{ println!(\"{{}}\", __d); }} }}\n"
        )
    } else {
        // The final value is a statement: an iterator there runs no stage.
        format!("fn main() {{ {source}\n; }}\n")
    }
}

/// The type of a statements-only snippet's final value when [`program`]
/// cannot print it: the report says it has no `Debug`, at a line after the
/// snippet's own.
fn unprintable(report: &rawr::diagnostic::Report, source: &str) -> Option<String> {
    let lines = crate::without_tests(source).split('\n').count();
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

/// Whether a top-level item starts a statement, outside strings, chars and
/// comments: statements end at `;`, and at a `}` closing a block that the
/// next word does not continue (`else`).
fn has_item(source: &str) -> bool {
    const ITEMS: &[&str] = &[
        "fn", "pub", "struct", "enum", "impl", "trait", "use", "mod", "const", "static", "type",
        "#[",
    ];
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    let mut item = false;
    let mut start = true;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if start && !byte.is_ascii_whitespace() {
            start = false;
            let rest = &source[index..];
            item |= ITEMS.iter().any(|word| {
                rest.starts_with(word)
                    && (*word == "#["
                        || !rest[word.len()..]
                            .starts_with(|c: char| c.is_alphanumeric() || c == '_'))
            });
        }
        match byte {
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index = source[index..]
                    .find('\n')
                    .map_or(bytes.len(), |end| index + end);
                continue;
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index = source[index + 2..]
                    .find("*/")
                    .map_or(bytes.len(), |end| index + end + 4);
                continue;
            }
            b'"' => {
                index += 1;
                while index < bytes.len() && bytes[index] != b'"' {
                    index += if bytes[index] == b'\\' { 2 } else { 1 };
                }
            }
            b'\''
                if bytes.get(index + 2) == Some(&b'\'')
                    || (bytes.get(index + 1) == Some(&b'\\')
                        && bytes.get(index + 3) == Some(&b'\'')) =>
            {
                index += if bytes[index + 1] == b'\\' { 3 } else { 2 };
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' => depth = depth.saturating_sub(1),
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let next = source[index + 1..].trim_start();
                    let word: String = next
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if !word.is_empty() && word != "else" && word != "as" {
                        start = true;
                    }
                }
            }
            b';' if depth == 0 => {
                start = true;
            }
            _ => {}
        }
        index += 1;
    }
    item
}

#[cfg(test)]
mod tests {
    use super::program;

    #[test]
    fn plain_snippets_become_programs() {
        let print =
            "let __d = format!(\"{:?}\", __v); if __d != \"()\" { println!(\"{}\", __d); } }\n";
        assert_eq!(
            program("1 + 2"),
            format!("fn main() {{ let __v = {{ 1 + 2\n}};\n{print}")
        );
        assert_eq!(
            program("let a = 1;\na + 1"),
            format!("fn main() {{ let __v = {{ let a = 1;\na + 1\n}};\n{print}")
        );
        let looped = "for i in 0..2 {\n    println!(\"{i}\");\n}";
        assert_eq!(
            program(looped),
            format!("fn main() {{ let __v = {{ {looped}\n}};\n{print}")
        );
        let items = "fn main() {\n    println!(\"x\");\n}\n";
        assert_eq!(program(items), items);
        let item_later = "let s = \"a;b\";\nfn f() {}";
        assert_eq!(program(item_later), item_later);
        assert_eq!(super::quiet_program("1 + 2"), "fn main() { 1 + 2\n; }\n");
    }
}
