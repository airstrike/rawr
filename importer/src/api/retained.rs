//! Owned API capture awaiting the connected source-consumer union.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use crate::node;

/// Rebuild the captured graph from its stored API product, only when asked.
pub(crate) type Materialize = Box<
    dyn FnOnce(Option<&super::cached::Product>) -> crate::Result<super::Graph<super::Captured>>
        + Send,
>;

/// The captured graph, or the deferred rebuild of a current stored API hit.
// Ready is the usual result and is matched at once; boxing it would allocate per hit.
#[allow(clippy::large_enum_variant)]
pub(crate) enum Lazy {
    Ready(super::Graph<super::Captured>),
    Pending(Materialize),
}

/// Authenticated API data without a current input or parent capability.
///
/// A current API hit defers its graph: a resolved warm launch never builds it.
pub struct Continuation {
    pub(crate) product: node::staging::Owned<super::cached::Product>,
    pub(crate) graph: Lazy,
    pub(crate) publication: Publication,
}

/// A continuation whose graph has been built for classification.
pub(crate) struct Materialized {
    pub(crate) product: node::staging::Owned<super::cached::Product>,
    pub(crate) graph: super::Graph<super::Captured>,
    pub(crate) publication: Publication,
}

pub(crate) struct Publication {
    pub(crate) context: crate::context::Context,
    pub(crate) retention: node::read_set::Retention,
    owner: node::Validated<crate::cargo::resolution::owner::Product>,
    input: crate::input::stored::Completed,
}

impl Continuation {
    pub(crate) fn capture(
        context: &crate::context::Context,
        owner: node::Validated<crate::cargo::resolution::owner::Product>,
        input: crate::input::stored::Completed,
        product: node::staging::Owned<super::cached::Product>,
        retention: node::read_set::Retention,
        graph: Lazy,
    ) -> Result<Self, node::Error> {
        retention.covers_parents(std::slice::from_ref(&product))?;
        input
            .require_account(context.graph_budget())
            .map_err(|error| node::Error::Transform(error.to_string().into()))?;
        Ok(Self {
            product,
            graph,
            publication: Publication {
                context: context.clone(),
                retention,
                owner,
                input,
            },
        })
    }

    /// Build the captured graph, from the stored API product when deferred.
    pub(crate) fn materialize(self) -> Result<Materialized, node::Error> {
        let graph = build(&self.product, self.graph, &self.publication.context)?;
        Ok(Materialized {
            product: self.product,
            graph,
            publication: self.publication,
        })
    }

    /// Publish the native-input proof of a current API without its graph. The
    /// owner already revalidated this evidence; the checks match `publish`.
    /// A process that keeps graphs builds the deferred one off this thread,
    /// so its next save that changes Rawr source finds it built.
    pub(crate) fn into_native_proof(self) -> Result<crate::input::proof::Proof, node::Error> {
        let Self {
            product,
            graph,
            publication,
        } = self;
        let node::staging::Owned::Current(api) = &product else {
            return Err(node::Error::State("historical API has no current proof"));
        };
        let context = publication.context.clone();
        let proof = publication.proof(api)?;
        if KEEP.load(Ordering::Relaxed) && matches!(graph, Lazy::Pending(_)) {
            std::thread::spawn(move || drop(build(&product, graph, &context)));
        }
        Ok(proof)
    }
}

/// Whether this process keeps the API graph it builds (see [`keep_graphs`]).
static KEEP: AtomicBool = AtomicBool::new(false);

/// The graph this process built last, by its API product's action and
/// content.
#[allow(clippy::type_complexity)]
static KEPT: Mutex<
    Option<(
        (node::ActionKey, node::Digest),
        super::Graph<super::Captured>,
    )>,
> = Mutex::new(None);

/// Keep the API graph this process builds. A `rawr run` owner builds the
/// same graph on each save that changes only Rawr source, and decoding it
/// takes seconds; kept, a save copies it. A warm launch that never built it
/// builds it in the background once the program runs.
pub fn keep_graphs() {
    KEEP.store(true, Ordering::Relaxed);
}

/// The graph of `product`: the captured one, the one this process kept, or
/// one rebuilt from the stored product. The lock is held while it builds, so
/// a save waits for a background build instead of repeating it.
fn build(
    product: &node::staging::Owned<super::cached::Product>,
    graph: Lazy,
    context: &crate::context::Context,
) -> Result<super::Graph<super::Captured>, node::Error> {
    let key = product.identity();
    let keep = KEEP.load(Ordering::Relaxed);
    let mut kept = KEPT.lock().unwrap_or_else(PoisonError::into_inner);
    let graph = match graph {
        Lazy::Ready(graph) => graph,
        Lazy::Pending(materialize) => match kept.as_ref() {
            Some((held, graph)) if *held == key => return Ok(graph.duplicate()),
            _ => {
                let product = product.value();
                let resolved = product.resolve(context)?;
                materialize(Some(resolved.as_ref().unwrap_or(product)))?
            }
        },
    };
    if keep {
        *kept = Some((key, graph.duplicate()));
    }
    Ok(graph)
}

impl Publication {
    pub(crate) fn callback_observations(&self, graph: &super::Graph<super::Captured>) -> [u8; 96] {
        graph.callback_observations(self.input.fingerprint())
    }

    fn check(&self, api: &node::Validated<super::cached::Product>) -> Result<(), node::Error> {
        self.retention
            .covers_parents(std::slice::from_ref(&node::Parent::validated(api)))?;
        api.receipt()
            .read_set()
            .require_ancestor(self.owner.receipt().read_set())
    }

    fn proof(
        self,
        api: &node::Validated<super::cached::Product>,
    ) -> Result<crate::input::proof::Proof, node::Error> {
        self.check(api)?;
        Ok(self
            .input
            .publish(&self.owner, self.context.graph_budget())?)
    }

    pub(crate) fn publish(
        self,
        graph: super::Graph<super::Captured>,
        api: node::Validated<super::cached::Product>,
    ) -> Result<super::Graph<super::Extracted>, node::Error> {
        self.check(&api)?;
        let input = self
            .input
            .publish(&self.owner, self.context.graph_budget())?;
        let chain = node::Chain::validated_retained(self.context, &api, self.retention)?;
        Ok(graph.publish_with_chain(input, Some(chain))?)
    }
}
