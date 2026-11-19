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
//!
//! With `"frame": n`, the code runs in a caller instead (#2309): it is
//! written before the caller's statement that makes the call on the stack,
//! the innermost one around it, and runs against the bindings the caller
//! lent that call (`Lent`). The values move into a scope of the code's own
//! for the run, at the same indices, and back after it, so nothing the
//! code adds moves the caller's entries while its call runs. A binding the
//! call holds by reference is borrowed until it returns, so code that uses
//! it is refused.

use std::ptr::NonNull;
use std::sync::Arc;

use serde_json::{json, Value};

use crate::ast::{ASTNode, Expr, Stmt};
use crate::diagnostic::{self, Report, Source};
use crate::engine::{KEYWORD_HELD, KEYWORD_SLOT_OF, KEYWORD_THIS, KEYWORD_THROUGH};
use crate::package::snapshot::Snapshot;
use crate::splice::{self, Spliced};
use crate::{Dynamic, EvalContext, ImmutableString, Position, Scope};

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

/// The frame code runs in: the statement it is written before, and whose
/// bindings it runs against.
struct Frame {
    /// The statement's position, as the program was compiled.
    statement: Position,
    /// A caller's bindings, as the call it made keeps them; `None` for the
    /// frame the program is paused in, whose bindings the step holds.
    caller: Option<Caller>,
}

/// A caller's bindings while the call it made runs (`Lent`).
struct Caller {
    /// The scope it keeps them in, and where its own entries end there.
    scope: NonNull<Scope<'static>>,
    end: usize,
    /// Its receiver, when the call doesn't hold it.
    this: Option<NonNull<Dynamic>>,
    /// The binding the call holds by reference, which code can't use
    /// while the call runs.
    held: Option<Held>,
    /// The receiver's name and what code that reaches it is told, when
    /// the call kept none.
    unkept: Option<(ImmutableString, String)>,
}

/// A caller's binding its running call holds by reference.
struct Held {
    /// The local, by its index in the caller's scope, or `None` for the
    /// receiver.
    index: Option<usize>,
    name: ImmutableString,
    /// Why code can't use it.
    why: String,
}

/// The answer to `code` evaluated in frame `frame` (0 for the function
/// the program is paused in, 1 for its caller, and on) where `context` is
/// paused, at `position`: `{"value": ..}`, its tail's `{:?}` text, or
/// `{"error": ..}`, its compile error or panic as rustc and Rust report
/// them.
pub(super) fn answer(
    context: &mut EvalContext,
    program: &Program,
    frame: usize,
    position: Position,
    code: &str,
) -> Value {
    let evaluated = frame_of(context, frame, position)
        .and_then(|frame| evaluate(context, program, &frame, code));
    match evaluated {
        Ok(value) => json!({ "value": value }),
        Err(error) => json!({ "error": error }),
    }
}

/// Frame `frame` where `context` is paused, at `position`. A caller's
/// statement is the one making the call that is on the stack, and its
/// bindings are where that call was lent them.
fn frame_of(context: &mut EvalContext, frame: usize, position: Position) -> Result<Frame, String> {
    if frame == 0 {
        return Ok(Frame {
            statement: position,
            caller: None,
        });
    }
    let base = NonNull::from(context.scope_mut()).cast::<Scope<'static>>();
    let frames = super::script_frames(context);
    let global = context.global_runtime_state();
    let stack = global.debugger.call_stack();
    let (Some(&index), Some(&within)) = (frames.get(frame), frames.get(frame - 1)) else {
        let last = frames.len().saturating_sub(1);
        // The debugger's stack has no call made before it attached (D1075).
        return Err(format!(
            "error: frame {frame} isn't on the debugger's stack, which has frames 0 to \
             {last}: a call made before the debugger attached isn't on it"
        ));
    };
    let shown =
        |symbol: &str| crate::resolve::lower_nominal::function_display_name(symbol).to_owned();
    let function = &stack[index];
    let call = &stack[index + 1];
    let (name, callee) = (shown(&function.fn_name), shown(&stack[within].fn_name));
    let kept = || {
        format!(
            "error: `{name}`'s bindings can't be reached while it calls `{callee}`: rawr \
             doesn't keep them for this call"
        )
    };
    let lent = call.caller.as_ref().ok_or_else(kept)?;
    let (scope, end) = super::scope_of(base, stack, &frames, frame).ok_or_else(kept)?;
    let statement = call_statement(global, &function.fn_name, call.pos)
        .ok_or_else(|| format!("error: `{name}` is calling `{callee}` from code rawr wrote"))?;
    let this_name = function
        .bindings
        .as_ref()
        .and_then(|bindings| bindings.this_name.clone());
    let borrowed = |held: &str| {
        format!("error: `{held}` is borrowed by the call to `{callee}`, which is still running")
    };
    let held = match lent.held {
        Some(crate::eval::Held::Local(place)) => {
            // SAFETY: as for `super::entries`, a shared reference inside
            // this step's callback; only the entries' addresses are read.
            let lent: &Scope = unsafe { scope.as_ref() };
            let found = lent
                .places()
                .take(end)
                .enumerate()
                .find(|(_, (_, at))| *at == place);
            let Some((index, (local, _))) = found else {
                // A place inside one of the frame's values: which one isn't
                // known, so the frame is borrowed whole.
                return Err(format!(
                    "error: `{name}`'s bindings are borrowed by the call to `{callee}`, which \
                     is still running"
                ));
            };
            Some(Held {
                index: Some(index),
                why: borrowed(local),
                name: local.clone(),
            })
        }
        Some(crate::eval::Held::This) => this_name.as_ref().map(|this| Held {
            index: None,
            why: borrowed(this),
            name: this.clone(),
        }),
        None => None,
    };
    let unkept = this_name
        .filter(|_| lent.this.is_none() && held.as_ref().is_none_or(|held| held.index.is_some()))
        .map(|this| {
            let why = format!(
                "error: `{this}` can't be reached while `{callee}` runs: rawr doesn't keep it \
                 for this call"
            );
            (this, why)
        });
    Ok(Frame {
        statement,
        caller: Some(Caller {
            scope,
            end,
            this: lent.this,
            held,
            unkept,
        }),
    })
}

