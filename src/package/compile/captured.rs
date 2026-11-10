//! Executable and native-use evidence from the same immutable snapshot pass.

#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
mod native_tests;

use crate::{diagnostic, host, package, reload, resolve, Engine, Scope, Span};
use rawr_import::runner::manifest::{self, coverage};
use sha2::{Digest as _, Sha256};

/// Unbound compiler output. Native coverage and live-owner admission are still
/// required before this candidate can enter a resident reload transaction.
pub(crate) struct Output {
    pub(crate) candidate: reload::Candidate,
    pub(crate) imported: resolve::lower_nominal::ImportedOutput,
    /// The snapshot's sources, which a use the carrier misses is reported
    /// against.
    sources: Box<[diagnostic::Source]>,
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
    /// The first use the carrier doesn't cover.
    Uncovered(Box<Unbound>),
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
    /// Compile any program, a plain one included.
    pub(crate) fn transform_plain<A: host::ImportAllocation>(
        engine: &Engine,
        snapshot: &package::snapshot::Snapshot,
        allocation: &mut A,
    ) -> Result<Self, Error> {
        let graph = engine.load_snapshot_kept(snapshot)?;
        let sources = graph.diagnostic_sources();
        let source = reload::source::Graph::from_package(&graph);
        let native = resolve::native::Capture::from_graph(&graph, allocation)?;
        let program =
            resolve::package::resolve_captured(graph, &engine.host_registry, native.clone())
                .map_err(|failure| {
                    // A crate derive whose item changed is expanded again
                    // by preparation, as a native miss is answered (#593).
                    let missed = matches!(failure.error, resolve::Error::ExpansionRequired { .. })
                        || native.matches(
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
                let (evidence, mut error) = failure.into_parts();
                // A miss a proof could settle is still the miss here: this
                // compilation proves nothing, so the host's full preparation
                // does.
                #[cfg(feature = "rust-import")]
                if let crate::ParseErrorType::Resolution(
                    resolve::Error::ImportedApplicationRequired {
                        miss: Some(miss), ..
                    },
                ) = error.err_type()
                {
                    let miss = crate::ParseErrorType::Resolution((**miss).clone());
                    error = crate::ParseError(Box::new(miss), error.position(), error.2.clone());
                }
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
        let candidate = reload::Candidate::from_package_plain(engine, executable, source);
        Ok(Self {
            candidate,
            imported,
            sources,
        })
    }

    /// Consume the actual compiled output only after every captured native use
    /// matches the same carrier. Runtime projections remain in its candidate
    /// for the normal off-live binding gate.
    pub(crate) fn cover(self, carrier: &manifest::Carrier) -> Coverage {
        let mut digest = Sha256::new();
        digest.update(b"rawr.resident.compiled-uses/v1");
        digest.update(carrier.identity());
        digest.update(carrier.coverage_digest());
        // The script functions each adapter holds: one adapter holds one
        // function at each slot, so two uses that pass two need two.
        let mut held = std::collections::BTreeMap::new();
        for used in &self.imported.uses {
            // A dependency crate's item from that crate's root, as the
            // carrier names it: `iced::__rawr_crate_iced_widget_<id>::Column`
            // is `iced_widget::Column`.
            let path = host::path::public(&used.path);
            let matched = carrier.cover(
                coverage::Use {
                    path: &*path,
                    selector: &used.selector,
                    endpoint: None,
                    types: &used.types,
                    constants: &used.constants,
                    slots: &used.slots,
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
            let shared = matched
                .as_ref()
                .and_then(coverage::Matched::entry)
                .filter(|_| !used.fillers.is_empty())
                .is_some_and(|entry| {
                    **held.entry(*entry.identity()).or_insert(&*used.fillers) != *used.fillers
                });
            let Some(matched) = matched.filter(|_| !shared) else {
                return Coverage::Uncovered(Box::new(Unbound {
                    at: self
                        .sources
                        .get(used.source.index())
                        .map(|source| (source.clone(), used.span)),
                    miss: None,
                }));
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

/// A use of a crate this compiler has no binding for: a lookup with native
/// provenance that missed, or a use at types the carrier doesn't cover.
/// Only a universe runner reports it (`rawr_import::universe`), which binds
/// what its own program uses and prepares nothing more, as the program's
/// error where the program makes the use. Any other runner prepares it.
pub(crate) struct Unbound {
    at: Option<(diagnostic::Source, Span)>,
    /// The lookup's own error, reported when it has no position.
    miss: Option<package::Error>,
}

impl Unbound {
    /// A lookup with native provenance that missed, at the position its
    /// own error reports.
    pub(crate) fn missed(error: package::Error) -> Self {
        let at = error.report().ok().and_then(|report| {
            report
                .labels()
                .iter()
                .find(|label| label.kind() == diagnostic::LabelKind::Primary)
                .map(|label| (label.source().clone(), label.span()))
        });
        Self {
            at,
            miss: Some(error),
        }
    }

    /// The program's error at the use.
    pub(crate) fn error(self) -> package::Error {
        let Some((source, span)) = self.at else {
            return self.miss.unwrap_or_else(|| {
                package::Error::diagnostic(diagnostic::Error::EmptySourceIdentity)
            });
        };
        let report = diagnostic::Report::error(
            diagnostic::code::RESOLVE_HOST_OPERATION,
            "this use of a crate is not available in this prebuilt runner",
        )
        .primary(
            source,
            span,
            Some("the runner was not built with this call".into()),
        )
        .map(diagnostic::Builder::finish);
        match report {
            Ok(report) => package::Error::import_diagnostic(report),
            Err(error) => package::Error::diagnostic(error),
        }
    }
}

impl Matched {
    pub(crate) fn into_parts(self) -> (Output, [u8; 32], [u8; 32]) {
        (self.output, self.carrier, self.coverage)
    }
}
