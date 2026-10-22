//! Move-only surface hit or cold-rebuild continuation.

/// Result of comparing one complete request with the admitted runner surface.
// The miss owns the allocation-free cold continuation; boxing it would add a
// heap allocation on the surface-change path solely to shrink this enum.
#[allow(clippy::large_enum_variant)]
pub enum Outcome<B> {
    /// The current epoch admitted the new snapshot without invoking Cargo.
    Hit(crate::supervisor::GenerationKey),
    /// The compiled surface differs and the complete cold continuation remains owned.
    Miss(Miss<B>),
}

/// Result of preparing a same-process generation without changing the live
/// application or consuming its cold continuation.
#[allow(clippy::large_enum_variant)]
pub enum Preparation<B> {
    Prepared(Prepared<B>),
    Miss(Miss<B>),
}

/// Correlation for one child request captured before mutable parent preparation.
/// Dropping does not perform IPC; callers must cancel or finish preparation.
#[must_use = "an early hot request must be prepared or explicitly cancelled"]
pub struct Started {
    pub(crate) previous: crate::supervisor::GenerationKey,
    pub(crate) nonce: [u8; 16],
    pub(crate) sequence: u64,
    pub(crate) live: bool,
}

impl Started {
    pub(crate) fn identity(&self) -> [u8; 56] {
        let mut identity = [0; 56];
        identity[..32].copy_from_slice(self.previous.as_bytes());
        identity[32..48].copy_from_slice(&self.nonce);
        identity[48..].copy_from_slice(&self.sequence.to_le_bytes());
        identity
    }
}

/// One runner-prepared hot generation awaiting an external latest-revision
/// commit or explicit abort.
#[must_use = "a prepared hot generation must be committed or aborted"]
pub struct Prepared<B> {
    pub(crate) previous: crate::supervisor::GenerationKey,
    pub(crate) next: crate::supervisor::GenerationKey,
    pub(crate) nonce: [u8; 16],
    pub(crate) route: Option<crate::profile::activation::Route>,
    pub(crate) manifest: crate::runner::Manifest<crate::runner::Generated, B>,
    pub(crate) read_set: crate::node::read_set::Prepared,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Transition {
    Commit,
    Abort,
}

impl<B> Prepared<B> {
    pub(crate) fn commit(
        &self,
        mut transition: impl FnMut(Transition) -> Result<(), super::RunnerError>,
    ) -> Result<(), super::RunnerError> {
        let Err(failure) = self.admit(|| transition(Transition::Commit)) else {
            return Ok(());
        };
        if failure.is_owner_lost() || failure.is_hot_consumed_rejection() {
            return Err(failure);
        }
        match transition(Transition::Abort) {
            Ok(()) => Err(failure),
            Err(cleanup) => Err(super::RunnerError::HotCleanup {
                failure: Box::new(failure),
                cleanup: Box::new(cleanup),
            }),
        }
    }

    /// Revalidate the exact current union after watcher registration.
    pub fn revalidate_prepared(&self) -> Result<(), crate::node::Error> {
        self.read_set.revalidate()
    }

    pub(crate) fn admit<T>(
        &self,
        operation: impl FnOnce() -> Result<T, super::RunnerError>,
    ) -> Result<T, super::RunnerError> {
        match self.read_set.with_live_last_good(operation)? {
            Ok(value) => Ok(value),
            Err(crate::store::transaction::Rejected::Operation(failure)) => Err(failure),
            Err(crate::store::transaction::Rejected::Cleanup { failure, cleanup }) => {
                Err(super::RunnerError::PinCleanup {
                    failure: Box::new(failure),
                    cleanup,
                })
            }
        }
    }
}

/// Complete request and snapshot proof returned by a closed hot gate.
pub struct Miss<B> {
    request: crate::bridge::GenerationRequestKey<B>,
    snapshot: crate::supervisor::snapshot::Bytes,
}

/// One-owner latest-wins miss slot; replacing a miss returns the obsolete
/// complete continuation for immediate resource release.
pub struct Latest<B> {
    pending: Option<Miss<B>>,
}

impl<B> Miss<B> {
    pub(crate) const fn new(
        request: crate::bridge::GenerationRequestKey<B>,
        snapshot: crate::supervisor::snapshot::Bytes,
    ) -> Self {
        Self { request, snapshot }
    }

    /// Recover the exact owned inputs for one serialized cold generation.
    pub fn into_parts(
        self,
    ) -> (
        crate::bridge::GenerationRequestKey<B>,
        crate::supervisor::snapshot::Bytes,
    ) {
        (self.request, self.snapshot)
    }

    pub(crate) const fn revision(&self) -> crate::supervisor::Revision {
        self.request.revision()
    }
}

impl<B> Latest<B> {
    #[must_use]
    pub const fn new() -> Self {
        Self { pending: None }
    }

    /// Keep only the greatest complete revision and return whichever request is obsolete.
    pub fn coalesce(&mut self, miss: Miss<B>) -> Option<Miss<B>> {
        match self.pending.as_ref() {
            Some(current) if current.revision() > miss.revision() => Some(miss),
            _ => self.pending.replace(miss),
        }
    }

    /// Take the sole newest cold-build continuation.
    pub fn take(&mut self) -> Option<Miss<B>> {
        self.pending.take()
    }
}

impl<B> Default for Latest<B> {
    fn default() -> Self {
        Self::new()
    }
}
