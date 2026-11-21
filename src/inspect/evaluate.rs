//! Rust evaluated in a paused frame (#490): `{"op": "eval", "code": ..}`.
//!
//! The code is compiled as a block statement written just before the
//! statement the program is paused at, in the program as it was compiled:
//! the program's compiler checks it there, with the frame's bindings in
//! scope, so its types, borrows and errors are those of the same code
//! written at that line. Where that statement is written as an expression
//! (a match arm's body without braces), the code's block and the
//! expression go in a block of their own (#2265). The block's own
//! statements then run in the paused frame, against the values it holds.
//! Its tail's value is answered as `{:?}` prints it, through a borrow, so
//! reading a binding doesn't move it.
//!
//! A runner's code compiles as a save compiles against the paused build's
//! native carrier, and binds against the paused generation's registry, so
//! a use of a crate the build was never compiled with (a field the program
//! never reads, a generic at a new type) is refused where it is written,
//! before any statement runs (#2406). The paused process gains no native
//! code; a save that writes the use rebuilds the runner with it.
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
/// place a paused step in its file, and the parses it compiled from, when
/// its compiler kept them.
pub struct Program {
    snapshot: Snapshot,
    entry: Source,
    positions: Arc<crate::program::Positions>,
    /// Read for each file the code isn't written into, which then isn't
    /// parsed again; nothing an evaluation does is kept in them.
    parses: Option<crate::package::Parses>,
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
            parses: None,
        }
    }

    /// The program, which compiled from `parses`.
    #[must_use]
    pub(crate) fn reading(self, parses: Option<crate::package::Parses>) -> Self {
        Self { parses, ..self }
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
    if file.expansion_at(span.start()).is_some() {
        return Err(
            "the program is paused in a macro's expansion, where no code is written".into(),
        );
    }
    let identity = file.identity().to_owned();
    // The file as it is written. The run's source of it goes on past the
    // file's own lines with the expansions of the macro calls among its
    // items, which compiling the spliced file lays out again.
    let text = program
        .snapshot
        .read(std::path::Path::new(&identity))
        .ok_or_else(|| format!("{identity} isn't one of the program's files"))?;
    let at = diagnostic::offset(
        text,
        span.start().line().unwrap_or(1),
        span.start().position().unwrap_or(1),
    );
    let mut spliced = Spliced::default();
    spliced.copy(0, 0, &text[..at]);
    let code_split = splice::split(context.engine(), code);
    // Where the paused statement is written as an expression, a match
    // arm's or a closure's body without braces, a block statement before
    // it isn't Rust: the code's block and the expression go in a block of
    // their own there, whose value is the expression's.
    let expression = expression_end(context.engine(), text, span);
    if expression.is_some() {
        spliced.write(0, at, "{ ");
    }
    let start = spliced
        .frame(1, 0, &code_split, true)
        .map_or(Position::NONE, |start| spliced.position(start));
    match expression {
        Some(end) => {
            spliced.copy(0, at, &text[at..end]);
            spliced.write(0, end, " }");
            spliced.copy(0, end, &text[end..]);
        }
        None => spliced.copy(0, at, &text[at..]),
    }
    let shown = [
        file.clone(),
        Source::new(IDENTITY, code).map_err(|error| error.to_string())?,
    ];
    let splice = Written {
        shown,
        spliced,
        frame: start,
    };
    let snapshot = program
        .snapshot
        .with_text(std::path::Path::new(&identity), splice.spliced.text())
        .ok_or_else(|| format!("{identity} isn't one of the program's files"))?;
    // The run's own compiler compiles it as it compiled the run: a crate's
    // generic call takes the types the run's lowering revealed, and a use
    // the runner's build lacks is refused where it is written (#2406). It
    // reads the generation's parses and keeps nothing in the run's caches.
    let compiled = context
        .engine()
        .compile_package_snapshot_with_positions(&snapshot, program.parses.as_ref());
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
    // The paused generation's registry binds the code's requirements as a
    // run's binds the program's (`Hosted::bind`): one it lacks is refused
    // before any statement runs, not when its call is reached.
    if let (Some(registry), Some(hosted)) = (global.host_runtime.as_deref(), ast.host()) {
        hosted.validate(registry).map_err(|error| {
            format!(
                "error: {error}: the running build was not compiled with this use\n  \
                 = note: writing this use in the program and saving it rebuilds the runner with it"
            )
        })?;
    }
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

/// Where the statement written at `span` of `text` ends, when it is
/// written where Rust takes an expression rather than a statement: the
/// token before it is none of those a statement follows (`{`, `}`, `;`,
/// an attribute's `]`), such as a match arm's `=>` or a closure's `|`.
fn expression_end(engine: &crate::Engine, text: &str, span: crate::Span) -> Option<usize> {
    use crate::tokenizer::Token;
    let start = span.start();
    let (before, _) = crate::resolve::lower_nominal::lex_rust(engine, text)
        .into_iter()
        .take_while(|(token, position)| *token != Token::EOF && *position < start)
        .last()?;
    if matches!(
        before,
        Token::LeftBrace | Token::RightBrace | Token::SemiColon | Token::RightBracket
    ) {
        return None;
    }
    // The span ends at its last character.
    let end = span.end();
    if end < start {
        return None;
    }
    let last = diagnostic::offset(text, end.line()?, end.position()?);
    let width = text[last..].chars().next()?.len_utf8();
    Some(last + width)
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
        at: (usize, usize),
        codes: &[&str],
    ) -> (Vec<Value>, String) {
        let snapshot = Snapshot::new(FILE.into(), vec![(FILE.into(), program.into())]).unwrap();
        let probe = Probe::register(&mut engine, at, codes);
        let engine = finish(engine);
        let (ast, positions) = engine
            .compile_package_snapshot_with_positions(&snapshot, None)
            .unwrap_or_else(|error| panic!("{error}"));
        // The entry as a run's debugger has it: with the expansions of the
        // macro calls among its items laid out past the file's own lines.
        let (candidate, _) = engine
            .compile_package_snapshot_generation_with_top_level(&snapshot, None)
            .unwrap_or_else(|error| panic!("{error}"));
        let entry = candidate
            .located()
            .expect("a run compiled under a debugger is located")
            .entry
            .clone();
        let positions = Arc::new(positions);
        let located = Arc::clone(&positions);
        let located_entry = entry.clone();
        let stops = Stops::of(&ast, &move |position| {
            let (file, span) = located.locate_position(&located_entry, position)?;
            Some((file.identity().to_owned(), span.start().line()?))
        });
        *probe.shared.lock().unwrap() = Some((Program::new(snapshot, entry, positions), stops));
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
        probe.finish()
    }

    /// A debugger on an engine that answers codes where its program first
    /// pauses at a line, in a frame, and what the program prints.
    struct Probe {
        printed: Arc<Mutex<String>>,
        answers: Arc<Mutex<Vec<Value>>>,
        /// The program the codes compile into and its stops, once it is
        /// compiled.
        shared: Arc<Mutex<Option<(Program, Stops)>>>,
    }

    impl Probe {
        /// Register on `engine` the debugger that evaluates each of
        /// `codes` in frame `frame` where the program first pauses at
        /// `line`.
        fn register(
            engine: &mut crate::Engine,
            (line, frame): (usize, usize),
            codes: &[&str],
        ) -> Self {
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
                    // The code's own steps call back too, inside the pause:
                    // as the inspector's `answering` does, they go on at once.
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
            Self {
                printed,
                answers,
                shared,
            }
        }

        /// What the codes answered, and what the program printed.
        fn finish(self) -> (Vec<Value>, String) {
            let answers = self.answers.lock().unwrap().clone();
            let printed = self.printed.lock().unwrap().clone();
            (answers, printed)
        }
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

    /// A method that takes `&mut self` and matches its argument, called on
    /// a value its caller prints after the call.
    const METHOD: &str = "// rawr
struct Counter {
    value: i64,
}

impl Counter {
    fn update(&mut self, step: i64) {
        match step {
            1 => {
                self.value += step;
            }
            _ => {}
        }
    }
}

fn main() {
    let mut counter = Counter { value: 0 };
    counter.update(1);
    println!(\"{}\", counter.value);
}
";

    #[test]
    fn an_assignment_in_a_method_writes_through_self_with_or_without_its_semicolon() {
        let (answers, printed) = evaluated(
            METHOD,
            10,
            &[
                "self.value = 100",
                "self.value",
                "self.value = 7;",
                "self.value *= 2",
            ],
        );
        assert_eq!(
            answers,
            [
                json!({ "value": "()" }),
                json!({ "value": "100" }),
                json!({ "value": "()" }),
                json!({ "value": "()" }),
            ]
        );
        // The paused statement then adds its step to the value written.
        assert_eq!(printed, "15\n");
    }

    /// [`METHOD`] with a macro called among its items, whose expansion the
    /// compiler lays out past the file's own lines.
    const EXPANDED: &str = "// rawr
struct Counter {
    value: i64,
}

impl Counter {
    fn update(&mut self, step: i64) {
        match step {
            1 => {
                self.value += step;
            }
            _ => {}
        }
    }
}

macro_rules! reader {
    () => {
        impl Counter {
            fn read(&self) -> i64 {
                self.value
            }
        }
    };
}

reader!();

fn main() {
    let mut counter = Counter { value: 0 };
    counter.update(1);
    println!(\"{}\", counter.read());
}
";

    #[test]
    fn code_is_written_into_the_files_own_text_not_its_laid_out_expansions() {
        let (answers, printed) = evaluated(EXPANDED, 10, &["self.value = 4;", "self.read()"]);
        assert_eq!(answers, [json!({ "value": "()" }), json!({ "value": "4" })]);
        assert_eq!(printed, "5\n");
    }

    #[test]
    fn a_refused_assignment_points_at_the_code_as_written() {
        let (answers, _) = evaluated(METHOD, 10, &["self.value = \"a\"", "step = 2;"]);
        let error = |answer: &Value| answer["error"].as_str().unwrap_or_default().to_owned();
        let mismatched = error(&answers[0]);
        assert!(
            mismatched.starts_with("error[E0308]: mismatched types"),
            "{}",
            answers[0]
        );
        assert!(mismatched.contains("<eval>:1:14"), "{mismatched}");
        let immutable = error(&answers[1]);
        assert!(
            immutable.starts_with("error[E0384]: cannot assign twice to immutable variable `step`"),
            "{}",
            answers[1]
        );
        assert!(immutable.contains("<eval>:1:1"), "{immutable}");
        assert!(!immutable.contains("__rawr"), "{immutable}");
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

    #[test]
    fn code_runs_where_the_program_is_paused_at_a_match_arms_expression() {
        let program = "// rawr
fn main() {
    let n: u64 = 2;
    let m = match n {
        1 => 10,
        _ => n * 3,
    };
    println!(\"{m}\");
}
";
        let (answers, printed) = evaluated(program, 6, &["n + 1", "let k = n; k * 10"]);
        assert_eq!(answers, [json!({ "value": "3" }), json!({ "value": "20" })]);
        assert_eq!(printed, "6\n");
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
                engine
                    .retain_imported_native(echo_native())
                    .expect("a fresh compiler retains a native surface")
            },
            program,
            (6, 0),
            &["count + 1"],
        );
        assert_eq!(answers, [json!({ "value": "4" })]);
        assert_eq!(printed, "3\n");
    }

    /// The selector of the `native` crate's `echo`.
    #[cfg(feature = "rust-import")]
    fn echo() -> rawr_import::surface::identity::Selector {
        use rawr_import::surface::identity::{Definition, Selector};
        Selector::Function {
            item: Definition::authenticated([41; 32]),
        }
    }

    /// A runner's native account whose build was compiled with one use of
    /// a crate, `native::echo::<i64>`: its carrier covers that use alone.
    #[cfg(feature = "rust-import")]
    fn echo_native() -> crate::package::import::runner::account::Native {
        use rawr_import::classify::representability::convert::{Builtin, Type};
        use rawr_import::runner::manifest::{self, coverage};
        let selector = echo();
        let used = coverage::Use {
            path: "native::echo",
            selector: &selector,
            endpoint: None,
            types: &[Type::Builtin(Builtin::I64)],
            constants: &[],
            slots: &[],
            impls: &[],
        };
        let carrier = manifest::carrier_fixture_covering(&[([42; 32], [43; 32], 1)], &[(0, used)]);
        crate::package::import::runner::account::Native::transform(
            manifest::Carrier::transform(&carrier).expect("the fixture's carrier reads"),
        )
    }

    /// [`evaluated`] on `engine` as a runner runs `program`: its compiler
    /// retains [`echo_native`], and the program runs bound to a runtime
    /// whose one adapter is `echo::<i64>`'s, as the runner's build was
    /// compiled.
    #[cfg(feature = "rust-import")]
    fn evaluated_in_runner(
        mut engine: crate::Engine,
        program: &str,
        line: usize,
        codes: &[&str],
    ) -> (Vec<Value>, String) {
        use crate::package::compile::captured::tests::native_facade;
        use rawr_import::classify::representability::convert::{Builtin, Type};
        let probe = Probe::register(&mut engine, (line, 0), codes);
        let engine = engine
            .retain_imported_native(echo_native())
            .expect("a fresh compiler retains a native surface");
        let snapshot = Snapshot::new(FILE.into(), vec![(FILE.into(), program.into())]).unwrap();
        let (candidate, ast) = engine
            .compile_package_snapshot_generation_with_top_level(&snapshot, None)
            .unwrap_or_else(|error| panic!("{error}"));
        let located = Arc::clone(
            candidate
                .located()
                .expect("a run compiled under a debugger is located"),
        );
        let lines = Arc::clone(&located);
        let stops = Stops::of(&ast, &move |position| lines.line(position));
        let program = located.program().expect("a debugger's run keeps its files");
        *probe.shared.lock().unwrap() = Some((program, stops));
        let mut runtime = crate::host::runtime::Builder::new(native_facade());
        let instance = rawr_import::surface::instance::key_with_items(
            &[Type::Builtin(Builtin::I64)],
            &[],
            &[],
            &[],
        );
        runtime
            .bind_imported_instance(echo().fingerprint(), instance, "native::echo", |_, args| {
                Ok(args[0].take())
            })
            .unwrap();
        let mut registry = crate::host::runtime::Registry::new();
        registry.install(runtime.finish()).unwrap();
        let bootstrap = candidate
            .bind(&engine, registry)
            .unwrap_or_else(|error| panic!("{error}"));
        let _ = bootstrap
            .invoke_main()
            .expect("the program has a `fn main`")
            .unwrap_or_else(|error| panic!("{error}"));
        probe.finish()
    }

    /// A runner's program that echoes a count and prints it, paused at
    /// line 4, before it prints.
    #[cfg(feature = "rust-import")]
    const ECHOED: &str = "// rawr
fn main() {
    let mut count: i64 = native::echo(3);
    println!(\"{count}\");
}
";

    /// Code evaluated in a generation bound to a runner's native runtime
    /// calls a crate's item through the adapters that generation's calls
    /// reach (#2264).
    #[cfg(feature = "rust-import")]
    #[test]
    fn code_calls_a_crates_item_through_the_runners_bindings() {
        let (answers, printed) = evaluated_in_runner(
            crate::package::compile::captured::tests::compiler(),
            ECHOED,
            4,
            &["native::echo(count) + 1"],
        );
        assert_eq!(answers, [json!({ "value": "4" })]);
        assert_eq!(printed, "3\n");
    }

    /// The error of code refused before it ran, which names the use the
    /// running build lacks, and what the program printed after it.
    #[cfg(feature = "rust-import")]
    fn refused(answers: &[Value], printed: &str, named: &str) {
        let [answer] = answers else {
            panic!("one code answers once: {answers:?}");
        };
        let error = answer["error"]
            .as_str()
            .unwrap_or_else(|| panic!("the code is refused: {answer}"));
        assert!(
            error.contains(&format!("the running build has no binding for `{named}`")),
            "{error}"
        );
        assert!(
            error.contains("saving it rebuilds the runner with it"),
            "{error}"
        );
        assert!(!error.contains("not supported in Rawr yet"), "{error}");
        // The code's first statement didn't run: the count is the one the
        // program echoed.
        assert_eq!(printed, "3\n");
    }

    /// A field the program never reads has no adapter in the running
    /// build: code that reads it is refused before it runs, naming the
    /// read, not as a read rawr can't make (#2406).
    #[cfg(feature = "rust-import")]
    #[test]
    fn code_reading_a_field_the_program_never_reads_is_refused_before_it_runs() {
        let mut engine = crate::package::compile::captured::tests::compiler();
        crate::package::compile::captured::native_tests::structure(&mut engine);
        let (answers, printed) = evaluated_in_runner(
            engine,
            ECHOED,
            4,
            &["count += 1; let read = |cell: structure::Cell| cell.0;"],
        );
        refused(&answers, &printed, "cell.0");
    }

    /// A named field read through a local is refused naming the whole
    /// read, though a field's name carries the span of its first
    /// character (#2406).
    #[cfg(feature = "rust-import")]
    #[test]
    fn a_refused_field_read_names_the_whole_read() {
        let mut engine = crate::package::compile::captured::tests::compiler();
        crate::package::compile::captured::native_tests::structure(&mut engine);
        let (answers, printed) = evaluated_in_runner(
            engine,
            ECHOED,
            4,
            &["count += 1; \
               let read = |held: structure::Meter| { let meter = held; meter.count * 10 };"],
        );
        refused(&answers, &printed, "meter.count");
        let error = answers[0]["error"].as_str().unwrap();
        assert_eq!(
            error.lines().next(),
            Some("error: the running build has no binding for `meter.count`"),
            "{error}"
        );
    }

    /// A crate's generic function at a type the running build has no
    /// instance of is refused before any of the code runs, not when the
    /// call is reached (#2406).
    #[cfg(feature = "rust-import")]
    #[test]
    fn code_calling_a_generic_at_a_type_the_build_lacks_is_refused_before_it_runs() {
        let (answers, printed) = evaluated_in_runner(
            crate::package::compile::captured::tests::compiler(),
            ECHOED,
            4,
            &["count += 1; native::echo(true)"],
        );
        refused(&answers, &printed, "native::echo(true)");
    }
}
