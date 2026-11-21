//! A candidate settled under a policy, and the one an owner's last
//! canonicalization settled, kept for the next one whose fragments all have
//! the same bodies (#1786).
//!
//! Settling a candidate (its raw body's encoding, its fixed closure and its
//! ledger) goes over every definition of every crate, and is a function of
//! the fragments' bodies, the catalog, the policy and the limits alone: the
//! invocations and the context only frame what it gives. A save that
//! changed no crate's API, such as a private field, gives the same bodies
//! under a new build's invocations, and its canonicalization frames the
//! kept settlement with them.

use std::sync::Arc;

use crate::api;
use crate::api::ledger;
use crate::api::model;
use crate::cargo;
use crate::Result;

use super::{raw_pair, Allocation, Catalog, EvidenceCategory, SealMode};

/// What a candidate's graph holds beside its invocations, and the encoding
/// of its raw body, which the context and the invocations frame. Its parts
/// are shared: the settlement kept for the next canonicalization and each
/// graph sealed from it hold the same ones.
pub(super) struct Settled {
    pub(super) body: Arc<[u8]>,
    /// Where the selected ledgers sit in the body.
    pub(super) places: Arc<api::selected::Places>,
    pub(super) definitions: Arc<[model::Definition]>,
    pub(super) paths: Arc<[ledger::Path]>,
    pub(super) implementations: Arc<[model::Implementation]>,
    pub(super) scalar_predicate_proofs: Arc<[model::ScalarPredicateProof]>,
    pub(super) support: Arc<[ledger::Support]>,
    pub(super) anonymous: Arc<[ledger::AnonymousImport]>,
    pub(super) index: Arc<[ledger::Index]>,
}

impl Settled {
    /// The graph of these parts run as `invocations` under `context`, which
    /// shares them.
    pub(super) fn seal(
        &self,
        invocations: Vec<api::Invocation>,
        context: &api::Context,
        allocation: &mut Allocation,
        seal: SealMode<'_>,
    ) -> Result<api::Graph<api::Captured>> {
        allocation.category(EvidenceCategory::Raw);
        let (canonical, semantic) = raw_pair(context, &invocations, &self.body, allocation)?;
        // Each encoding is its head, then the body after its opening brace.
        let head = api::frame::Head::new(Some(3), canonical.len() + 1 - self.body.len());
        let raw = api::Graph::raw_pair(
            canonical,
            semantic,
            context.resolution_universe(),
            Some(head),
            Arc::clone(&self.places),
        );
        allocation.category(EvidenceCategory::Graph);
        allocation.final_graph(
            invocations.len(),
            self.definitions.len(),
            self.paths.len(),
            self.implementations.len(),
            self.support.len(),
            self.anonymous.len(),
        );
        let seal_graph = || {
            api::Graph::seal_with_raw(
                invocations,
                Arc::clone(&self.definitions),
                Arc::clone(&self.paths),
                Arc::clone(&self.implementations),
                Arc::clone(&self.scalar_predicate_proofs),
                Arc::clone(&self.support),
                Arc::clone(&self.anonymous),
                Arc::clone(&self.index),
                raw,
                allocation,
            )
        };
        match seal {
            SealMode::Direct => seal_graph(),
            #[cfg(feature = "rustc-driver")]
            SealMode::Evidence(session) => session.observe_inline(
                crate::process::EvidenceInlineStage::GraphSealing,
                seal_graph,
            ),
            #[cfg(not(feature = "rustc-driver"))]
            SealMode::Marker(_) => unreachable!("the marker carries no sealing authority"),
        }
    }
}

/// One settlement, with what it is a function of beside the bodies.
pub(super) struct Kept {
    catalog: Catalog,
    limits: cargo::Limits,
    policy: [u8; 32],
    /// Each fragment's read key, in fragment order.
    keys: Vec<[u8; 32]>,
    settled: Settled,
}

impl Kept {
    /// `settled`, of the fragments whose read keys are `keys`, under this
    /// catalog, these limits and the policy whose identity is `policy`.
    pub(super) fn new(
        keys: Vec<[u8; 32]>,
        catalog: &Catalog,
        limits: &cargo::Limits,
        policy: [u8; 32],
        settled: Settled,
    ) -> Self {
        Self {
            catalog: catalog.clone(),
            limits: *limits,
            policy,
            keys,
            settled,
        }
    }

    /// The settlement, when it was made of the fragments whose read keys are
    /// `keys`, in order, under this catalog, these limits and the policy
    /// whose identity is `policy`.
    pub(super) fn of(
        &self,
        keys: &[[u8; 32]],
        catalog: &Catalog,
        limits: &cargo::Limits,
        policy: &[u8; 32],
    ) -> Option<&Settled> {
        (self.keys == keys
            && self.policy == *policy
            && self.limits == *limits
            && self.catalog == *catalog)
            .then_some(&self.settled)
    }
}
