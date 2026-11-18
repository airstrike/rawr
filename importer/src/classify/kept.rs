//! The classification a long-running owner keeps between saves.
//!
//! Replaying a stored classification decodes every outcome and checks its
//! support ledger against the API. It is a function of the stored
//! product and the API's semantic content alone: a save whose API and
//! classification are the last one's, as after a private field, takes the
//! parts the last replay gave, and only re-seats the API graph's usage they
//! are charged on (#1786).
//!
//! It keeps each item's record of the last classification too, and where
//! each crate's records are kept beyond the process (`records`): a later
//! classification takes an item from either where the item reads the same
//! (#1618, #271).

use super::{api, cached, ClassifiedParts, Error, Policy};
use crate::node;

/// The last classification an owner replayed, for the next save, and the
/// last one it classified item by item, for the next classification of a
/// grown or changed API (#1618).
#[derive(Default)]
pub struct Kept {
    held: Option<Held>,
    items: Option<std::sync::Arc<super::items::Items>>,
    /// Each path's facade contribution of the last plan this owner made.
    contributions: Option<std::sync::Arc<crate::surface::facade::contribution::Contributions>>,
    /// Where each crate's records are kept, and what was read of them, once
    /// a classification needs them.
    #[cfg(feature = "rustc-driver")]
    records: Option<Records>,
}

/// Where a classification's records are kept, and what was read of them.
#[cfg(feature = "rustc-driver")]
struct Records {
    sources: super::records::Sources,
    loaded: Option<super::records::Loaded>,
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
        if let Some(items) = generic.items()
            && !self
                .items
                .as_ref()
                .is_some_and(|kept| std::sync::Arc::ptr_eq(kept, items))
        {
            #[cfg(feature = "rustc-driver")]
            if let Some(records) = &mut self.records {
                let loaded = records.loaded.get_or_insert_with(Default::default);
                records.sources.publish(items, loaded);
            }
            self.items = Some(std::sync::Arc::clone(items));
        }
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

impl Kept {
    /// Keep each crate's records where `sources` says, and take earlier
    /// records from there, read once a classification needs them.
    #[cfg(feature = "rustc-driver")]
    pub fn share(&mut self, sources: super::records::Sources) {
        if self
            .records
            .as_ref()
            .is_some_and(|records| records.sources.same(&sources))
        {
            return;
        }
        self.records = Some(Records {
            sources,
            loaded: None,
        });
    }

    /// Read each crate's kept records and contributions, once.
    fn load(&mut self) {
        #[cfg(feature = "rustc-driver")]
        if let Some(records) = &mut self.records
            && records.loaded.is_none()
        {
            records.sources.resolve();
            records.loaded = Some(records.sources.load());
        }
    }

    /// Each path's facade contribution a plan may take: the last plan's,
    /// then each crate's kept ones, read now if not yet.
    pub(crate) fn earlier_contributions(
        &mut self,
    ) -> Vec<&crate::surface::facade::contribution::Contributions> {
        self.load();
        let kept = self.contributions.as_deref().into_iter();
        #[cfg(feature = "rustc-driver")]
        let loaded = self
            .records
            .as_ref()
            .and_then(|records| records.loaded.as_ref())
            .into_iter()
            .flat_map(super::records::Loaded::contributions);
        #[cfg(not(feature = "rustc-driver"))]
        let loaded = std::iter::empty();
        kept.chain(loaded).collect()
    }

    /// Keep `contributions`, a plan's, for the next plan, and each crate's
    /// where its records are kept.
    pub(crate) fn keep_contributions(
        &mut self,
        contributions: crate::surface::facade::contribution::Contributions,
    ) {
        #[cfg(feature = "rustc-driver")]
        if let (Some(records), Some(items)) = (&mut self.records, &self.items) {
            let loaded = records.loaded.get_or_insert_with(Default::default);
            records
                .sources
                .publish_facade(&contributions, items, loaded);
        }
        self.contributions = Some(std::sync::Arc::new(contributions));
    }

    /// Each item's record a classification may take: the last one this
    /// owner classified, then each crate's kept records, read now if not
    /// yet.
    pub(crate) fn earlier(&mut self) -> Vec<&super::items::Items> {
        self.load();
        let kept = self.items.as_deref().into_iter();
        #[cfg(feature = "rustc-driver")]
        let loaded = self
            .records
            .as_ref()
            .and_then(|records| records.loaded.as_ref())
            .into_iter()
            .flat_map(super::records::Loaded::items);
        #[cfg(not(feature = "rustc-driver"))]
        let loaded = std::iter::empty();
        kept.chain(loaded).collect()
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
