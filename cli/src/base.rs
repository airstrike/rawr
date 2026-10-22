//! The base every program gets: the language and its core intrinsics
//! (`format!`, `println!`, ..). Installed layers (rawr-iced today) add a
//! domain on top when a program brings them in; [`layer`] is the one place
//! that asks them, from the source alone, before any layer's work. A program
//! with nothing layered on runs once here: its top level, then `fn main` if
//! there is one, as in Rust.

use std::path::PathBuf;

use rawr::{diagnostic, project, CallFnOptions, Dynamic, Engine, Scope};

/// The layers this binary carries. The one place a feature flag, or finding
/// a layer's separate binary, would change.
const INSTALLED: &[rawr::layer::Layer] = &[rawr_iced::LAYER];

/// What runs a program: the base alone, or the base with one layer on top.
pub(crate) enum Program {
    /// The base alone, with the package graph its decision loaded.
    Base(project::Target, rawr::package::Graph),
    Layered(rawr::layer::Layer),
}

/// Decide, before any layer's work, what runs the program at `path`: each
/// installed layer judges the facts of the one package-graph load (the
/// parse the base compiles from). A target that cannot be discovered or
/// parsed, or brings in two layers, is a diagnostic.
pub(crate) fn layer(path: Option<&PathBuf>) -> Result<Program, diagnostic::Report> {
    let target = match path {
        Some(path) => project::Target::discover(path),
        None => project::Target::current(),
    }
    .map_err(|error| diagnostic::Report::from_project(&error))?;
    let graph = rawr::package::Graph::load(&Engine::new(), &target)
        .map_err(|error| compile_report(&error, &target))?;
    let facts = rawr::layer::Facts::of(&graph);
    let mut claimed = INSTALLED.iter().filter(|layer| (layer.claims)(&facts));
    match (claimed.next(), claimed.next()) {
        (None, _) => Ok(Program::Base(target, graph)),
        (Some(layer), None) => Ok(Program::Layered(*layer)),
        (Some(first), Some(second)) => Err(diagnostic::Report::error(
            diagnostic::code::FRAMEWORK_FACADE,
            format!(
                "this program brings in both {} and {}; a program takes one layer",
                first.name, second.name
            ),
        )
        .help("use the crates of only one of them")
        .finish()),
    }
}

/// Compile a base program; the source path checked.
pub(crate) fn check(
    target: &project::Target,
    graph: rawr::package::Graph,
) -> Result<PathBuf, diagnostic::Report> {
    Engine::new()
        .compile_package(graph)
        .map_err(|error| compile_report(&error, target))?;
    Ok(target.source().path().to_path_buf())
}

/// Run a base program once. `Ok(Some(text))` is a `main` that returned
/// `Err`: Rust's `Error: {err:?}`, for stderr and a failing exit.
pub(crate) fn run(
    target: &project::Target,
    graph: rawr::package::Graph,
) -> Result<Option<String>, diagnostic::Report> {
    let engine = Engine::new();
    let (ast, main) = engine
        .compile_package(graph)
        .map_err(|error| compile_report(&error, target))?;
    let runtime = |error: Box<rawr::EvalAltResult>| {
        let source = source(target);
        diagnostic::Report::from_evaluation(source.clone(), &error).unwrap_or_else(|_| {
            crate::watch::report(diagnostic::code::ENVIRONMENT_DIAGNOSTIC, error, source)
        })
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
        crate::watch::report(
            diagnostic::code::ENVIRONMENT_DIAGNOSTIC,
            format!("cannot retain exact package diagnostic evidence: {report_error}"),
            source(target),
        )
    })
}
