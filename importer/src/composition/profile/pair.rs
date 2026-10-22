//! Owned profile outputs awaiting the actual runner consumer.

use crate::node::staging::Owned;
use crate::{node, profile};
use profile::demand::cached::{GeneratedProduct, Product};

/// Audited generated profile data with historical ancestry, not current authority.
pub struct Pair {
    pub(crate) context: crate::context::Context,
    pub(crate) demand: Owned<Product>,
    pub(crate) generated: Owned<GeneratedProduct>,
    pub(crate) parts: profile::demand::Parts,
    pub(crate) proof: profile::Parts,
    pub(crate) demand_retention: node::read_set::Retention,
    pub(crate) retention: node::read_set::Retention,
}

impl Pair {
    pub(crate) fn generated_fragment(&self) -> &node::Fragment {
        match &self.generated {
            Owned::Current(value) => value.receipt().read_set(),
            Owned::Staged(value) => value.fragment(),
        }
    }

    /// Authenticated demand identity for diagnostics.
    pub fn demand_digest(&self) -> &[u8; 32] {
        self.parts.value().digest()
    }
    /// Authenticated generated profile identity for diagnostics.
    pub fn fingerprint(&self) -> &[u8; 32] {
        self.proof.value().fingerprint()
    }
    /// Authenticated rules identity for diagnostics.
    pub fn rules_digest(&self) -> &[u8; 32] {
        self.parts.value().rules_digest()
    }

    /// Consume only already-current products; no store acquisition or replay.
    pub(crate) fn into_current(self) -> Result<(profile::Demand, profile::Proof), profile::Error> {
        let (Owned::Current(demand), Owned::Current(generated)) = (self.demand, self.generated)
        else {
            return Err(super::node_error(node::Error::State(
                "historical pair requires promotion",
            )));
        };
        super::finish(
            &self.context,
            demand,
            generated,
            self.parts,
            self.proof,
            self.demand_retention,
            self.retention,
        )
    }

    pub(crate) fn promote(
        self,
        force: bool,
        store: &crate::store::Store,
    ) -> Result<(profile::Demand, profile::Proof), profile::Error> {
        let (demand, generated) = match (self.demand, self.generated) {
            (Owned::Current(demand), Owned::Current(generated)) if !force => (demand, generated),
            (demand, generated) => node::staging::promote_owned_pair(demand, generated, store)
                .map_err(super::node_error)?,
        };
        super::finish(
            &self.context,
            demand,
            generated,
            self.parts,
            self.proof,
            self.demand_retention,
            self.retention,
        )
    }
}

pub(super) trait Output: Sized {
    fn complete(pair: Pair, store: &crate::store::Store) -> Result<Self, profile::Error>;
}
impl Output for Pair {
    fn complete(pair: Pair, _: &crate::store::Store) -> Result<Self, profile::Error> {
        Ok(pair)
    }
}
impl Output for (profile::Demand, profile::Proof) {
    fn complete(pair: Pair, store: &crate::store::Store) -> Result<Self, profile::Error> {
        pair.promote(false, store)
    }
}
