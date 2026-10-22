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
//! the body of a `fn main`, and a final expression is printed with `{:?}`,
//! as `rawr run` prints a script's final value.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use rawr::layer::{Claims, Facts};
use rawr::{CallFnOptions, Dynamic, Engine, Scope};

use crate::{Limits, PlayError, SOURCE_NAME};

/// Most output kept, in bytes; a program printing more is stopped.
pub const MAX_OUTPUT: usize = 64 * 1024;

/// What ends output cut at [`MAX_OUTPUT`].
pub const TRUNCATED: &str = "… output truncated";

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
    /// 0, or 1 when `main` returned `Err`, the program failed at run time or
    /// a limit stopped it.
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
            program,
            unit,
            ..
        } = self;
        let evidence = diagnostic(&program)
            .ok_or_else(|| PlayError::Compile("the snippet cannot be read".to_owned()))?;
        let ast = engine
            .compile_unit(unit, evidence.clone())
            .map_err(|error| match rawr::diagnostic::Report::from_parse(evidence.clone(), &error) {
                Ok(report) => PlayError::Compile(crate::report(&report, &program, source)),
                Err(_) => PlayError::Compile(error.to_string()),
            })?;
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
                        sink.lock().unwrap().push(Stream::Stdout, &format!("{value:?}\n"));
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
                        _ => rawr::diagnostic::Report::from_evaluation(Some(evidence), &error)
                            .map(|report| crate::report(&report, &program, source))
                            .unwrap_or(text),
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
        .on_print(move |text| print.lock().unwrap().push(Stream::Stdout, &format!("{text}\n")))
        .on_debug(move |text, _, _| debug.lock().unwrap().push(Stream::Stderr, &format!("{text}\n")))
        .set_module_resolver(rawr::module_resolvers::DummyModuleResolver::new());
    for symbol in ["import", "eval"] {
        engine.disable_symbol(symbol);
    }
    let refuse = || Err::<(), Box<rawr::EvalAltResult>>("sleep is not available in ?play".into());
    engine
        .register_fn("sleep", move |_: rawr::INT| refuse())
        .register_fn("sleep", move |_: rawr::FLOAT| refuse());
    engine
}

/// The program a snippet runs as. One with items runs as written. One of
/// statements alone becomes `fn main`'s body; its final expression, unless
/// it is a loop or an output macro, is printed with `{:?}`. What is added
/// shares the first line and the final expression's line, or follows the
/// snippet, so its lines keep their numbers.
#[must_use]
pub fn program(source: &str) -> String {
    let source = crate::without_tests(source);
    let scan = scan(&source);
    if scan.item {
        return source;
    }
    let tail = source[scan.tail..].trim();
    let statement = tail.is_empty()
        || ["for ", "while ", "loop ", "loop{", "let "]
            .iter()
            .any(|start| tail.starts_with(start))
        || [
            "print!", "println!", "eprint!", "eprintln!", "assert!", "assert_eq!", "assert_ne!",
            "panic!",
        ]
        .iter()
        .any(|start| tail.starts_with(start));
    if statement {
        return format!("fn main() {{ {source}\n}}\n");
    }
    let at = scan.tail + source[scan.tail..].find(tail).unwrap_or(0);
    format!(
        "fn main() {{ {}println!(\"{{:?}}\", {}\n); }}\n",
        &source[..at],
        &source[at..]
    )
}

struct Scan {
    /// Whether a top-level item starts a statement.
    item: bool,
    /// Where the final statement starts.
    tail: usize,
}

/// Find top-level items and the final statement, outside strings, chars and
/// comments: statements end at `;`, and at a `}` closing a block that the
/// next word does not continue (`else`).
fn scan(source: &str) -> Scan {
    const ITEMS: &[&str] = &[
        "fn", "pub", "struct", "enum", "impl", "trait", "use", "mod", "const", "static", "type",
        "#[",
    ];
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    let mut tail = 0;
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
                index = source[index..].find('\n').map_or(bytes.len(), |end| index + end);
                continue;
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index = source[index + 2..].find("*/").map_or(bytes.len(), |end| index + end + 4);
                continue;
            }
            b'"' => {
                index += 1;
                while index < bytes.len() && bytes[index] != b'"' {
                    index += if bytes[index] == b'\\' { 2 } else { 1 };
                }
            }
            b'\'' if bytes.get(index + 2) == Some(&b'\'')
                || (bytes.get(index + 1) == Some(&b'\\') && bytes.get(index + 3) == Some(&b'\'')) =>
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
                        tail = index + 1;
                        start = true;
                    }
                }
            }
            b';' if depth == 0 => {
                tail = index + 1;
                start = true;
            }
            _ => {}
        }
        index += 1;
    }
    Scan { item, tail }
}

#[cfg(test)]
mod tests {
    use super::program;

    #[test]
    fn plain_snippets_become_programs() {
        assert_eq!(program("1 + 2"), "fn main() { println!(\"{:?}\", 1 + 2\n); }\n");
        assert_eq!(
            program("let a = 1;\na + 1"),
            "fn main() { let a = 1;\nprintln!(\"{:?}\", a + 1\n); }\n"
        );
        assert_eq!(program("println!(\"hi\")"), "fn main() { println!(\"hi\")\n}\n");
        assert_eq!(program("let s = \"a;b\";"), "fn main() { let s = \"a;b\";\n}\n");
        let looped = "for i in 0..2 {\n    println!(\"{i}\");\n}";
        assert_eq!(program(looped), format!("fn main() {{ {looped}\n}}\n"));
        let items = "fn main() {\n    println!(\"x\");\n}\n";
        assert_eq!(program(items), items);
        let value = "if true { 1 } else { 2 }";
        assert!(program(value).contains("println!(\"{:?}\", if true"));
    }
}
