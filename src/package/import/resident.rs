//! Native-covered compiler output, still requiring live-owner admission.
//!
//! These types do not grant reuse authority. The generated runner must bind the
//! output to its admitted artifact, original request, epoch and publication
//! gate. Native runtime binding remains mandatory before installation.

use crate::{application, host, package, reload, Engine, Shared};

use super::runner::account;
use package::compile::captured;

/// Shared bounded wire codec for generated owners; metadata is not admission.
#[doc(hidden)]
pub use rawr_import::protocol::resident as wire;

#[cfg(test)]
mod tests;

/// The same compiled candidate and its child-derived semantic/native evidence.
pub struct Compiled {
    candidate: reload::Candidate,
    semantic: Option<application::SemanticFingerprint>,
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
    /// Read original native allocation facts without granting reuse authority.
    #[doc(hidden)]
    #[must_use]
    pub fn imported_native_snapshot(&self) -> Option<account::Snapshot> {
        self.imported_native.as_ref().map(account::Native::snapshot)
    }

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
    /// with native provenance request preparation; ordinary source errors do not.
    #[doc(hidden)]
    pub fn compile_package_snapshot_resident(
        &self,
        snapshot: &package::snapshot::Snapshot,
        cycle: &host::facade::ProfileCycle,
    ) -> Result<Outcome, Error> {
        let native = self.imported_native.as_ref().ok_or(Error::Unowned)?;
        let mut attempt = account::support::Pending::transform(native.attempt()?)?;
        let output = match captured::Output::transform(self, snapshot, cycle, &mut attempt) {
            Ok(output) => output,
            Err(captured::Error::NativeMiss(_)) => return Ok(Outcome::NeedsPreparation),
            Err(captured::Error::Source(error)) => return Err(Error::Source(error)),
            Err(captured::Error::Resource(error)) => return Err(Error::Resource(error)),
        };
        let covered = match output.cover(native.carrier()) {
            captured::Coverage::NeedsPreparation => return Ok(Outcome::NeedsPreparation),
            captured::Coverage::Matched(covered) => *covered,
        };
        let (output, carrier, coverage) = covered.into_parts();
        // Complete the one contribution retained by this exact executable and
        // every copy of its closure-captured compiler support before publication.
        let candidate = output.candidate.retain_imported(attempt.finish());
        Ok(Outcome::Compiled(Compiled {
            candidate,
            semantic: output.semantic,
            carrier,
            coverage,
        }))
    }
}

impl Compiled {
    /// Consume the one-shot candidate and the evidence derived from that exact
    /// compilation. No second compile is needed for the eventual installation.
    pub fn into_parts(
        self,
    ) -> (
        reload::Candidate,
        Option<application::SemanticFingerprint>,
        [u8; 32],
        [u8; 32],
    ) {
        (self.candidate, self.semantic, self.carrier, self.coverage)
    }
}
