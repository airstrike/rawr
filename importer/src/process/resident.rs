//! A resident compiler result bound to one live owner and original hot attempt.

/// Only the prepared branch owns a candidate awaiting commit or abort.
// Boxing `Prepared` would change this public, exhaustively matched contract.
#[allow(clippy::large_enum_variant)]
pub enum Outcome {
    Prepared(Prepared),
    NeedsPreparation,
    Source(Box<str>),
    Obsolete,
    Resource(Box<str>),
}

/// Actual child compilation plus the exact current Rawr-file witness.
#[must_use = "resident preparation must be committed or aborted"]
pub struct Prepared {
    pub(crate) previous: crate::supervisor::GenerationKey,
    pub(crate) next: crate::supervisor::GenerationKey,
    pub(crate) nonce: [u8; 16],
    pub(crate) route: crate::profile::activation::Route,
    pub(crate) freshness: crate::supervisor::snapshot::attempt::Freshness,
}

impl Prepared {
    /// Revalidate the exact snapshot using this attempt's cumulative budget.
    pub fn revalidate(&self) -> Result<(), crate::node::Error> {
        self.freshness.revalidate()
    }

    pub(crate) fn identity(&self) -> [u8; 48] {
        let mut identity = [0; 48];
        identity[..32].copy_from_slice(self.next.as_bytes());
        identity[32..].copy_from_slice(&self.nonce);
        identity
    }
}
