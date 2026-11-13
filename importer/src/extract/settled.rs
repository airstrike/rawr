//! A candidate settled under a policy, and the one the last canonicalization
//! in this process settled, kept for the next one whose fragments all have
//! the same bodies (#1786).
//!
//! Settling a candidate (its raw body's encoding, its fixed closure and its
//! ledger) goes over every definition of every crate, and is a function of
//! the fragments' bodies, the catalog, the policy and the limits alone: the
//! invocations and the context only frame what it gives. A save that
//! changed no crate's API, such as a private field, gives the same bodies
//! under a new build's invocations, and its canonicalization frames the
//! kept settlement with them.

use std::sync::{Mutex, PoisonError};

use crate::api;
use crate::api::ledger;
use crate::api::model;
use crate::cargo;
use crate::Result;

use super::{raw_pair, Allocation, Catalog, EvidenceCategory, SealMode};

/// What a candidate's graph holds beside its invocations, and the encoding
/// of its raw body, which the context and the invocations frame.
#[derive(Clone)]
pub(super) struct Settled {
    pub(super) body: Vec<u8>,
    pub(super) definitions: Vec<model::Definition>,
    pub(super) paths: Vec<ledger::Path>,
    pub(super) implementations: Vec<model::Implementation>,
    pub(super) scalar_predicate_proofs: Vec<model::ScalarPredicateProof>,
    pub(super) support: Vec<ledger::Support>,
    pub(super) anonymous: Vec<ledger::AnonymousImport>,
    pub(super) index: Vec<ledger::Index>,
}

impl Settled {
    /// The graph of these parts run as `invocations` under `context`.
    pub(super) fn seal(
        self,
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
                self.definitions,
                self.paths,
                self.implementations,
                self.scalar_predicate_proofs,
                self.support,
                self.anonymous,
                self.index,
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
struct Kept {
    catalog: Catalog,
    limits: cargo::Limits,
    policy: [u8; 32],
    /// Each fragment's read key, in fragment order.
    keys: Vec<[u8; 32]>,
    settled: Settled,
}

static KEPT: Mutex<Option<Kept>> = Mutex::new(None);

/// The settlement of the fragments whose read keys are `keys`, in order,
/// when the last one kept was made of them under this catalog, these limits
/// and the policy whose identity is `policy`.
pub(super) fn take(
    keys: &[[u8; 32]],
    catalog: &Catalog,
    limits: &cargo::Limits,
    policy: &[u8; 32],
) -> Option<Settled> {
    let kept = KEPT.lock().unwrap_or_else(PoisonError::into_inner);
    kept.as_ref()
        .filter(|kept| {
            kept.keys == keys
                && kept.policy == *policy
                && kept.limits == *limits
                && kept.catalog == *catalog
        })
        .map(|kept| kept.settled.clone())
}

/// Keep `settled`, of the fragments whose read keys are `keys`, in place of
/// whatever was kept before.
pub(super) fn keep(
    keys: Vec<[u8; 32]>,
    catalog: &Catalog,
    limits: &cargo::Limits,
    policy: [u8; 32],
    settled: Settled,
) {
    *KEPT.lock().unwrap_or_else(PoisonError::into_inner) = Some(Kept {
        catalog: catalog.clone(),
        limits: *limits,
        policy,
        keys,
        settled,
    });
}
