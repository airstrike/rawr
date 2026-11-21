//! Native-covered compiler output, which still needs live-owner admission.
//!
//! These types do not grant reuse authority. The generated runner must bind the
//! output to its admitted artifact, original request, epoch and publication
//! gate. Native runtime binding remains mandatory before installation.

use crate::{package, reload, Engine, Shared};

use super::runner::account;
use package::compile::captured;

/// Shared bounded wire codec for generated owners; metadata is not admission.
#[doc(hidden)]
pub use rawr_import::protocol::resident as wire;

#[cfg(test)]
mod tests;

/// What a runner's compiler keeps from one compile of its program to the
/// next (#1604, #1646): each source's parse, and each lowering and reading
/// by what it read. The thread that compiles the program again on each save
/// owns it and lends it to each compile; the engine holds none of it, and
/// code a debugger evaluates compiles without it.
pub struct Cache {
    parses: package::Cells,
    lowerings: crate::resolve::lower_nominal::Lowerings,
    /// What Cargo answered its compiles' loads, for as long as the runner
    /// holds the cache.
    answers: crate::project::cargo::Answers,
}

/// The same compiled candidate and its child-derived native evidence.
pub struct Compiled {
    candidate: reload::Candidate,
    carrier: [u8; 32],
    coverage: [u8; 32],
}

/// Closed result of one actual compilation against the retained native surface.
// Boxing `Compiled` would change this public, exhaustively matched contract.
#[allow(clippy::large_enum_variant)]
pub enum Outcome {
    /// Every captured native requirement matched the compiler's own carrier.
    Compiled(Compiled),
    /// A proven native lookup missed, or exact compiled requirements were not covered.
    /// The caller may prepare once; this does not assert the source will compile.
    NeedsPreparation,
}

