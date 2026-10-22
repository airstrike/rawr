//! The base every program gets: the language and its core intrinsics
//! (`format!`, `println!`, ..). Installed layers (rawr-iced today) add a
//! domain on top when a program brings them in; [`layer`] is the one place
//! that asks them, from the source alone, before any layer's work. A program
//! with nothing layered on runs once here: its top level, then `fn main` if
//! there is one, as in Rust.

use std::path::PathBuf;

use rawr::layer::{Facts, Layer, Sheet};
use rawr::{diagnostic, project, CallFnOptions, Dynamic, Engine, Scope};

/// The layers this binary carries. The one place a feature flag, or finding
/// a layer's separate binary, would change.
const INSTALLED: &[Layer] = &[rawr_iced::LAYER];

/// A program as the command loaded it: the one parse every later step of
/// the command uses, its facts, and the command's timing session holding
/// that parse.
pub(crate) struct Loaded {
    pub(crate) target: project::Target,
    pub(crate) graph: rawr::package::Graph,
    pub(crate) sheet: Sheet,
    pub(crate) timing: rawr::timing::Session,
}

/// What runs a program: the base alone, or the base with one layer on top.
pub(crate) enum Program {
    Base(Loaded),
    Layered(Loaded),
}

/// Discover and load the program at `path` once, with its facts.
pub(crate) fn load(path: Option<&PathBuf>) -> Result<Loaded, diagnostic::Report> {
    let target = match path {
        Some(path) => project::Target::discover(path),
        None => project::Target::current(),
    }
    .map_err(|error| diagnostic::Report::from_project(&error))?;
    let timing = rawr::timing::Session::start();
    let graph = timing
        .observe_result(rawr::timing::Phase::ParseItems, || {
            rawr::package::Graph::load(&Engine::new(), &target)
        })
        .map_err(|error| compile_report(&error, &target))?;
    let mut sheet = Sheet::default();
    sheet.update(&graph);
    Ok(Loaded {
        target,
        graph,
        sheet,
        timing,
    })
}

/// Decide, before any layer's work, what runs the program at `path`: each
/// installed layer judges the facts of the one load. A target that cannot
/// be discovered or parsed, or brings in two layers, is a diagnostic.
pub(crate) fn layer(path: Option<&PathBuf>) -> Result<Program, diagnostic::Report> {
    let loaded = load(path)?;
    Ok(match claimed(loaded.sheet.facts())? {
        None => Program::Base(loaded),
        Some(_) => Program::Layered(loaded),
    })
}

/// The installed layer a program with these facts brings in, if any.
fn claimed(facts: &Facts) -> Result<Option<Layer>, diagnostic::Report> {
    let mut claimed = INSTALLED.iter().filter(|layer| (layer.claims)(facts));
    match (claimed.next(), claimed.next()) {
        (None, _) => Ok(None),
        (Some(layer), None) => Ok(Some(*layer)),
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

/// A live session's layer, judged again across reloads: each reload brings
/// the sheet to its new load, and the layers judge the facts again only
/// when they changed. A session cannot change layer in place: a program
/// that no longer brings its layer in (or brings in another) gets a
/// diagnostic asking for a restart, and the running program stays as it
/// was, as for any failed reload.
pub(crate) struct Session {
    layer: Layer,
    sheet: Sheet,
    verdict: Result<(), diagnostic::Report>,
}

impl Session {
    /// A session of `layer` over the program it claimed.
    pub(crate) const fn new(layer: Layer, sheet: Sheet) -> Self {
        Self {
            layer,
            sheet,
            verdict: Ok(()),
        }
    }

    /// Whether the session's layer still runs this reload's load.
    pub(crate) fn judge(&mut self, graph: &rawr::package::Graph) -> Result<(), diagnostic::Report> {
        if self.sheet.update(graph).changed {
            self.verdict = match claimed(self.sheet.facts()) {
                Ok(Some(layer)) if layer.name == self.layer.name => Ok(()),
                Ok(now) => Err(diagnostic::Report::error(
                    diagnostic::code::FRAMEWORK_FACADE,
                    match now {
                        Some(other) => format!(
                            "this program now brings in {} instead of {}",
                            other.name, self.layer.name
                        ),
                        None => format!(
                            "this program no longer brings in {}; it now runs on the base language",
                            self.layer.name
                        ),
                    },
                )
                .help("stop it and run it again with `rawr run`")
                .finish()),
                Err(report) => Err(report),
            };
        }
        self.verdict.clone()
    }
}

/// Compile a base program; the source path checked.
pub(crate) fn check(loaded: Loaded) -> Result<PathBuf, diagnostic::Report> {
    Engine::new()
        .compile_package(loaded.graph)
        .map_err(|error| compile_report(&error, &loaded.target))?;
    Ok(loaded.target.source().path().to_path_buf())
}

/// Run a base program once. `Ok(Some(text))` is a `main` that returned
/// `Err`: Rust's `Error: {err:?}`, for stderr and a failing exit.
pub(crate) fn run(loaded: Loaded) -> Result<Option<String>, diagnostic::Report> {
    let Loaded { target, graph, .. } = loaded;
    let target = &target;
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