/// The statement of the function `function` that makes the call at
/// `call`: the innermost one around it, so code written before it is in
/// the scope the call is made in.
fn call_statement(
    global: &crate::GlobalRuntimeState,
    function: &str,
    call: Position,
) -> Option<Position> {
    let mut functions: Vec<_> = global
        .lib
        .iter()
        .flat_map(|lib| lib.iter_script_fn())
        .map(|(.., function)| crate::Shared::clone(function))
        .collect();
    // A closure runs the body its pointer holds.
    let mut index = 0;
    while let Some(within) = functions.get(index).cloned() {
        super::stops::closures(within.body.statements(), &mut functions);
        index += 1;
    }
    functions
        .iter()
        .filter(|candidate| candidate.name == function)
        .find_map(|candidate| {
            let mut found = None;
            let mut path = Vec::new();
            for statement in candidate.body.iter() {
                statement.walk(&mut path, &mut |path: &[ASTNode]| {
                    if let Some(node @ ASTNode::Expr(..)) = path.last()
                        && node.position() == call
                    {
                        found = path.iter().rev().find_map(|node| match node {
                            ASTNode::Stmt(statement) => Some(statement.position()),
                            ASTNode::Expr(..) => None,
                        });
                    }
                    found.is_none()
                });
                path.clear();
                if found.is_some() {
                    break;
                }
            }
            found
        })
}

/// Whether `statements` use the binding `name`, or the receiver when
/// `receiver` is set: by name, or where it is, as the compiler names that
/// (`KEYWORD_SLOT_OF`, `KEYWORD_THROUGH`, `KEYWORD_HELD`).
fn uses(statements: &[Stmt], name: &str, receiver: bool) -> bool {
    let named = |mut local: &str| {
        while let Some(rest) = [KEYWORD_SLOT_OF, KEYWORD_THROUGH, KEYWORD_HELD]
            .iter()
            .find_map(|prefix| local.strip_prefix(prefix))
        {
            local = rest;
        }
        local == name || (receiver && local == KEYWORD_THIS)
    };
    let mut found = false;
    let mut path = Vec::new();
    for statement in statements {
        statement.walk(&mut path, &mut |path: &[ASTNode]| {
            found = match path.last() {
                Some(ASTNode::Expr(Expr::Variable(variable, ..))) => named(&variable.1),
                Some(ASTNode::Expr(Expr::ThisPtr(..))) => receiver,
                _ => false,
            };
            !found
        });
        path.clear();
        if found {
            break;
        }
    }
    found
}

