//! Rust evaluated in a paused frame (#490): `{"op": "eval", "code": ..}`.
//!
//! The code is compiled as a block statement written just before the
//! statement the program is paused at, in the program as it was compiled:
//! the program's compiler checks it there, with the frame's bindings in
//! scope, so its types, borrows and errors are those of the same code
//! written at that line. The block's own statements then run in the paused
//! frame, against the values it holds. Its tail's value is answered as
//! `{:?}` prints it, through a borrow, so reading a binding doesn't move it.
//!
//! The block is the code's scope: a binding it makes ends with it. A
//! mutation Rust allows at that line changes the frame's value, and the
//! program goes on with it.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::diagnostic::{self, Report, Source};
use crate::package::snapshot::Snapshot;
use crate::splice::{self, Spliced};
use crate::{EvalContext, ImmutableString, Position};

/// The name an evaluation's errors give its code.
const IDENTITY: &str = "<eval>";

/// What compiling code into the running program takes: its files as they
/// were compiled, its entry source and the compiler's positions, which
/// place a paused step in its file.
pub struct Program {
    snapshot: Snapshot,
    entry: Source,
    positions: Arc<crate::program::Positions>,
}

impl Program {
    /// The running program, compiled from `snapshot` with `entry` its
    /// entry source and `positions` its compiler's.
    #[must_use]
    pub const fn new(
        snapshot: Snapshot,
        entry: Source,
        positions: Arc<crate::program::Positions>,
    ) -> Self {
        Self {
            snapshot,
            entry,
            positions,
        }
    }
}

/// Code written into one of the program's files.
struct Written {
    /// The file, as the program had it and as the reader sees the code.
    shown: [Source; 2],
    spliced: Spliced,
    /// Where the code's frame starts in the spliced file.
    frame: Position,
}

/// The answer to `code` evaluated where `context` is paused, at
/// `position`: `{"value": ..}`, its tail's `{:?}` text, or `{"error":
/// ..}`, its compile error or panic as rustc and Rust report them.
pub(super) fn answer(
    context: &mut EvalContext,
    program: &Program,
    position: Position,
    code: &str,
) -> Value {
    match evaluate(context, program, position, code) {
        Ok(value) => json!({ "value": value }),
        Err(error) => json!({ "error": error }),
    }
}

fn evaluate(
    context: &mut EvalContext,
    program: &Program,
    position: Position,
    code: &str,
) -> Result<String, String> {
    let (file, span) = program
        .positions
        .locate_position(&program.entry, position)
        .ok_or("the program is paused in code rawr wrote around it")?;
    let text = file.text();
    let at = diagnostic::offset(
        text,
        span.start().line().unwrap_or(1),
        span.start().position().unwrap_or(1),
    );
    let mut spliced = Spliced::default();
    spliced.copy(0, 0, &text[..at]);
    let code_split = splice::split(context.engine(), code);
    let frame = spliced
        .frame(1, 0, &code_split, true)
        .map_or(Position::NONE, |start| spliced.position(start));
    spliced.copy(0, at, &text[at..]);
    let shown = [
        file.clone(),
        Source::new(IDENTITY, code).map_err(|error| error.to_string())?,
    ];
    let splice = Written {
        shown,
        spliced,
        frame,
    };
    let identity = file.identity().to_owned();
    let snapshot = program
        .snapshot
        .with_text(std::path::Path::new(&identity), splice.spliced.text())
        .ok_or_else(|| format!("{identity} isn't one of the program's files"))?;
    let engine = context.engine();
    let compiled = crate::package::Graph::load_snapshot(engine, &snapshot)
        .and_then(|graph| engine.compile_package_with_positions(graph));
    let (ast, _, positions) = compiled.map_err(|error| match error.report() {
        Ok(report) => splice.rendered(&report, &identity),
        Err(_) => error.to_string(),
    })?;
    let entry = if program.entry.identity() == identity {
        Source::new(identity.as_str(), splice.spliced.text()).map_err(|error| error.to_string())?
    } else {
        program.entry.clone()
    };
    let place = |position: Position| {
        positions
            .locate_position(&entry, position)
            .filter(|(source, _)| source.identity() == identity)
            .map(|(_, span)| span.start())
    };
    let statements = splice::framed(ast.statements(), splice.frame, &place)
        .or_else(|| {
            ast.iter_fn_def()
                .find_map(|function| splice::framed(function.body.iter(), splice.frame, &place))
        })
        .ok_or("the code's statements aren't where it was written")?;

    let (engine, global, caches, scope, this) = context.parts();
    let libraries = global.lib.len();
    global.lib.push(ast.shared_lib().clone());
    global.lib.push(ast.shared_support().clone());
    let resolutions = caches.fn_resolution_caches_len();
    caches.push_fn_resolution_cache();
    let bindings = scope.len();
    let ran = engine.eval_stmt_block(global, caches, scope, this, &statements, false);
    let value = scope
        .get_value::<ImmutableString>(splice::VALUE)
        .map(|value| value.to_string());
    scope.rewind(bindings);
    caches.rewind_fn_resolution_caches(resolutions);
    global.lib.truncate(libraries);
    match ran {
        Ok(_) => Ok(value.unwrap_or_else(|| "()".to_owned())),
        Err(error) => {
            let report = match positions.locate(&entry, &error) {
                Some((source, span)) => {
                    Report::from_evaluation_located(source.clone(), Some(span), &error)
                }
                None => Report::from_evaluation(None, &error),
            };
            Err(report.map_or_else(
                |_| error.to_string(),
                |report| splice.rendered(&report, &identity),
            ))
        }
    }
}

