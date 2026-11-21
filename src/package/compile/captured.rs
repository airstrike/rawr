//! Executable and native-use evidence from the same immutable snapshot pass.

#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
pub(crate) mod native_tests;

use super::Reuse;
use crate::{diagnostic, host, package, program, reload, resolve, Engine, Scope, Span};
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
    /// Compile any program, a plain one included. A compile lent `cache`
    /// reads and keeps there.
    pub(crate) fn transform_plain<A: host::ImportAllocation>(
        engine: &Engine,
        snapshot: &package::snapshot::Snapshot,
        allocation: &mut A,
        mut cache: Option<&mut package::import::resident::Cache>,
    ) -> Result<Self, Error> {
        let reuse = cache.as_deref_mut().map_or(Reuse::Fresh, Reuse::Kept);
        let lowered = lower(
            engine,
            snapshot,
            allocation,
            reuse,
            reload::source::Graph::from_package,
        )?;
        let parses = cache.and_then(|cache| cache.parses());
        let candidate = reload::Candidate::from_package_plain(
            lowered.executable,
            lowered.read,
            engine,
            snapshot,
            parses,
        );
        Ok(Self {
            candidate,
            imported: lowered.imported,
            sources: lowered.sources,
        })
    }

    /// Consume the actual compiled output only after every captured native use
    /// matches the same carrier. Runtime projections remain in its candidate
    /// for the normal off-live binding gate.
    pub(crate) fn cover(self, carrier: &manifest::Carrier) -> Coverage {
        match covered(&self.imported, &self.sources, carrier) {
            Ok(coverage) => Coverage::Matched(Box::new(Matched {
                output: self,
                carrier: *carrier.identity(),
                coverage,
            })),
            Err(unbound) => Coverage::Uncovered(unbound),
        }
    }
}

/// What one compile against the retained native surface lowered, and what
/// `read` took of its package graph first.
struct Lowered<T> {
    executable: program::Executable,
    imported: resolve::lower_nominal::ImportedOutput,
    sources: Box<[diagnostic::Source]>,
    read: T,
}

