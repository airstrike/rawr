//! Executable and native-use evidence from the same immutable snapshot pass.

#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
mod native_tests;

use crate::{application, host, package, reload, resolve, Engine, Scope};
use rawr_import::runner::manifest::{self, coverage};
use sha2::{Digest as _, Sha256};

/// Unbound compiler output. Native coverage and live-owner admission are still
/// required before this candidate can enter a resident reload transaction.
pub(crate) struct Output {
    pub(crate) candidate: reload::Candidate,
    pub(crate) semantic: Option<application::SemanticFingerprint>,
    pub(crate) imported: resolve::lower_nominal::ImportedOutput,
}

/// The intact compiler output joined to exact native inventory metadata.
/// This does not establish live native ownership or runtime binding.
pub(crate) struct Matched {
    output: Output,
    carrier: [u8; 32],
    coverage: [u8; 32],
}

pub(crate) enum Coverage {
    Matched(Box<Matched>),
    NeedsPreparation,
}

/// Only a terminal lookup with native provenance may request preparation.
/// Source diagnostics and allocation refusals keep their original meanings.
#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Source(#[from] package::Error),
    #[error(transparent)]
    NativeMiss(package::Error),
    #[error(transparent)]
    Resource(#[from] rawr_import::surface::Error),
}

impl Output {
    pub(crate) fn transform<A: host::ImportAllocation>(
        engine: &Engine,
        snapshot: &package::snapshot::Snapshot,
        cycle: &host::facade::ProfileCycle,
        allocation: &mut A,
    ) -> Result<Self, Error> {
        let graph = package::Graph::load_snapshot(engine, snapshot)?;
        let sources = graph.diagnostic_sources();
        let source = reload::source::Graph::from_package(&graph);
        let native = resolve::native::Capture::from_graph(&graph, allocation)?;
        let program =
            resolve::package::resolve_captured(graph, &engine.host_registry, native.clone())
                .map_err(|failure| {
                    let missed = native.matches(
                        crate::program::source::Id::from_index(failure.primary.index()),
                        &failure.error,
                    );
                    let error = package::Error::resolution(sources.clone(), failure);
                    if missed {
                        Error::NativeMiss(error)
                    } else {
                        Error::Source(error)
                    }
                })?;
        let (executable, imported) = resolve::lower_package_with_uses(
            engine,
            program,
            allocation,
            Some(&Scope::new()),
            engine.optimization_level,
        )
        .map_err(|failure| match failure {
            resolve::ImportedFailure::Surface(error) => Error::Resource(error),
            resolve::ImportedFailure::Lowering(failure) => {
                let (evidence, error) = failure.into_parts();
                let missed = match (&evidence, error.err_type()) {
                    (
                        resolve::LoweringEvidence::Original(source),
                        crate::ParseErrorType::Resolution(error),
                    ) => native.matches(*source, error),
                    _ => false,
                };
                let error = package::Error::compilation(sources.clone(), evidence, error);
                if missed {
                    Error::NativeMiss(error)
                } else {
                    Error::Source(error)
                }
            }
        })?;
        let (candidate, semantic) = reload::Candidate::from_package_derived(
            executable, source, cycle,
        )
        .map_err(|(source, error)| {
            package::Error::resolution(
                sources,
                resolve::package::Failure {
                    primary: package::source::Id::from_index(source.index()),
                    secondary: None,
                    error: resolve::Error::from(error),
                },
            )
        })?;
        Ok(Self {
            candidate,
            semantic,
            imported,
        })
    }

    /// Consume the actual compiled output only after every captured native use
    /// matches the same carrier. Runtime projections and endpoint contracts
    /// remain in its candidate for the normal off-live binding gate.
    pub(crate) fn cover(self, carrier: &manifest::Carrier) -> Coverage {
        let mut digest = Sha256::new();
        digest.update(b"rawr.resident.compiled-uses/v1");
        digest.update(carrier.identity());
        digest.update(carrier.coverage_digest());
        for used in &self.imported.uses {
            let matched = carrier.cover(
                coverage::Use {
                    path: &used.path,
                    selector: &used.selector,
                    endpoint: used.endpoint.as_ref(),
                    types: &used.types,
                },
                used.functions.iter().map(|function| coverage::Function {
                    position: function.position,
                    path: &*function.path,
                    selector: &function.selector,
                }),
                used.mappers.iter().map(|mapper| coverage::Mapper {
                    position: mapper.position,
                    seal: &mapper.seal,
                }),
            );
            let Some(matched) = matched else {
                return Coverage::NeedsPreparation;
            };
            digest.update(matched.digest());
        }
        Coverage::Matched(Box::new(Matched {
            output: self,
            carrier: *carrier.identity(),
            coverage: digest.finalize().into(),
        }))
    }
}

impl Matched {
    pub(crate) fn into_parts(self) -> (Output, [u8; 32], [u8; 32]) {
        (self.output, self.carrier, self.coverage)
    }
}
