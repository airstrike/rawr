//! A hover request and its answer where the program is paused.

use serde_json::{json, Value};

use super::expression::{Node, Tokens};
use super::read::{Frame, Need, Read};
use super::within;
use crate::inspect::evaluate::{self, Program};
use crate::inspect::{depth_of, read, Placed, Request, Shared};
use crate::{EvalContext, Position};

/// What a hover asks to read.
#[derive(Debug, PartialEq, Eq)]
pub(in crate::inspect) enum Ask {
    /// The smallest expression a hover may read at character `column` of
    /// line `line` (both from 1) of `file`, as the program compiled it.
    /// `text`, the line as the shell shows it, must be the line compiled.
    At {
        file: String,
        line: usize,
        column: usize,
        text: Option<String>,
    },
    /// One expression, given whole.
    Code(String),
}

impl Ask {
    /// The hover `request` asks for, when it is one: `{"op": "hover",
    /// "file", "line", "column", "text"}` or `{"op": "hover", "code"}`.
    pub(in crate::inspect) fn of(request: &Value) -> Option<Self> {
        if request["op"] != "hover" {
            return None;
        }
        let number = |key: &str| {
            request[key]
                .as_u64()
                .and_then(|number| usize::try_from(number).ok())
        };
        if let Some(code) = request["code"].as_str() {
            return Some(Self::Code(code.to_owned()));
        }
        Some(Self::At {
            file: request["file"].as_str().unwrap_or_default().to_owned(),
            line: number("line").unwrap_or(1),
            column: number("column").unwrap_or(1),
            text: request["text"].as_str().map(str::to_owned),
        })
    }
}

/// Where an expression is written: its line, and the characters it starts
/// at and ends before on it, all from 1.
fn range(text: &str, span: &std::ops::Range<usize>) -> Value {
    let column = |at: usize| {
        let start = text[..at].rfind('\n').map_or(0, |newline| newline + 1);
        text[start..at].chars().count() + 1
    };
    json!({
        "line": text[..span.start].matches('\n').count() + 1,
        "start": column(span.start),
        "end": column(span.end),
    })
}

impl Shared {
    /// The answer to `ask`, `request`'s hover, where `context` is paused at
    /// `position` of the generation `placed` places: the expression read,
    /// where it is written, and its `value` (with its parts when the
    /// request asks), why a hover `refused` it, or why nothing is `unread`
    /// there.
    pub(in crate::inspect) fn hovered(
        &self,
        context: &mut EvalContext,
        request: &Request,
        ask: &Ask,
        position: Position,
        placed: Option<&Placed>,
    ) -> Value {
        let program = placed.and_then(|placed| placed.program.as_ref());
        let unread = |why: &str| json!({ "unread": why });
        let (source, lexed, function, at) = match ask {
            Ask::Code(code) => {
                let lexed = crate::resolve::lower_nominal::lex_rust(context.engine(), code);
                (code.as_str(), lexed, None, None)
            }
            Ask::At {
                file,
                line,
                column,
                text,
            } => {
                let Some(source) = program.and_then(|program| program.text(file)) else {
                    return unread("the run kept none of this file's text, which a hover reads");
                };
                let compiled = source
                    .split('\n')
                    .nth(line.saturating_sub(1))
                    .unwrap_or_default();
                if text.as_deref().is_some_and(|shown| {
                    shown.trim_end_matches('\r') != compiled.trim_end_matches('\r')
                }) {
                    return unread(
                        "the line changed since the program compiled it: save it to read it",
                    );
                }
                let lexed = crate::resolve::lower_nominal::lex_rust(context.engine(), source);
                let function = within::function(&lexed, *line, *column);
                let at = crate::diagnostic::offset(source, *line, *column);
                (source, lexed, function, Some((*line, at)))
            }
        };
        let tokens = Tokens::on(source, &lexed, at.map(|(line, _)| line));
        let nodes;
        let whole;
        let node = match at {
            Some((_, at)) => {
                nodes = tokens.nodes();
                Node::around(&nodes, at)
            }
            None => {
                whole = tokens.whole();
                whole.as_ref()
            }
        };
        let Some(node) = node else {
            return match ask {
                Ask::Code(_) => json!({ "refused": "a hover reads one expression" }),
                Ask::At { .. } => unread("nothing to read here"),
            };
        };
        let expression = &source[node.span.clone()];
        let range = range(source, &node.span);
        let Some(frame) = self.hovered_frame(context, request.frame, function.as_deref(), position)
        else {
            let function = function.unwrap_or_default();
            return json!({
                "expression": expression,
                "range": range,
                "unread": format!("`{function}` isn't on the stack: its values show while it runs"),
            });
        };
        let read = self.hover_read(context, frame, position, placed, node, source);
        let mut answer = json!({ "expression": expression, "range": range, "frame": frame });
        match read {
            Read::Value(value) => {
                answer["value"] = json!(value);
                request.shape.add(answer)
            }
            Read::Refused(why) => {
                answer["refused"] = json!(why);
                answer
            }
            Read::Unread(why) => {
                answer["unread"] = json!(why);
                answer
            }
        }
    }