fn evaluate(
    context: &mut EvalContext,
    program: &Program,
    frame: &Frame,
    code: &str,
) -> Result<String, String> {
    let (file, span) = program
        .positions
        .locate_position(&program.entry, frame.statement)
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
    let start = spliced
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
        frame: start,
    };
    let identity = file.identity().to_owned();
    let snapshot = program
        .snapshot
        .with_text(std::path::Path::new(&identity), splice.spliced.text())
        .ok_or_else(|| format!("{identity} isn't one of the program's files"))?;
    // The run's own compiler compiles it as it compiled the run: a crate's
    // generic call takes the types the run's lowering revealed.
    let compiled = context
        .engine()
        .compile_package_snapshot_with_positions(&snapshot);
    let (ast, positions) = compiled.map_err(|error| match error.report() {
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
    if let Some(caller) = &frame.caller {
        let refused = caller
            .held
            .as_ref()
            .map(|held| (held.name.as_str(), held.index.is_none(), &held.why))
            .into_iter()
            .chain(
                caller
                    .unkept
                    .as_ref()
                    .map(|(this, why)| (this.as_str(), true, why)),
            )
            .find(|(name, receiver, _)| uses(&statements, name, *receiver));
        if let Some((.., why)) = refused {
            return Err(why.clone());
        }
    }

    let (engine, global, caches, scope, this) = context.parts();
    let libraries = global.lib.len();
    global.lib.push(ast.shared_lib().clone());
    global.lib.push(ast.shared_support().clone());
    let resolutions = caches.fn_resolution_caches_len();
    caches.push_fn_resolution_cache();
    let (ran, value) = match &frame.caller {
        None => {
            let bindings = scope.len();
            let ran = engine.eval_stmt_block(global, caches, scope, this, &statements, false);
            let value = tail(scope);
            scope.rewind(bindings);
            (ran, value)
        }
        Some(caller) => {
            // A caller whose call extended the paused frame's scope keeps its
            // bindings in that scope, which the step lends here.
            let own = caller.scope == NonNull::from(&mut *scope).cast();
            // SAFETY: the caller's call is on the call stack, below the step
            // `context` is at, on this thread, and its caller uses neither
            // pointer until it returns (`Lent`). The references end with
            // this block, inside the step's callback.
            let (lent, this) = unsafe {
                (
                    if own {
                        scope
                    } else {
                        caller.scope.cast().as_mut()
                    },
                    caller.this.map(|mut this| this.as_mut()),
                )
            };
            // The code runs in a scope of its own holding the caller's
            // values, moved there, at the same indices. What it adds goes
            // there, so the caller's entries stay where its running call
            // may hold one.
            let end = caller.end.min(lent.len());
            let held = caller.held.as_ref().and_then(|held| held.index);
            let mut moved = Scope::with_capacity(end);
            let names: Vec<ImmutableString> = lent
                .iter_inner()
                .take(end)
                .map(|(name, ..)| name.clone())
                .collect();
            for (index, name) in names.into_iter().enumerate() {
                let value = if held == Some(index) {
                    Dynamic::UNIT
                } else {
                    std::mem::take(lent.get_mut_by_index(index))
                };
                let access = value.access_mode();
                moved.push_entry(name, access, value);
            }
            // The call is made inside the statement, past where the code
            // is compiled: entries the statement made before it stand
            // between, so the code finds its bindings by name.
            let searching = std::mem::replace(&mut global.always_search_scope, true);
            let ran = engine.eval_stmt_block(global, caches, &mut moved, this, &statements, false);
            global.always_search_scope = searching;
            let value = tail(&moved);
            for index in (0..end).filter(|&index| held != Some(index)) {
                *lent.get_mut_by_index(index) = std::mem::take(moved.get_mut_by_index(index));
            }
            (ran, value)
        }
    };
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

/// The `{:?}` text of the code's tail, which its frame keeps in
/// [`splice::VALUE`] in `scope`.
fn tail(scope: &Scope) -> Option<String> {
    scope
        .get_value::<ImmutableString>(splice::VALUE)
        .map(|value| value.to_string())
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
        evaluated_in(program, line, 0, codes)
    }

    /// [`evaluated`], each code in frame `frame` of the pause.
    fn evaluated_in(
        program: &str,
        line: usize,
        frame: usize,
        codes: &[&str],
    ) -> (Vec<Value>, String) {
        evaluated_on(
            crate::Engine::new(),
            crate::Shared::new,
            program,
            (line, frame),
            codes,
        )
    }

    /// [`evaluated_in`] at `(line, frame)`, on `engine` as `finish` makes
    /// it once the debugger is registered: the program compiles as a run
    /// compiles it on that engine.
    fn evaluated_on(
        mut engine: crate::Engine,
        finish: impl FnOnce(crate::Engine) -> crate::Shared<crate::Engine>,
        program: &str,
        (line, frame): (usize, usize),
        codes: &[&str],
    ) -> (Vec<Value>, String) {
        let snapshot = Snapshot::new(FILE.into(), vec![(FILE.into(), program.into())]).unwrap();
        let printed = Arc::new(Mutex::new(String::new()));
        let answers = Arc::new(Mutex::new(Vec::new()));
        let shared: Arc<Mutex<Option<(Program, Stops)>>> = Arc::default();
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
                        let answer = answer(&mut context, program, frame, position, code);
                        answering.lock().unwrap().push(answer);
                    }
                }
                Ok(crate::debugger::DebuggerCommand::StepInto)
            },
        );
        let engine = finish(engine);
        let (ast, positions) = engine
            .compile_package_snapshot_with_positions(&snapshot)
            .unwrap_or_else(|error| panic!("{error}"));
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

    /// A frame with a shared and a mutable borrow and an immutable binding.
    const BORROWS: &str = "// rawr