/// The program `snapshot` holds, compiled from the parses and lowerings
/// `reuse` lends, with the native uses its lowering inferred. A lookup
/// with native provenance that misses is [`Error::NativeMiss`].
fn lower<A: host::ImportAllocation, T>(
    engine: &Engine,
    snapshot: &package::snapshot::Snapshot,
    allocation: &mut A,
    reuse: Reuse<'_>,
    read: impl FnOnce(&package::Graph) -> T,
) -> Result<Lowered<T>, Error> {
    let (mut graph, mut cache) = match reuse {
        Reuse::Fresh => (package::Graph::load_snapshot(engine, snapshot)?, None),
        Reuse::Kept(cache) => (cache.load_snapshot(engine, snapshot)?, Some(cache)),
        Reuse::Read(parses) => (
            package::Graph::load_snapshot_reading(engine, snapshot, parses)?,
            None,
        ),
    };
    let expanded = graph.expand_crate_macros(&engine.host_registry);
    let sources = graph.diagnostic_sources();
    let read = read(&graph);
    let native = resolve::native::Capture::from_graph(&graph, allocation)?;
    let program = expanded
        .and_then(|()| {
            let kept = cache
                .as_deref_mut()
                .map(package::import::resident::Cache::lowerings);
            resolve::lower_nominal::kept::resolve(engine, graph, native.clone(), kept)
        })
        .map_err(|failure| {
            // A crate derive whose item changed, or a crate macro's
            // call an edit made, is expanded by preparation, as a
            // native miss is answered (#593, #1125).
            let missed = matches!(
                failure.error,
                resolve::Error::ExpansionRequired { .. }
                    | resolve::Error::MacroExpansionRequired { .. }
            ) || native.matches(
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
        cache.map(package::import::resident::Cache::lowerings),
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
                // A crate macro's call an edit made: preparation
                // expands it (#1125).
                (
                    _,
                    crate::ParseErrorType::Resolution(resolve::Error::MacroExpansionRequired {
                        ..
                    }),
                ) => true,
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
    Ok(Lowered {
        executable,
        imported,
        sources,
        read,
    })
}

/// Whether `carrier` covers each native use `imported` holds: the digest
/// of what each matched, or the first use it doesn't cover, placed in
/// `sources`.
fn covered(
    imported: &resolve::lower_nominal::ImportedOutput,
    sources: &[diagnostic::Source],
    carrier: &manifest::Carrier,
) -> Result<[u8; 32], Box<Unbound>> {
    let mut digest = Sha256::new();
    digest.update(b"rawr.resident.compiled-uses/v1");
    digest.update(carrier.identity());
    digest.update(carrier.coverage_digest());
    // The script functions each adapter holds: one adapter holds one
    // function at each slot, so two uses that pass two need two.
    let mut held = std::collections::BTreeMap::new();
    for used in &imported.uses {
        // A dependency crate's item from that crate's root, as the
        // carrier names it: `iced::__rawr_crate_iced_widget_<id>::Column`
        // is `iced_widget::Column`.
        let path = host::path::public(&used.path);
        // The impls that cross with its script types, as the runner
        // was built with them: a changed shape is a use it doesn't
        // cover (#326).
        let impls = package::import::crossing_impls(
            &used.types,
            &imported.impls,
            imported.programs.source(used.source),
        );
        let matched = carrier.cover(
            coverage::Use {
                path: &*path,
                selector: &used.selector,
                endpoint: None,
                types: &used.types,
                constants: &used.constants,
                slots: &used.slots,
                impls: &impls,
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
            let what = if shared {
                format!("`{path}`: its adapter already holds other script functions")
            } else {
                format!("`{path}` at types {:?}", used.types)
            };
            return Err(Box::new(Unbound {
                at: sources
                    .get(used.source.index())
                    .map(|source| (source.clone(), used.span)),
                miss: None,
                what: Some(what),
            }));
        };
        digest.update(matched.digest());
    }
    Ok(digest.finalize().into())
}

/// The program `snapshot` holds, compiled as a save compiles it against
/// `native`, the engine's retained native surface ([`Output::transform_plain`]
/// then [`Output::cover`]), with the [`program::Positions`] that place its
/// steps in its files. A source whose text `parses` holds the parse of is
/// read from it, and nothing the compile does is kept. A use `native`'s
/// carrier doesn't cover is the error [`Unbound::running`] reports: code
/// a debugger evaluates in a paused run can't make a use its build lacks.
pub(crate) fn positioned(
    engine: &Engine,
    native: &package::import::runner::account::Native,
    snapshot: &package::snapshot::Snapshot,
    parses: Option<&package::Parses>,
) -> Result<(crate::AST, program::Positions), package::Error> {
    let resource = |error| package::Error::import(package::import::Error::Surface(error));
    let mut attempt = package::import::runner::account::support::Pending::transform(
        native.attempt().map_err(resource)?,
    );
    let reuse = parses.map_or(Reuse::Fresh, Reuse::Read);
    let lowered = match lower(engine, snapshot, &mut attempt, reuse, |_| ()) {
        Ok(lowered) => lowered,
        Err(Error::NativeMiss(error)) => return Err(Unbound::missed(error).running()),
        Err(Error::Source(error)) => return Err(error),
        Err(Error::Resource(error)) => return Err(resource(error)),
    };
    covered(&lowered.imported, &lowered.sources, native.carrier())
        .map_err(|unbound| unbound.running())?;
    let mut executable = lowered.executable;
    executable.retain_imported(attempt.finish());
    Ok(executable.into_ast_with_positions())
}

/// A use of a crate this compiler has no binding for: a lookup with native
/// provenance that missed, or a use at types the carrier doesn't cover.
/// A universe runner (`rawr_import::universe`), which binds what its own
/// program uses and prepares nothing more, reports it as the program's
/// error where the program makes the use ([`Self::error`]). So does code
/// a debugger evaluates in a paused run, whose process gains no native
/// code while it runs ([`Self::running`]). Any other runner prepares it.
pub(crate) struct Unbound {
    at: Option<(diagnostic::Source, Span)>,
    /// The lookup's own error, reported when it has no position.
    miss: Option<package::Error>,
    /// The use the carrier lacks, for `RAWR_VERBOSE`'s line.
    what: Option<String>,
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
            what: None,
        }
    }

    /// Where the use is, as its file's path and the line and column it
    /// starts at, and what missed, in one line.
    pub(crate) fn describe(&self) -> String {
        let place = self.at.as_ref().map(|(source, span)| {
            let start = span.start();
            format!(
                "{}:{}:{}",
                source.identity(),
                start.line().unwrap_or(0),
                start.position().unwrap_or(0)
            )
        });
        let miss = self
            .miss
            .as_ref()
            .map(ToString::to_string)
            .or_else(|| self.what.clone());
        match (place, miss) {
            (Some(place), Some(miss)) => format!("{place}: {miss}"),
            (Some(place), None) => format!("{place}: a use the carrier doesn't cover"),
            (None, Some(miss)) => miss,
            (None, None) => "a use the carrier doesn't cover".to_owned(),
        }
    }

    /// The program's error at the use.
    pub(crate) fn error(self) -> package::Error {
        self.reported(
            |_| "this use of a crate is not available in this prebuilt runner".to_owned(),
            "the runner was not built with this call",
            None,
        )
    }

    /// The error at the use in code evaluated in a paused run, which names
    /// the use as the code writes it. The running process holds the native
    /// code its build compiled, and the use isn't in it; a save that writes
    /// the use in the program rebuilds the runner with it.
    pub(crate) fn running(self) -> package::Error {
        self.reported(
            |written| match written {
                Some(written) => format!("the running build has no binding for `{written}`"),
                None => "the running build has no binding for this use of a crate".to_owned(),
            },
            "the running build was not compiled with this use",
            Some("writing this use in the program and saving it rebuilds the runner with it"),
        )
    }

    /// The report at the use: `message` given the code at its span, with
    /// `label` on it and `note` after it. With no position, the lookup's
    /// own error.
    fn reported(
        self,
        message: impl FnOnce(Option<&str>) -> String,
        label: &str,
        note: Option<&str>,
    ) -> package::Error {
        let Some((source, span)) = self.at else {
            return self.miss.unwrap_or_else(|| {
                package::Error::diagnostic(diagnostic::Error::EmptySourceIdentity)
            });
        };
        let message = message(written(&source, span));
        let report = diagnostic::Report::error(diagnostic::code::RESOLVE_HOST_OPERATION, message)
            .primary(source, span, Some(label.into()))
            .map(|report| match note {
                Some(note) => report.note(note).finish(),
                None => report.finish(),
            });
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

/// The code `span` covers in `source`, its end column included, on one
/// line: `None` when it covers nothing.
fn written(source: &diagnostic::Source, span: Span) -> Option<&str> {
    let text = source.text();
    let byte = |at: crate::Position, after: usize| {
        let column = at.position().unwrap_or(1) + after;
        diagnostic::offset(text, at.line().unwrap_or(1), column)
    };
    let start = byte(span.start(), 0);
    let code = text.get(start..byte(span.end(), 1).max(start))?.trim();
    (!code.is_empty() && !code.contains('\n')).then_some(code)
}