    /// The frame a hover in `function` reads: `asked` when it runs that
    /// function, else the innermost frame that does. `asked` when the
    /// hover's place is in no function.
    fn hovered_frame(
        &self,
        context: &EvalContext,
        asked: usize,
        function: Option<&str>,
        position: Position,
    ) -> Option<usize> {
        let Some(function) = function else {
            return Some(asked);
        };
        std::iter::once(asked)
            .chain(0..depth_of(context))
            .find(|&frame| {
                self.frame_name(context, frame, position)
                    .is_some_and(|name| name.rsplit("::").next() == Some(function))
            })
    }

    /// What `node`, written in `source`, reads as in frame `frame` where
    /// `context` is paused at `position`.
    fn hover_read(
        &self,
        context: &mut EvalContext,
        frame: usize,
        position: Position,
        placed: Option<&Placed>,
        node: &Node,
        source: &str,
    ) -> Read {
        let program = placed.and_then(|placed| placed.program.as_ref());
        let symbol = self.frame_symbol(context, frame, position);
        let stops = self.state().stops.clone();
        let declaring = symbol.map(|symbol| match &stops {
            Some(stops) => crate::ImmutableString::from(stops.declaring(&symbol)),
            None => symbol,
        });
        let line = crate::inspect::frame_line(context, frame, position, placed)
            .map_or(0, |(_, line)| line);
        let paused = Paused {
            program,
            frame,
            position,
            declaring,
            line,
        };
        paused.read(context, node, source)
    }
}

/// A frame of a paused program, as a hover reads it.
struct Paused<'p> {
    /// The program code evaluated there compiles into, when the run kept
    /// its files.
    program: Option<&'p Program>,
    /// The frame, 0 for the function the program is paused in.
    frame: usize,
    /// Where the program is paused.
    position: Position,
    /// The function the frame's bindings are declared in, for their
    /// types, and the line the frame is at (0 when not known).
    declaring: Option<crate::ImmutableString>,
    line: usize,
}

impl Paused<'_> {
    /// What `node`, written in `source`, reads as in this frame where
    /// `context` is paused.
    fn read(&self, context: &mut EvalContext, node: &Node, source: &str) -> Read {
        let written = written(context);
        let mut ask = |need: Need<'_>| match need {
            Need::Binding(name) => {
                match read(
                    context,
                    self.declaring.as_ref(),
                    self.frame,
                    self.line,
                    Some(name),
                )? {
                    Ok(mut found) => found.pop().map(|binding| binding.value),
                    Err(why) => Some(Err(why)),
                }
            }
            Need::Evaluate(code) => {
                let answer =
                    evaluate::answer(context, self.program?, self.frame, self.position, code);
                match (answer["value"].as_str(), answer["error"].as_str()) {
                    (Some(value), _) => Some(Ok(value.to_owned())),
                    (None, error) => Some(Err(error.unwrap_or("no value").to_owned())),
                }
            }
        };
        Frame::new(&mut ask, written).read(node, source)
    }
}

/// Each type the program wrote a `Deref` or an `Index` impl for, by its
/// name, with the impl's method: a hover reading through one would run it.
fn written(context: &EvalContext) -> Vec<(String, &'static str)> {
    context
        .global_runtime_state()
        .lib
        .iter()
        .flat_map(|lib| lib.iter_script_fn())
        .filter_map(|(.., function)| owned(&function.name))
        .collect()
}

