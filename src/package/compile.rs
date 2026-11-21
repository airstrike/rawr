//! One aggregate package compiler shared by the library, the CLI and hosts.

#[cfg(feature = "rust-import")]
pub(crate) mod captured;

use crate::{program, project, resolve, Engine, Scope};

use super::{Error, Graph};

/// What a compile of a snapshot reuses of earlier compiles.
#[cfg(feature = "rust-import")]
pub(crate) enum Reuse<'a> {
    /// Nothing: each source is parsed and lowered.
    Fresh,
    /// A runner's cache, which the compile reads and keeps its own work in.
    Kept(&'a mut super::import::resident::Cache),
    /// The parses a generation compiled from, read and never written:
    /// what code a debugger evaluates in it compiles from.
    Read(&'a super::Parses),
}

type ExecutableEvidence = (
    program::Executable,
    Box<[crate::diagnostic::Source]>,
    crate::package::source::Id,
    Option<crate::reload::source::Graph>,
);

pub(crate) fn executable(
    engine: &Engine,
    target: &project::Target,
    answers: &mut project::cargo::Answers,
) -> Result<program::Executable, Error> {
    executable_with_evidence(engine, target, false, answers).map(|(executable, ..)| executable)
}

/// One in-memory file, `text` named `identity` and already parsed as
/// `unit`, compiled as the root of a crate: each inline module (`mod
/// geometry { .. }`) is read from the file's own text, as `rawr run` reads
/// it, without a file or a project. A file module (`mod util;`) needs the
/// project, so the single-file compiler refuses it.
///
/// The program is the single-file compiler's
/// ([`Executable::into_one_file`](program::Executable::into_one_file)).
pub(crate) fn one_file(
    engine: &Engine,
    unit: crate::parse::Unit,
    (identity, text): (&str, &str),
    (scope, optimization_level): (Option<&Scope>, crate::OptimizationLevel),
) -> crate::parser::ParseResult<program::Executable> {
    for item in unit.items() {
        if let crate::item::Parsed::Module(module) = item
            && module.inline.is_none()
        {
            return Err(resolve::Error::UnsupportedNominalSyntax {
                construct: "file modules require project/package compilation".into(),
                source: module.span,
            }
            .into());
        }
    }
    // A name no path can hold, as `../x`, is the file's name only.
    let snapshot = |identity: &str| {
        let path = std::path::Path::new("/").join(identity);
        super::snapshot::Snapshot::new(path.clone(), vec![(path, text.into())])
    };
    let snapshot = snapshot(identity)
        .or_else(|_| snapshot("main.rs"))
        .expect("one absolute file is a valid snapshot");
    // A file with no manifest asks Cargo nothing.
    let compiled = Graph::load_parsed(engine, &snapshot, unit, &mut project::cargo::Answers::new())
        .and_then(|graph| lower_graph(engine, graph, false, (scope, optimization_level)))
        .map_err(Error::into_file_error);
    compiled.map(|(executable, ..)| executable.into_one_file())
}

#[cfg(feature = "rust-import")]
pub(crate) fn snapshot(
    engine: &Engine,
    snapshot: &super::snapshot::Snapshot,
) -> Result<crate::AST, Error> {
    snapshot_program(engine, snapshot, Reuse::Fresh, |_| ())
        .map(|(executable, ())| executable.into_hosted_ast())
}

/// The program `snapshot` holds, compiled as a runner compiles its start
/// ([`snapshot_generation_plain`]), with the [`program::Positions`] that
/// place its steps in its files: code a debugger evaluates checks in the
/// program as the run checked it (#2292). On a compiler that retains a
/// native surface, that is a save's covered compile, so a use the
/// runner's build lacks is refused where it is written (#2406). A source
/// whose text `parses` holds the parse of is read from it; nothing the
/// compile does is kept.
#[cfg(feature = "rust-import")]
pub(crate) fn snapshot_with_positions(
    engine: &Engine,
    snapshot: &super::snapshot::Snapshot,
    parses: Option<&super::Parses>,
) -> Result<(crate::AST, program::Positions), Error> {
    if let Some(native) = &engine.imported_native {
        return captured::positioned(engine, native, snapshot, parses);
    }
    let reuse = parses.map_or(Reuse::Fresh, Reuse::Read);
    snapshot_program(engine, snapshot, reuse, |_| ())
        .map(|(executable, ())| executable.into_ast_with_positions())
}

/// A generation of any program, a plain one included, for a runner with no
/// host profile. A compile lent `cache` reads and keeps there.
#[cfg(feature = "rust-import")]
pub(crate) fn snapshot_generation_plain(
    engine: &Engine,
    snapshot: &super::snapshot::Snapshot,
    mut cache: Option<&mut super::import::resident::Cache>,
) -> Result<crate::reload::Candidate, Error> {
    let reuse = cache.as_deref_mut().map_or(Reuse::Fresh, Reuse::Kept);
    let (executable, (source, declared)) = snapshot_program(engine, snapshot, reuse, |graph| {
        // A crate built for its tests runs them in place of its `main`.
        let declared = graph
            .entry_package()
            .features()
            .test()
            .then(|| super::test::Declared::of(graph));
        (crate::reload::source::Graph::from_package(graph), declared)
    })?;
    let tests = declared.map(|declared| super::test::Declared::compiled_in(declared, &executable));
    let parses = cache.and_then(|cache| cache.parses());
    Ok(
        crate::reload::Candidate::from_package_plain(executable, source, engine, snapshot, parses)
            .with_tests(tests),
    )
}

/// The program `snapshot` holds, lowered as a runner lowers it: the
/// dependencies' macro calls expanded, and on a compiler that retains a
/// native surface, lowered against it, which reveals each opaque return
/// type (`fn view() -> impl Widget`) before a crate's generic call takes
/// it. `read` takes what it needs of the loaded graph first.
#[cfg(feature = "rust-import")]
fn snapshot_program<T>(
    engine: &Engine,
    snapshot: &super::snapshot::Snapshot,
    reuse: Reuse<'_>,
    read: impl FnOnce(&Graph) -> T,
) -> Result<(program::Executable, T), Error> {
    // A compile lent no cache keeps what Cargo answers for itself alone.
    let mut answers = project::cargo::Answers::new();
    let (mut graph, cache) = match reuse {
        Reuse::Fresh => (Graph::load_snapshot(engine, snapshot, &mut answers)?, None),
        Reuse::Kept(cache) => (cache.load_snapshot(engine, snapshot)?, Some(cache)),
        Reuse::Read(parses) => (
            Graph::load_snapshot_reading(engine, snapshot, parses, &mut answers)?,
            None,
        ),
    };
    let expanded = graph.expand_crate_macros(&engine.host_registry);
    let sources = graph.diagnostic_sources();
    let read = read(&graph);
    let program = expanded
        .and_then(|()| resolve::package::resolve(graph, &engine.host_registry))
        .map_err(|failure| Error::resolution(sources.clone(), failure))?;
    let kept = cache.map(super::import::resident::Cache::lowerings);
    snapshot_executable(engine, kept, program, &sources).map(|executable| (executable, read))
}

#[cfg(feature = "rust-import")]
fn snapshot_executable(
    engine: &Engine,
    kept: Option<&mut resolve::lower_nominal::Lowerings>,
    program: resolve::Program,
    sources: &[crate::diagnostic::Source],
) -> Result<program::Executable, Error> {
    // Initial, neutral and ordinary checked outputs overlap resident candidates
    // too. Their executable and captured support share this one contribution;
    // converting a neutral executable into its AST must retain live support.
    if let Some(native) = &engine.imported_native {
        let resource = |error| Error::import(super::import::Error::Surface(error));
        let mut attempt = super::import::runner::account::support::Pending::transform(
            native.attempt().map_err(resource)?,
        );
        let (mut executable, _uses) = resolve::lower_package_with_uses(
            engine,
            kept,
            program,
            &mut attempt,
            Some(&Scope::new()),
            engine.optimization_level,
        )
        .map_err(|failure| match failure {
            resolve::ImportedFailure::Surface(error) => resource(error),
            resolve::ImportedFailure::Lowering(failure) => {
                let (evidence, error) = failure.into_parts();
                Error::compilation(sources.into(), evidence, error)
            }
        })?;
        executable.retain_imported(attempt.finish());
        return Ok(executable);
    }
    resolve::lower_package_with_engine(
        engine,
        program,
        Some(&Scope::new()),
        engine.optimization_level,
    )
    .map_err(|failure| {
        let (evidence, error) = failure.into_parts();
        Error::compilation(sources.into(), evidence, error)
    })
}

fn executable_with_evidence(
    engine: &Engine,
    target: &project::Target,
    retain_source: bool,
    answers: &mut project::cargo::Answers,
) -> Result<ExecutableEvidence, Error> {
    executable_from_graph(engine, Graph::load(engine, target, answers)?, retain_source)
}

/// [`executable_with_evidence`] of a graph already loaded.
pub(crate) fn executable_from_graph(
    engine: &Engine,
    graph: Graph,
    retain_source: bool,
) -> Result<ExecutableEvidence, Error> {
    lower_graph(
        engine,
        graph,
        retain_source,
        (Some(&Scope::new()), engine.optimization_level),
    )
}

/// [`executable_from_graph`], lowered with `scope`'s constants at
/// `optimization_level`.
fn lower_graph(
    engine: &Engine,
    graph: Graph,
    retain_source: bool,
    (scope, optimization_level): (Option<&Scope>, crate::OptimizationLevel),
) -> Result<ExecutableEvidence, Error> {
    let sources = graph.diagnostic_sources();
    let entry = graph.entry_source().id();
    let source = retain_source.then(|| crate::reload::source::Graph::from_package(&graph));
    let program = resolve::package::resolve(graph, &engine.host_registry)
        .map_err(|failure| Error::resolution(sources.clone(), failure))?;
    let executable = resolve::lower_package_with_engine(engine, program, scope, optimization_level)
        .map_err(|failure| {
            let (evidence, error) = failure.into_parts();
            Error::compilation(sources.clone(), evidence, error)
        })?;
    Ok((executable, sources, entry, source))
}