fn bump(shared: &u64, unique: &mut u64) {
    let fixed = 1;
    println!(\"{shared} {unique} {fixed}\");
}

fn main() {
    let a: u64 = 1;
    let mut b: u64 = 2;
    bump(&a, &mut b);
    println!(\"{a} {b}\");
}
";

    #[test]
    fn code_writes_through_a_mutable_borrow_and_not_a_shared_one_or_an_immutable_binding() {
        let (answers, printed) = evaluated(
            BORROWS,
            4,
            &["*shared = 5;", "fixed = 5;", "*unique += 10;"],
        );
        let error = |answer: &Value| answer["error"].as_str().unwrap_or_default().to_owned();
        assert!(
            error(&answers[0]).starts_with(
                "error[E0594]: cannot assign to `*shared`, which is behind a `&` reference"
            ),
            "{}",
            answers[0]
        );
        assert!(
            error(&answers[1])
                .starts_with("error[E0384]: cannot assign twice to immutable variable `fixed`"),
            "{}",
            answers[1]
        );
        assert_eq!(answers[2], json!({ "value": "()" }));
        // The frame and its caller go on with the write the borrow allowed,
        // and without the two it refused.
        assert_eq!(printed, "1 12 1\n1 12\n");
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

    /// A caller that keeps a local, `turn`, and calls `inner` with a copy
    /// of it and a borrow of another, `held`, mid-statement.
    const CALLER: &str = "// rawr
fn inner(n: u64, held: &mut u64) -> u64 {
    let doubled = n * 2;
    *held += 1;
    doubled
}

fn main() {
    let mut turn: u64 = 1;
    let mut held: u64 = 0;
    let next = 1 + inner(turn, &mut held);
    println!(\"{turn} {next} {held}\");
}
";

    #[test]
    fn code_reads_and_writes_a_callers_bindings_in_its_frame() {
        let (answers, printed) = evaluated_in(
            CALLER,
            3,
            1,
            &["turn + 1", "turn = 10;", "turn", "doubled", "held"],
        );
        assert_eq!(answers[0], json!({ "value": "2" }));
        assert_eq!(answers[1], json!({ "value": "()" }));
        assert_eq!(answers[2], json!({ "value": "10" }));
        let error = |answer: &Value| answer["error"].as_str().unwrap_or_default().to_owned();
        // `inner`'s own bindings aren't in `main`'s scope.
        assert!(
            error(&answers[3]).starts_with("error[E0425]: cannot find value `doubled`"),
            "{}",
            answers[3]
        );
        // The running call holds `held` by reference until it returns.
        assert!(
            error(&answers[4]).contains("`held` is borrowed by the call to `main::inner`"),
            "{}",
            answers[4]
        );
        // `main` goes on with the value written in its frame; the call it
        // was making had its argument already.
        assert_eq!(printed, "10 3 1\n");
    }

    /// A program whose generic function passes its type parameter to a
    /// crate's generic function, which a runner's lowering compiles per
    /// instance, as it reveals an opaque return type: code evaluated in it
    /// checks as the run checked it, so the crate's call takes the type
    /// the instance gives it (#2292).
    #[cfg(feature = "rust-import")]
    #[test]
    fn code_checks_in_a_program_whose_crate_call_takes_an_instance_type() {
        use crate::package::import::runner::account;
        let program = "// rawr
fn shown<T: Clone>(value: T) -> T { native::echo(value) }
fn echoed() -> i64 { shown(7i64) }
fn main() {
    let count = 3;
    println!(\"{count}\");
}
";
        let (answers, printed) = evaluated_on(
            crate::package::compile::captured::tests::compiler(),
            |engine| {
                let native = account::Native::transform(account::tests::carrier());
                engine
                    .retain_imported_native(native)
                    .expect("a fresh compiler retains a native surface")
            },
            program,
            (6, 0),
            &["count + 1"],
        );
        assert_eq!(answers, [json!({ "value": "4" })]);
        assert_eq!(printed, "3\n");
    }
}
