//! One aggregate package compiler shared by library, CLI, and iced hosts.

#[cfg(all(not(feature = "no_function"), feature = "rust-import"))]
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

pub(crate) fn descriptor(
    engine: &Engine,
    target: &project::Target,
    session: Option<&timing::Session>,
) -> Result<crate::application::Descriptor, Error> {
    descriptor_from_graph(engine, load(engine, target, session)?, session)
}

/// [`descriptor`] of a graph already loaded.
pub(crate) fn descriptor_from_graph(
    engine: &Engine,
    graph: Graph,
    session: Option<&timing::Session>,
) -> Result<crate::application::Descriptor, Error> {
    let (executable, sources, entry, _) = executable_from_graph(engine, graph, session, false)?;
    timing::observe_optional(session, timing::Phase::ApplicationContract, || {
        executable
            .require_application()
            .map_err(|error| (program::source::Id::from_index(entry.index()), error))?
            .validate_package()
    })
    .map_err(|(source, error)| {
        Error::resolution(
            sources,
            resolve::package::Failure {
                primary: crate::package::source::Id::from_index(source.index()),
                secondary: None,
                error: resolve::Error::from(error),
            },
        )
    })
}

pub(crate) fn catalog(
    engine: &Engine,
    target: &project::Target,
    session: Option<&timing::Session>,
) -> Result<crate::preview::Catalog, Error> {
    let (executable, sources, _, _) = executable_with_evidence(engine, target, session, false)?;
    timing::observe_optional(session, timing::Phase::ApplicationContract, || {
        crate::preview::Catalog::from_package_executable(executable)
    })
    .map_err(|(source, error)| {
        Error::resolution(
            sources,
            resolve::package::Failure {
                primary: crate::package::source::Id::from_index(source.index()),
                secondary: None,
                error: resolve::Error::from(error),
            },
        )
    })
}

pub(crate) fn generation(
    engine: &Engine,
    target: &project::Target,
    session: Option<&timing::Session>,
) -> Result<crate::reload::Candidate, Error> {
    generation_from_graph(engine, load(engine, target, session)?, session)
}

/// [`generation`] of a graph already loaded.
pub(crate) fn generation_from_graph(
    engine: &Engine,
    graph: Graph,
    session: Option<&timing::Session>,
) -> Result<crate::reload::Candidate, Error> {
    let (executable, sources, _, source) = executable_from_graph(engine, graph, session, true)?;
    let source = source.expect("reload compilation retains its canonical source graph");
    crate::reload::Candidate::from_package(executable, source).map_err(|(source, error)| {
        Error::resolution(
            sources,
            resolve::package::Failure {
                primary: crate::package::source::Id::from_index(source.index()),
                secondary: None,
                error: resolve::Error::from(error),
            },
        )
    })
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
    snapshot_executable(engine, program, &sources).map(program::Executable::into_ast)
}

#[cfg(feature = "rust-import")]
pub(crate) fn snapshot_generation(
    engine: &Engine,
    snapshot: &super::snapshot::Snapshot,
) -> Result<crate::reload::Candidate, Error> {
    snapshot_generation_with_semantics(engine, snapshot, None)
}

#[cfg(feature = "rust-import")]
pub(crate) fn snapshot_generation_checked(
    engine: &Engine,
    snapshot: &super::snapshot::Snapshot,
    cycle: &crate::host::facade::ProfileCycle,
    expected: &[u8; 32],
) -> Result<crate::reload::Candidate, Error> {
    snapshot_generation_with_semantics(engine, snapshot, Some((cycle, expected)))
}

#[cfg(feature = "rust-import")]
fn snapshot_generation_with_semantics(
    engine: &Engine,
    snapshot: &super::snapshot::Snapshot,
    semantic: Option<(&crate::host::facade::ProfileCycle, &[u8; 32])>,
) -> Result<crate::reload::Candidate, Error> {
    let graph = Graph::load_snapshot(engine, snapshot)?;
    let sources = graph.diagnostic_sources();
    let source = crate::reload::source::Graph::from_package(&graph);
    let program = resolve::package::resolve(graph, &engine.host_registry)
        .map_err(|failure| Error::resolution(sources.clone(), failure))?;
    let executable = snapshot_executable(engine, program, &sources)?;
    let candidate = match semantic {
        Some((cycle, expected)) => {
            crate::reload::Candidate::from_package_checked(executable, source, cycle, expected)
        }
        None => crate::reload::Candidate::from_package(executable, source),
    };
    candidate.map_err(|(source, error)| {
        Error::resolution(
            sources,
            resolve::package::Failure {
                primary: crate::package::source::Id::from_index(source.index()),
                secondary: None,
                error: resolve::Error::from(error),
            },
        )
    })
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
    #[cfg(not(feature = "no_function"))]
    if let Some(native) = &engine.imported_native {
        let resource = |error| Error::import(super::import::Error::Surface(error));
        let mut attempt = super::import::runner::account::support::Pending::transform(
            native.attempt().map_err(resource)?,
        )
        .map_err(resource)?;
        let (mut executable, _uses) = resolve::lower_package_with_uses(
            engine,
            program,
            &mut attempt,
            Some(&Scope::new()),
            #[cfg(not(feature = "no_optimize"))]
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
        #[cfg(not(feature = "no_optimize"))]
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
            #[cfg(not(feature = "no_optimize"))]
            engine.optimization_level,
        )
        .map_err(|failure| {
            let (evidence, error) = failure.into_parts();
            Error::compilation(sources.clone(), evidence, error)
        })
    })?;
    Ok((executable, sources, entry, source))
}
