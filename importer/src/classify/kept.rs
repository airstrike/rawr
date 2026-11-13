//! The classification a long-running owner keeps between saves.
//!
//! Replaying a stored classification decodes every outcome and checks its
//! support ledger against the API. It is a function of the stored
//! product and the API's semantic content alone: a save whose API and
//! classification are the last one's, as after a private field, takes the
//! parts the last replay gave, and only re-seats the API graph's usage they
//! are charged on (#1786).

use super::{api, cached, ClassifiedParts, Error, Policy};
use crate::node;

/// The last classification an owner replayed, for the next save.
#[derive(Default)]
pub struct Kept {
    held: Option<Held>,
}

struct Held {
    /// The stored classification's content.
    generic: node::Digest,
    /// The semantic content of the API it classifies.
    semantic: api::Fingerprint,
    /// The API graph's usage the parts were charged on.
    seed: api::Usage,
    parts: ClassifiedParts,
}

impl Kept {
    /// The parts of the stored classification `generic`, whose content is
    /// `digest`, for `graph`: the kept ones when they are of the same
    /// content and API, else replayed and kept.
    pub(crate) fn replay(
        &mut self,
        generic: &cached::Product,
        digest: node::Digest,
        graph: &api::Graph<impl api::Phase>,
        policy: &Policy,
    ) -> Result<ClassifiedParts, Error> {
        let semantic = graph.semantic_fingerprint();
        if let Some(held) = self
            .held
            .as_ref()
            .filter(|held| held.generic == digest && held.semantic == semantic)
        {
            return held.parts.share(held.seed, graph.allocation());
        }
        crate::progress::replayed_classification();
        let parts = generic.replay(graph, policy)?;
        let seed = graph.allocation();
        self.held = Some(Held {
            generic: digest,
            semantic,
            seed,
            parts: parts.share(seed, seed)?,
        });
        Ok(parts)
    }
}

impl ClassifiedParts {
    /// These parts again, sharing every ledger, charged on an API graph
    /// whose usage is `now` in place of `then`.
    pub(super) fn share(&self, then: api::Usage, now: api::Usage) -> Result<Self, Error> {
        let reseat = |charged: usize, then: usize, now: usize| {
            charged
                .checked_sub(then)
                .and_then(|own| own.checked_add(now))
                .ok_or(Error::Cardinality)
        };
        Ok(Self {
            definitions: std::sync::Arc::clone(&self.definitions),
            outcomes: std::sync::Arc::clone(&self.outcomes),
            support: std::sync::Arc::clone(&self.support),
            canonical: std::sync::Arc::clone(&self.canonical),
            fingerprint: self.fingerprint,
            allocation: crate::contract::Usage {
                items: reseat(self.allocation.items, then.items, now.items)?,
                bytes: reseat(self.allocation.bytes, then.bytes, now.bytes)?,
            },
        })
    }
}
