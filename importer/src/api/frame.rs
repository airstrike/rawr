//! A graph framed by another build's context and invocations.
//!
//! A graph's canonical encoding is a head that names its context and its
//! invocations, `{"schema":3,"context":…,"invocations":…,`, then the body
//! its ledgers were encoded into. A native edit that changes no crate's API,
//! such as a private field, gives a new build whose sidecars canonicalize to
//! the same semantic content: the ledgers and the body are the last graph's,
//! and only the head is this build's. Framing the kept graph with the new
//! head copies the body once and decodes nothing (#1786).

use serde::Serialize;
#[cfg(not(target_arch = "wasm32"))]
use sha2::{Digest as _, Sha256};

#[cfg(not(target_arch = "wasm32"))]
use super::{semantic, Captured, Fingerprint, Graph};
use super::{Context, Invocation};
use crate::Result;

/// Where a canonical encoding's body begins, and the schema its head names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Head {
    schema: Option<u32>,
    length: usize,
}

#[derive(Serialize)]
struct Framing<C, I> {
    #[serde(skip_serializing_if = "Option::is_none")]
    schema: Option<u32>,
    context: C,
    invocations: I,
}

impl Head {
    /// The head of an encoding whose first `length` bytes name its schema,
    /// context and invocations.
    pub(crate) const fn new(schema: Option<u32>, length: usize) -> Self {
        Self { schema, length }
    }

    /// The head `canonical` opens with under `context` and `invocations`,
    /// when it opens with one.
    pub(crate) fn of(
        canonical: &[u8],
        schema: Option<u32>,
        context: &Context,
        invocations: &[Invocation],
    ) -> Option<Self> {
        let head = encode(schema, context, invocations).ok()?;
        canonical.starts_with(&head).then_some(Self {
            schema,
            length: head.len(),
        })
    }
}

/// `{"schema":…,"context":…,"invocations":…,` with no closing brace: the
/// fields a body follows.
fn encode(
    schema: Option<u32>,
    context: impl Serialize,
    invocations: impl Serialize,
) -> Result<Vec<u8>> {
    let mut head = serde_json::to_vec(&Framing {
        schema,
        context,
        invocations,
    })
    .map_err(|error| crate::Error::Extraction {
        code: "canonical-head-encoding",
        reason: error.to_string().into_boxed_str(),
    })?;
    head.pop();
    head.push(b',');
    Ok(head)
}

#[cfg(not(target_arch = "wasm32"))]
impl Graph<Captured> {
    /// This graph's ledgers under `context` and `invocations`: the graph a
    /// replay of its stored content gives for that build, when its semantic
    /// content is the one they name. `None` when it is not, or when this
    /// graph's encoding has no known head.
    pub(crate) fn framed(
        &self,
        context: &Context,
        invocations: Vec<Invocation>,
        allocation: &mut crate::extract::Allocation,
    ) -> Result<Option<Self>> {
        let Some(head) = self.head else {
            return Ok(None);
        };
        if context.resolution_universe() != self.universe {
            return Ok(None);
        }
        let body = &self.canonical[head.length..];
        let semantic_head = encode(
            head.schema,
            semantic::Context(context),
            semantic::Invocations(&invocations),
        )?;
        let mut digest = Sha256::new();
        digest.update(&semantic_head);
        digest.update(body);
        if Fingerprint(digest.finalize().into()) != self.semantic_fingerprint {
            return Ok(None);
        }
        let current = encode(head.schema, context, &invocations)?;
        let mut canonical = Vec::new();
        canonical
            .try_reserve_exact(current.len() + body.len())
            .map_err(|source| crate::Error::Allocation {
                operation: "framed canonical graph",
                source,
            })?;
        canonical.extend_from_slice(&current);
        canonical.extend_from_slice(body);
        allocation.items(1)?;
        allocation.bytes(canonical.len())?;
        let mut graph = Self {
            invocations: invocations.into(),
            definitions: std::sync::Arc::clone(&self.definitions),
            exports: std::sync::Arc::clone(&self.exports),
            implementations: std::sync::Arc::clone(&self.implementations),
            scalar_predicate_proofs: std::sync::Arc::clone(&self.scalar_predicate_proofs),
            support: std::sync::Arc::clone(&self.support),
            anonymous: std::sync::Arc::clone(&self.anonymous),
            index: std::sync::Arc::clone(&self.index),
            fingerprint: Fingerprint(Sha256::digest(&canonical).into()),
            canonical: canonical.into(),
            semantic_fingerprint: self.semantic_fingerprint,
            head: Some(Head::new(head.schema, current.len())),
            stored: None,
            universe: self.universe,
            transaction: self.transaction,
            retained: self.retained,
            evidence: (),
            lookup: std::sync::Arc::clone(&self.lookup),
        };
        graph
            .validate_cardinality(allocation)
            .map_err(|error| allocation.failure(error))?;
        graph.retained = graph
            .recount_retained()
            .ok_or_else(|| {
                allocation.retained_failure(crate::Error::Limit {
                    resource: "retained graph usage",
                    limit: usize::MAX,
                    actual: usize::MAX,
                })
            })?
            .usage();
        Ok(Some(graph))
    }
}