/// Failures are not generic instructions to invoke Cargo again.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No retained generated native context belongs to this engine.
    #[error("resident compilation requires retained native compiler ownership")]
    Unowned,
    /// A compiler cannot exchange its native account after reconstruction.
    #[error("native compiler ownership is already retained")]
    AlreadyRetained,
    /// The actual source failed parsing, resolution, typing or semantic checks.
    #[error(transparent)]
    Source(#[from] package::Error),
    /// The shared child metadata account refused this attempt.
    #[error(transparent)]
    Resource(#[from] rawr_import::surface::Error),
}

impl Engine {
    /// Move generated native metadata ownership into the completed compiler.
    /// All later executable authorities retain it through this same engine.
    /// This transform alone does not prove artifact admission or a live owner.
    #[doc(hidden)]
    pub fn retain_imported_native(
        mut self,
        native: account::Native,
    ) -> Result<Shared<Self>, Error> {
        if self.imported_native.is_some() {
            return Err(Error::AlreadyRetained);
        }
        self.imported_native = Some(native);
        Ok(Shared::new(self))
    }

    /// Compile the immutable snapshot once and compare actual inferred native
    /// uses with this compiler's retained carrier. Only actual terminal misses
    /// with native provenance request preparation; ordinary source errors do
    /// not. Any program, a plain one included. The compile reads what
    /// `cache` kept of earlier compiles and keeps its own work there.
    #[doc(hidden)]
    pub fn compile_package_snapshot_resident_plain(
        &self,
        snapshot: &package::snapshot::Snapshot,
        cache: &mut Cache,
    ) -> Result<Outcome, Error> {
        Ok(match self.compile_covered(snapshot, Some(cache))? {
            Ok(compiled) => Outcome::Compiled(compiled),
            Err(unbound) => {
                // Under `RAWR_VERBOSE=1`, which use the running build lacks:
                // what `rawr run` prepares again for.
                if std::env::var_os("RAWR_VERBOSE").is_some_and(|verbose| verbose == "1") {
                    eprintln!("rawr runner: not covered: {}", unbound.describe());
                }
                Outcome::NeedsPreparation
            }
        })
    }

    /// Compile the snapshot against this compiler's retained carrier alone,
    /// as a universe runner compiles a snippet: it prepares nothing, so a
    /// native use the carrier doesn't cover is the program's own error,
    /// reported where the program makes it.
    #[doc(hidden)]
    pub fn compile_package_snapshot_covered(
        &self,
        snapshot: &package::snapshot::Snapshot,
    ) -> Result<Compiled, Error> {
        self.compile_covered(snapshot, None)?
            .map_err(|unbound| Error::Source(unbound.error()))
    }

    /// The compiled program, or the first native miss: a lookup with
    /// native provenance, or a use the carrier doesn't cover. A compile
    /// lent `cache` reads and keeps there.
    pub(crate) fn compile_covered(
        &self,
        snapshot: &package::snapshot::Snapshot,
        cache: Option<&mut Cache>,
    ) -> Result<Result<Compiled, captured::Unbound>, Error> {
        let native = self.imported_native.as_ref().ok_or(Error::Unowned)?;
        let mut attempt = account::support::Pending::transform(native.attempt()?);
        let output = captured::Output::transform_plain(self, snapshot, &mut attempt, cache);
        let output = match output {
            Ok(output) => output,
            Err(captured::Error::NativeMiss(error)) => {
                return Ok(Err(captured::Unbound::missed(error)));
            }
            Err(captured::Error::Source(error)) => return Err(Error::Source(error)),
            Err(captured::Error::Resource(error)) => return Err(Error::Resource(error)),
        };
        let covered = match output.cover(native.carrier()) {
            captured::Coverage::Uncovered(unbound) => return Ok(Err(*unbound)),
            captured::Coverage::Matched(covered) => *covered,
        };
        let (output, carrier, coverage) = covered.into_parts();
        // Complete the one contribution retained by this exact executable and
        // every copy of its closure-captured compiler support before publication.
        let candidate = output.candidate.retain_imported(attempt.finish());
        Ok(Ok(Compiled {
            candidate,
            carrier,
            coverage,
        }))
    }
}

impl Default for Cache {
    fn default() -> Self {
        Self::new()
    }
}

impl Cache {
    /// A cache in memory alone.
    #[must_use]
    pub fn new() -> Self {
        Self {
            parses: package::Cells::in_memory(),
            lowerings: crate::resolve::lower_nominal::Lowerings::new(),
            answers: crate::project::cargo::Answers::new(),
        }
    }

    /// A cache that also keeps its compiles in `directory`, the project's
    /// `target/rawr`, for the next runner: each source's lowering and
    /// reveal's readings of its bodies. Its first compile reuses what an
    /// earlier runner kept there under the same keys, and reads each
    /// source `rawr` parsed from the parse `rawr` kept there (#1638).
    #[must_use]
    pub fn in_directory(directory: &std::path::Path) -> Self {
        let mut lowerings = crate::resolve::lower_nominal::Lowerings::new();
        lowerings.store(crate::resolve::lower_nominal::Disk::new(
            directory.join("lowered"),
        ));
        Self {
            parses: package::Cells::new(directory.join("parse")).remember(),
            lowerings,
            answers: crate::project::cargo::Answers::new(),
        }
    }

    /// How many sources its compiles parsed so far, rather than reading
    /// them from an earlier compile's parse.
    #[must_use]
    pub fn parsed(&self) -> usize {
        self.parses.counts().parsed
    }

    /// How many sources its compiles lowered so far, rather than reusing
    /// an earlier compile's lowering (#1646).
    #[must_use]
    pub fn lowered(&self) -> usize {
        self.lowerings.lowered()
    }

    /// How many sources' bodies its compiles read again to reveal what
    /// their calls instantiate, rather than reusing an earlier compile's
    /// reading (#1646).
    #[must_use]
    pub fn revealed(&self) -> usize {
        self.lowerings.revealed()
    }

    /// The package graph of `snapshot`, each source whose text an earlier
    /// compile parsed read from that parse: only the sources whose text
    /// changed are parsed (#1604).
    pub(crate) fn load_snapshot(
        &mut self,
        engine: &Engine,
        snapshot: &package::snapshot::Snapshot,
    ) -> Result<package::Graph, package::Error> {
        package::Graph::load_snapshot_with_cells(engine, snapshot, &self.parses, &mut self.answers)
    }

    /// The parses its compiles read and made, shared: what the generation
    /// its last compile made holds for a debugger.
    pub(crate) fn parses(&self) -> Option<package::Parses> {
        self.parses.parses()
    }

    /// Its lowerings, which a compile reuses and keeps its own in.
    pub(crate) fn lowerings(&mut self) -> &mut crate::resolve::lower_nominal::Lowerings {
        &mut self.lowerings
    }
}

impl Compiled {
    /// The same compiled program's top level, for a runner that evaluates
    /// it when it installs the candidate. Nothing is compiled again.
    #[must_use]
    pub fn with_top_level(mut self) -> (Self, crate::AST) {
        let (candidate, ast) = self.candidate.with_top_level();
        self.candidate = candidate;
        (self, ast)
    }

    /// Consume the one-shot candidate and the evidence derived from that exact
    /// compilation. No second compile is needed for the eventual installation.
    pub fn into_parts(self) -> (reload::Candidate, [u8; 32], [u8; 32]) {
        (self.candidate, self.carrier, self.coverage)
    }
}
