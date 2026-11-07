//! One aggregate package compiler shared by the library, the CLI and hosts.

#[cfg(feature = "rust-import")]
pub(crate) mod captured;

use crate::{program, project, resolve, timing, Engine, Scope};

use super::{Error, Graph};

type ExecutableEvidence = (
    program::Executable,
    Box<[crate::diagnostic::Source]>,
    crate::package::source::Id,
    Option<crate::reload::source::Graph>,
);

pub(crate) fn executable(
    engine: &Engine,
    target: &project::Target,
    session: Option<&timing::Session>,
) -> Result<program::Executable, Error> {
    executable_with_evidence(engine, target, session, false).map(|(executable, ..)| executable)
}

/// One in-memory file, `text` named `identity`, compiled as the root of a crate, when its `mod`
/// items are all inline (`mod geometry { .. }`): each module is read from
/// the file's own text, as `rawr run` reads it, without a file or a
/// project. `None` when the file declares no module, or a file module
/// (`mod util;`), which the single-file compiler refuses.
///
/// The program is the single-file compiler's
/// ([`Executable::into_one_file`](program::Executable::into_one_file)).
pub(crate) fn inline_modules(
    engine: &Engine,
    unit: &crate::parse::Unit,
    identity: &str,
    text: &str,
) -> Option<crate::parser::ParseResult<program::Executable>> {
    let mut inline = false;
    for item in unit.items() {
        if let crate::item::Parsed::Module(module) = item {
            module.inline?;
            inline = true;
        }
    }
    if !inline {
        return None;
    }
    let path = std::path::Path::new("/").join(identity);
    let snapshot = super::snapshot::Snapshot::new(path.clone(), vec![(path, text.into())]).ok()?;
    let compiled = Graph::load_snapshot(engine, &snapshot)
        .and_then(|graph| executable_from_graph(engine, graph, None, false))
        .map_err(Error::into_file_error);
    Some(compiled.map(|(executable, ..)| executable.into_one_file()))
}

#[cfg(feature = "rust-import")]
pub(crate) fn snapshot(
    engine: &Engine,
    snapshot: &super::snapshot::Snapshot,
) -> Result<crate::AST, Error> {
    let graph = Graph::load_snapshot(engine, snapshot)?;
    let sources = graph.diagnostic_sources();
    let program = resolve::package::resolve(graph, &engine.host_registry)
        .map_err(|failure| Error::resolution(sources.clone(), failure))?;
    snapshot_executable(engine, program, &sources).map(program::Executable::into_hosted_ast)
}

/// A generation of any program, a plain one included, for a runner with no
/// host profile.
#[cfg(feature = "rust-import")]
pub(crate) fn snapshot_generation_plain(
    engine: &Engine,
    snapshot: &super::snapshot::Snapshot,
) -> Result<crate::reload::Candidate, Error> {
    let graph = Graph::load_snapshot(engine, snapshot)?;
    let sources = graph.diagnostic_sources();
    let source = crate::reload::source::Graph::from_package(&graph);
    let program = resolve::package::resolve(graph, &engine.host_registry)
        .map_err(|failure| Error::resolution(sources.clone(), failure))?;
    let executable = snapshot_executable(engine, program, &sources)?;
    Ok(crate::reload::Candidate::from_package_plain(
        engine, executable, source,
    ))
}

#[cfg(feature = "rust-import")]
fn snapshot_executable(
    engine: &Engine,
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
    session: Option<&timing::Session>,
    retain_source: bool,
) -> Result<ExecutableEvidence, Error> {
    executable_from_graph(
        engine,
        load(engine, target, session)?,
        session,
        retain_source,
    )
}

/// Load (parse) a target's graph, observed as [`timing::Phase::ParseItems`].
pub(crate) fn load(
    engine: &Engine,
    target: &project::Target,
    session: Option<&timing::Session>,
) -> Result<Graph, Error> {
    timing::observe_optional(session, timing::Phase::ParseItems, || {
        Graph::load(engine, target)
    })
}

/// [`executable_with_evidence`] of a graph already loaded.
pub(crate) fn executable_from_graph(
    engine: &Engine,
    graph: Graph,
    session: Option<&timing::Session>,
    retain_source: bool,
) -> Result<ExecutableEvidence, Error> {
    let sources = graph.diagnostic_sources();
    let entry = graph.entry_source().id();
    let source = retain_source.then(|| crate::reload::source::Graph::from_package(&graph));
    let program = timing::observe_optional(session, timing::Phase::Resolve, || {
        resolve::package::resolve(graph, &engine.host_registry)
            .map_err(|failure| Error::resolution(sources.clone(), failure))
    })?;
    let executable = timing::observe_optional(session, timing::Phase::Lower, || {
        resolve::lower_package_with_engine(
            engine,
            program,
            Some(&Scope::new()),
            engine.optimization_level,
        )
        .map_err(|failure| {
            let (evidence, error) = failure.into_parts();
            Error::compilation(sources.clone(), evidence, error)
        })
    })?;
    Ok((executable, sources, entry, source))
}