impl Written {
    /// `report`, an error in the spliced file `identity`, rendered where
    /// the reader sees it: in the file as the program has it, or in the
    /// code.
    fn rendered(&self, report: &Report, identity: &str) -> String {
        let shown = self.spliced.relabel(report, identity, &self.shown);
        diagnostic::render(&shown).trim_end().to_owned()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    use super::super::stops::Stops;
    use super::*;

    const FILE: &str = "/rawr-eval/src/main.rs";

    /// What evaluating each of `codes` answers where `program` first pauses
    /// at `line`, then what the program printed as it went on.
    fn evaluated(program: &str, line: usize, codes: &[&str]) -> (Vec<Value>, String) {
        let snapshot = Snapshot::new(FILE.into(), vec![(FILE.into(), program.into())]).unwrap();
        let printed = Arc::new(Mutex::new(String::new()));
        let answers = Arc::new(Mutex::new(Vec::new()));
        let shared: Arc<Mutex<Option<(Program, Stops)>>> = Arc::default();
        let mut engine = crate::Engine::new();
        let printing = Arc::clone(&printed);
        engine.on_output(move |_, text| printing.lock().unwrap().push_str(text));
        let answering = Arc::clone(&answers);
        let at = Arc::clone(&shared);
        let done = AtomicBool::new(false);
        let codes: Vec<String> = codes.iter().map(|code| (*code).to_owned()).collect();
        #[allow(deprecated)]
        engine.register_debugger(
            |_, mut debugger| {
                debugger.status = crate::eval::DebuggerStatus::STEP;
                debugger
            },
            move |mut context, event, node, _, position| {
                // The code's own steps call back too, inside the pause: as
                // the inspector's `answering` does, they go on at once.
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
                    for code in &codes {
                        let answer = answer(&mut context, program, position, code);
                        answering.lock().unwrap().push(answer);
                    }
                }
                Ok(crate::debugger::DebuggerCommand::StepInto)
            },
        );
        let graph = crate::package::Graph::load_snapshot(&engine, &snapshot).unwrap();
        let (ast, main, positions) = engine.compile_package_with_positions(graph).unwrap();
        let entry = Source::new(FILE, program).unwrap();
        let positions = Arc::new(positions);
        let located = Arc::clone(&positions);
        let located_entry = entry.clone();
        let stops = Stops::of(&ast, &move |position| {
            let (file, span) = located.locate_position(&located_entry, position)?;
            Some((file.identity().to_owned(), span.start().line()?))
        });
        *shared.lock().unwrap() = Some((Program::new(snapshot, entry, positions), stops));
        let mut scope = crate::Scope::new();
        let _ = engine
            .eval_ast_with_scope::<crate::Dynamic>(&mut scope, &ast)
            .unwrap();
        let _ = engine
            .call_fn::<crate::Dynamic>(&mut scope, &ast, main.unwrap(), ())
            .unwrap_or_else(|error| panic!("{error}"));
        let answers = answers.lock().unwrap().clone();
        let printed = printed.lock().unwrap().clone();
        (answers, printed)
    }

    const PROGRAM: &str = "// rawr
fn main() {
    let mut v = vec![1, 2, 3];
    let count: u64 = 3;
    let mut total = 0;
    let first = &v[0];
    println!(\"{first} {total}\");
    v.push(4);
    println!(\"{} {total}\", v.len());
}
";

    #[test]
    fn code_reads_and_computes_with_the_paused_frames_bindings() {
        let (answers, printed) = evaluated(
            PROGRAM,
            7,
            &[
                "count * 2",
                "let w = v.len() as u64; w + count",
                "v",
                "(count, first)",
                "total += 10;",
            ],
        );
        assert_eq!(
            answers,
            [
                json!({ "value": "6" }),
                json!({ "value": "6" }),
                // Read through a borrow: the program still has `v`.
                json!({ "value": "[1, 2, 3]" }),
                json!({ "value": "(3, 1)" }),
                json!({ "value": "()" }),
            ]
        );
        // The program goes on with what the code changed.
        assert_eq!(printed, "1 10\n4 10\n");
    }

    #[test]
    fn code_is_refused_as_rustc_refuses_it_where_the_program_is_paused() {
        let (answers, _) = evaluated(PROGRAM, 7, &["count + \"a\"", "v.push(5)"]);
        let errors: Vec<&str> = answers
            .iter()
            .map(|answer| {
                answer["error"]
                    .as_str()
                    .unwrap_or_else(|| panic!("{answer}"))
            })
            .collect();
        assert_eq!(
            errors[0],
            "error[E0277]: cannot add `&str` to `u64`\n --> <eval>:1:7\n  |\n1 | count + \"a\"\n  \
             |       ^ no implementation for `u64 + &str`\n  |\n  = help: the trait `Add<&str>` \
             is not implemented for `u64`"
        );
        // The paused program holds a shared borrow of `v`, which it uses
        // after the line: a mutable borrow there is rustc's E0502, each
        // label where its part is written.
        assert!(
            errors[1].starts_with(
                "error[E0502]: cannot borrow `v` as mutable because it is also borrowed as \
                 immutable"
            ),
            "{}",
            errors[1]
        );
        assert!(errors[1].contains("<eval>:1:1"), "{}", errors[1]);
        assert!(errors[1].contains("main.rs:6"), "{}", errors[1]);
        assert!(!errors[1].contains("__rawr"), "{}", errors[1]);
    }
}
