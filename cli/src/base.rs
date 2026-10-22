//! The base every program gets: the language and its core intrinsics
//! (`format!`, `println!`, ..). A program another installed binary serves
//! (rawr-iced today) is handed to it after this one load; see `crate::main`.
//! A program nothing else serves runs once here: its top level, then
//! `fn main` if there is one, as in Rust.

use std::path::PathBuf;

use rawr::layer::{Claims, Sheet};
use rawr::package::Cells;
use rawr::{diagnostic, project, CallFnOptions, Dynamic, Engine, Scope};

/// A program as the command loaded it: the one parse every later step of
/// the command uses, its facts, the parse cells it was loaded through and
/// the command's timing session holding that parse.
pub struct Loaded {
    /// The discovered target.
    pub target: project::Target,
    /// The program's package graph.
    pub graph: rawr::package::Graph,
    /// Its facts, one cell per source.
    pub sheet: Sheet,
    /// The parse cells it was loaded through.
    pub cells: Cells,
    /// The command's timing session.
    pub timing: rawr::timing::Session,
}

/// Discover the program at `path` (the current directory when `None`).
pub fn discover(path: Option<&PathBuf>) -> Result<project::Target, diagnostic::Report> {
    match path {
        Some(path) => project::Target::discover(path),
        None => project::Target::current(),
    }
    .map_err(|error| diagnostic::Report::from_project(&error))
}

/// Discover and load the program at `path` once, with its facts. A source
/// whose parse cell holds its text is read from the cell, not parsed.
pub fn load(path: Option<&PathBuf>) -> Result<Loaded, diagnostic::Report> {
    let target = discover(path)?;
    let cells = Cells::new(crate::cells::directory(&target));
    let timing = rawr::timing::Session::start();
    let graph = timing
        .observe_result(rawr::timing::Phase::ParseItems, || {
            rawr::package::Graph::load_with_cells(&Engine::new(), &target, &cells)
        })
        .map_err(|error| compile_report(&error, &target))?;
    let mut sheet = Sheet::default();
    sheet.update(&graph);
    Ok(Loaded {
        target,
        graph,
        sheet,
        cells,
        timing,
    })
}

/// A live session's program, judged again across reloads: each reload
/// brings the sheet to its new load, and the claims are judged again only
/// when the facts changed. A session cannot hand its program elsewhere in
/// place: a program that no longer uses what the session serves gets a
/// diagnostic asking for a restart, and the running program stays as it
/// was, as for any failed reload.
pub struct Session {
    claims: Claims,
    sheet: Sheet,
    verdict: Result<(), diagnostic::Report>,
}

impl Session {
    /// A session serving the program `claims` claimed.
    #[must_use]
    pub const fn new(claims: Claims, sheet: Sheet) -> Self {
        Self {
            claims,
            sheet,
            verdict: Ok(()),
        }
    }

    /// Whether this session still serves this reload's load.
    pub fn judge(&mut self, graph: &rawr::package::Graph) -> Result<(), diagnostic::Report> {
        if self.sheet.update(graph).changed {
            self.verdict = if self.claims.claim(self.sheet.facts()) {
                Ok(())
            } else {
                Err(diagnostic::Report::error(
                    diagnostic::code::FRAMEWORK_FACADE,
                    format!("this program no longer uses {}", uses(&self.claims)),
                )
                .help("stop it and run it again with `rawr run`")
                .finish())
            };
        }
        self.verdict.clone()
    }
}

/// What `claims` names, as a user reads it: `` `iced` or `[rust]` in rawr.toml ``.
#[must_use]
pub fn uses(claims: &Claims) -> String {
    claims
        .crates
        .iter()
        .map(|name| format!("`{name}`"))
        .chain(
            claims
                .sections
                .iter()
                .map(|section| format!("`[{section}]` in rawr.toml")),
        )
        .collect::<Vec<_>>()
        .join(" or ")
}

/// Compile a base program; the source path checked.
pub fn check(loaded: Loaded) -> Result<PathBuf, diagnostic::Report> {
    Engine::new()
        .compile_package(loaded.graph)
        .map_err(|error| compile_report(&error, &loaded.target))?;
    Ok(loaded.target.source().path().to_path_buf())
}

/// Why a base program did not finish.
pub enum Failed {
    /// It did not compile.
    Compile(diagnostic::Report),
    /// It panicked or failed at run time. Rust exits with 101 for a panic.
    Panic(diagnostic::Report),
}

/// Run a base program once. `Ok(Some(text))` is a `main` that returned
/// `Err`: Rust's `Error: {err:?}`, for stderr and a failing exit.
pub fn run(loaded: Loaded) -> Result<Option<String>, Failed> {
    let Loaded { target, graph, .. } = loaded;
    let target = &target;
    let engine = Engine::new();
    let (ast, main) = engine
        .compile_package(graph)
        .map_err(|error| Failed::Compile(compile_report(&error, target)))?;
    let runtime = |error: Box<rawr::EvalAltResult>| {
        let source = source(target);
        Failed::Panic(
            diagnostic::Report::from_evaluation(source.clone(), &error).unwrap_or_else(|_| {
                report(diagnostic::code::ENVIRONMENT_DIAGNOSTIC, error, source)
            }),
        )
    };
    let mut scope = Scope::new();
    let value = engine
        .eval_ast_with_scope::<Dynamic>(&mut scope, &ast)
        .map_err(runtime)?;
    let Some(main) = main else {
        // A script's final value, as Rust's `{:?}` shows it.
        if !value.is_unit() {
            println!("{value:?}");
        }
        return Ok(None);
    };
    let returned = engine
        .call_fn_with_options::<Dynamic>(
            CallFnOptions::new().eval_ast(false).rewind_scope(false),
            &mut scope,
            &ast,
            main,
            (),
        )
        .map_err(runtime)?;
    Ok(match returned.try_cast::<Result<Dynamic, Dynamic>>() {
        Some(Err(error)) => Some(format!("Error: {error:?}")),
        _ => None,
    })
}

fn source(target: &project::Target) -> Option<diagnostic::Source> {
    diagnostic::Source::new(
        target.source().path().to_string_lossy().into_owned(),
        target.source().text(),
    )
    .ok()
}

fn compile_report(error: &rawr::package::Error, target: &project::Target) -> diagnostic::Report {
    error.report().unwrap_or_else(|report_error| {
        report(
            diagnostic::code::ENVIRONMENT_DIAGNOSTIC,
            format!("cannot retain exact package diagnostic evidence: {report_error}"),
            source(target),
        )
    })
}

fn report(
    code: diagnostic::code::Code,
    message: impl std::fmt::Display,
    source: Option<diagnostic::Source>,
) -> diagnostic::Report {
    let builder = diagnostic::Report::error(code, message.to_string());
    match source {
        Some(source) => builder.source(source).finish(),
        None => builder.finish(),
    }
}