/// The type and the method of `symbol`, a function the program wrote,
/// when it is a `Deref`, `DerefMut`, `Index` or `IndexMut` impl's method:
/// `main::Grid::Index<usize>::index` is `Grid`'s `index`.
fn owned(symbol: &str) -> Option<(String, &'static str)> {
    const METHODS: [&str; 4] = ["deref", "deref_mut", "index", "index_mut"];
    const TRAITS: [&str; 4] = ["Deref", "DerefMut", "Index", "IndexMut"];
    let shown = crate::resolve::lower_nominal::function_display_name(symbol);
    // The path's segments, past the `::` inside generic arguments.
    let mut segments = Vec::new();
    let (mut depth, mut start) = (0usize, 0);
    for (at, character) in shown.char_indices() {
        match character {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            ':' if depth == 0 && shown[at..].starts_with("::") && at >= start => {
                segments.push(&shown[start..at]);
                start = at + 2;
            }
            _ => {}
        }
    }
    segments.push(&shown[start..]);
    let (method, rest) = segments.split_last()?;
    let method = METHODS.into_iter().find(|known| known == method)?;
    let (implemented, rest) = rest.split_last()?;
    let implemented = implemented.split('<').next()?;
    if !TRAITS.contains(&implemented) {
        return None;
    }
    let owner = rest.last()?.split('<').next()?;
    Some((owner.to_owned(), method))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    use super::{Paused, Read};
    use crate::inspect::evaluate::Program;
    use crate::inspect::hover::expression::Tokens;
    use crate::inspect::stops::Stops;
    use crate::package::snapshot::Snapshot;

    const FILE: &str = "/rawr-hover/src/main.rs";

    /// A player whose `reset` the program calls once, after the line a
    /// hover reads at, 22.
    const PLAYER: &str = "// rawr
#[derive(Debug)]
struct Player {
    name: String,
    score: u64,
    items: Vec<u64>,
}

impl Player {
    fn reset(&mut self) -> u64 {
        self.score = 0;
        self.score
    }
}

fn main() {
    let mut player = Player { name: \"Ada\".to_string(), score: 3, items: vec![10, 20, 30] };
    let turn: u64 = 1;
    let names = vec![\"a\", \"b\"];
    let boxed = Box::new(7u64);
    let shown = *boxed - 4;
    println!(\"{} {shown}\", player.score);
    player.reset();
    println!(\"{}\", player.score);
}
";

    /// What each of `codes` reads as where `program` first pauses at
    /// `line`, then what the program printed.
    fn hovered(program: &str, line: usize, codes: &[&str]) -> (Vec<Read>, String) {
        let snapshot = Snapshot::new(FILE.into(), vec![(FILE.into(), program.into())]).unwrap();
        let mut engine = crate::Engine::new();
        let printed = Arc::new(Mutex::new(String::new()));
        let printing = Arc::clone(&printed);
        engine.on_output(move |_, text| printing.lock().unwrap().push_str(text));
        let reads = Arc::new(Mutex::new(Vec::new()));
        let shared: Arc<Mutex<Option<(Program, Stops)>>> = Arc::default();
        let (reading, at) = (Arc::clone(&reads), Arc::clone(&shared));
        let done = AtomicBool::new(false);
        let codes: Vec<String> = codes.iter().map(|code| (*code).to_owned()).collect();
        #[allow(deprecated)]
        engine.register_debugger(
            |_, mut debugger| {
                debugger.status = crate::eval::DebuggerStatus::STEP;
                debugger
            },
            move |mut context, event, node, _, position| {
                if done.load(Ordering::Acquire) {
                    return Ok(crate::debugger::DebuggerCommand::StepInto);
                }
                let paused = matches!(event, crate::debugger::DebuggerEvent::Step)
                    && at.lock().unwrap().as_ref().is_some_and(|(_, stops)| {
                        stops
                            .get(position, &node)
                            .is_some_and(|stop| stop.line == line)
                    });
                if paused && !done.swap(true, Ordering::AcqRel) {
                    let at = at.lock().unwrap();
                    let (program, _) = at.as_ref().unwrap();
                    let paused = Paused {
                        program: Some(program),
                        frame: 0,
                        position,
                        declaring: None,
                        line: 0,
                    };
                    let engine = crate::Engine::new();
                    for code in &codes {
                        let tokens = Tokens::of(&engine, code);
                        let node = tokens.whole().expect("each code is one expression");
                        let read = paused.read(&mut context, &node, code);
                        reading.lock().unwrap().push(read);
                    }
                }
                Ok(crate::debugger::DebuggerCommand::StepInto)
            },
        );
        let engine = crate::Shared::new(engine);
        let (ast, positions) = engine
            .compile_package_snapshot_with_positions(&snapshot, None)
            .unwrap_or_else(|error| panic!("{error}"));
        let (candidate, _) = engine
            .compile_package_snapshot_generation_with_top_level(&snapshot, None)
            .unwrap_or_else(|error| panic!("{error}"));
        let entry = candidate
            .located()
            .expect("a run compiled under a debugger is located")
            .entry
            .clone();
        let positions = Arc::new(positions);
        let (located, located_entry) = (Arc::clone(&positions), entry.clone());
        let stops = Stops::of(&ast, &move |position| {
            let (file, span) = located.locate_position(&located_entry, position)?;
            Some((file.identity().to_owned(), span.start().line()?))
        });
        *shared.lock().unwrap() = Some((Program::new(snapshot, entry, positions), stops));
        let mut scope = crate::Scope::new();
        let _ = engine
            .eval_ast_with_scope::<crate::Dynamic>(&mut scope, &ast)
            .unwrap();
        let main = ast
            .iter_fn_def()
            .map(|function| function.name.clone())
            .find(|name| {
                crate::resolve::lower_nominal::function_display_name(name)
                    .rsplit("::")
                    .next()
                    == Some("main")
            })
            .expect("the program has a `fn main`");
        let _ = engine
            .call_fn::<crate::Dynamic>(&mut scope, &ast, main, ())
            .unwrap_or_else(|error| panic!("{error}"));
        let reads = std::mem::take(&mut *reads.lock().unwrap());
        let printed = printed.lock().unwrap().clone();
        (reads, printed)
    }

    #[test]
    fn a_paused_frames_places_read_and_its_calls_never_run() {
        let (reads, printed) = hovered(
            PLAYER,
            22,
            &[
                "player.score",
                "player.items[2]",
                "player.items[turn]",
                "*boxed",
                "player.name",
                "names[turn as usize % names.len()]",
                "player.reset()",
            ],
        );
        let value = |text: &str| Read::Value(text.to_owned());
        assert_eq!(
            reads,
            [
                value("3"),
                value("30"),
                value("20"),
                value("7"),
                value("\"Ada\""),
                value("\"b\""),
                Read::Refused("a hover doesn't run calls: evaluate it in the console".to_owned()),
            ]
        );
        assert_eq!(printed, "3 3\n0\n", "the hover never reset the player");
    }

    #[test]
    fn a_programs_deref_and_index_impls_are_named_by_their_types() {
        let ast = crate::Engine::new()
            .compile(
                "use std::ops::{Deref, Index};\n\
                 struct Wrapper { inner: i64 }\n\
                 impl Deref for Wrapper { type Target = i64; fn deref(&self) -> &i64 { &self.inner } }\n\
                 struct Grid { cells: Vec<i64> }\n\
                 impl Index<usize> for Grid {\n\
                 \x20   type Output = i64;\n\
                 \x20   fn index(&self, at: usize) -> &i64 { &self.cells[at] }\n\
                 }\n\
                 impl Grid { fn index(&self) -> usize { 0 } }\n\
                 pub fn run() -> i64 { let w = Wrapper { inner: 1 }; let g = Grid { cells: vec![2] }; *w + g[0] + g.index() as i64 }\n\
                 run()",
            )
            .unwrap_or_else(|error| panic!("{error}"));
        let mut owned: Vec<(String, &str)> = ast
            .iter_fn_def()
            .filter_map(|function| super::owned(&function.name))
            .collect();
        owned.sort();
        assert_eq!(
            owned,
            [
                ("Grid".to_owned(), "index"),
                ("Wrapper".to_owned(), "deref")
            ],
            "an inherent `index` is no `Index`"
        );
    }
}
