//! Owned API capture awaiting the connected source-consumer union.

use crate::node;

/// Rebuild the captured graph from its stored API product, only when asked.
pub(crate) type Materialize = Box<
    dyn FnOnce(Option<&super::cached::Product>) -> crate::Result<super::Graph<super::Captured>>,
>;

/// The captured graph, or the deferred rebuild of a current stored API hit.
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
        let graph = match self.graph {
            Lazy::Ready(graph) => graph,
            Lazy::Pending(materialize) => {
                let product = self.product.value();
                let resolved = product.resolve(&self.publication.context)?;
                materialize(Some(resolved.as_ref().unwrap_or(product)))?
            }
        };
        Ok(Materialized {
            product: self.product,
            graph,
            publication: self.publication,
        })
    }

    /// Publish the native-input proof of a current API without its graph. The
    /// owner already revalidated this evidence; the checks match `publish`.
    pub(crate) fn into_native_proof(self) -> Result<crate::input::proof::Proof, node::Error> {
        let node::staging::Owned::Current(api) = &self.product else {
            return Err(node::Error::State("historical API has no current proof"));
        };
        self.publication.proof(api)
    }
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
