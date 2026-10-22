//! Content-addressed facade-plan selection.

use super::Plan;
use crate::node;

const DOMAIN: &[u8] = b"rawr.surface.facade/v1";

/// Exact facade bytes plus the current validated facade DAG link.
pub struct Cached {
    product: node::Validated<Product>,
    chain: node::Chain,
}

pub(super) fn encode(
    contract: &crate::Contract<crate::Classified>,
    limits: &crate::cargo::Limits,
) -> crate::Result<Cached> {
    let parent = contract
        .node_chain()
        .cloned()
        .ok_or_else(|| crate::Error::Input {
            code: "missing-generic-node-parent",
            reason: "cached facade selection requires a current generic-contract receipt".into(),
        })?;
    if parent.output_limit() != limits.node_output_bytes() {
        return Err(crate::Error::Request {
            field: "facade cache limits",
            reason: "differ from the limits sealed by the API node context".into(),
        });
    }
    let authority =
        authority(contract.fingerprint().as_bytes()).map_err(|source| crate::Error::Node {
            operation: "capture facade node authority",
            source: Box::new(source),
        })?;
    let product = parent
        .direct::<Product>(&authority, || {
            let bytes = Plan::encode(contract, limits)
                .map_err(|error| node::Error::Transform(error.to_string().into_boxed_str()))?;
            Product::capture(bytes)
        })
        .map_err(|source| crate::Error::Node {
            operation: "facade selection",
            source: Box::new(source),
        })?;
    Ok(Cached::from_validated(parent.context().clone(), product))
}

pub(crate) fn authority(fingerprint: &[u8; 32]) -> Result<Vec<u8>, node::Error> {
    let mut authority = Vec::new();
    authority
        .try_reserve_exact(DOMAIN.len().saturating_add(4).saturating_add(32))
        .map_err(|_| node::Error::Allocation)?;
    authority.extend_from_slice(DOMAIN);
    authority.extend_from_slice(&super::SCHEMA.to_le_bytes());
    authority.extend_from_slice(fingerprint);
    Ok(authority)
}

impl Cached {
    pub(crate) fn entry(&self) -> crate::node::read_set::Entry {
        crate::node::read_set::Entry {
            kind: node::Kind::Facade,
            schema: <Product as node::Value>::SCHEMA,
            action: self.product.receipt().action(),
            recipe: self.product.receipt().recipe_digest(),
            content: self.product.receipt().content_digest(),
            bundled: false,
        }
    }

    /// The same check from the contract identity and its generic node alone.
    pub(crate) fn require_contract_parts(
        &self,
        contract: crate::contract::fingerprint::Fingerprint,
        parent: &node::Chain,
    ) -> Result<(), crate::surface::Error> {
        if parent.context().identity() != self.chain.context().identity()
            || !parent
                .context()
                .graph_budget()
                .shares_with(self.chain.context().graph_budget())
        {
            return Err(crate::surface::Error::ForeignRunner);
        }
        let mut authority = [0; DOMAIN.len() + 4 + 32];
        authority[..DOMAIN.len()].copy_from_slice(DOMAIN);
        authority[DOMAIN.len()..DOMAIN.len() + 4].copy_from_slice(&super::SCHEMA.to_le_bytes());
        authority[DOMAIN.len() + 4..].copy_from_slice(contract.as_bytes());
        let action = node::ActionKey::derive(
            node::Kind::Facade,
            <Product as node::Value>::SCHEMA,
            node::Digest::from_bytes(parent.context().identity()),
            &authority,
            parent.output_limit(),
            std::slice::from_ref(parent.current_parent()),
        );
        if action != self.product.receipt().action() {
            return Err(crate::surface::Error::ForeignRunner);
        }
        Ok(())
    }

    pub(crate) fn from_validated(
        context: crate::context::Context,
        product: node::Validated<Product>,
    ) -> Self {
        let chain = node::Chain::validated(context, &product);
        Self { product, chain }
    }

    /// Borrow the byte-identical canonical facade plan.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.product.value().bytes
    }

    /// Borrow the current facade node for downstream checked-demand joins.
    #[doc(hidden)]
    #[must_use]
    pub const fn node_chain(&self) -> &node::Chain {
        &self.chain
    }
}

/// Owned facade encoding, without a current facade chain.
pub(crate) struct Product {
    bytes: Box<[u8]>,
}

impl Product {
    pub(crate) fn capture_parts(
        graph: &crate::api::Graph<impl crate::api::Phase>,
        parts: &crate::classify::ClassifiedParts,
        limits: &crate::cargo::Limits,
    ) -> Result<Self, node::Error> {
        let bytes = Plan::encode_parts(graph, parts, limits)
            .map_err(|error| node::Error::Transform(error.to_string().into_boxed_str()))?;
        Self::capture(bytes)
    }

    fn capture(bytes: Box<[u8]>) -> Result<Self, node::Error> {
        let product = Self { bytes };
        product.validate()?;
        Ok(product)
    }

    fn validate(&self) -> Result<(), node::Error> {
        let plan: Plan = serde_json::from_slice(&self.bytes)
            .map_err(|_| node::Error::Codec("cached facade plan is not valid JSON"))?;
        plan.validate()
            .map_err(|_| node::Error::Codec("cached facade plan is invalid"))?;
        let canonical = serde_json::to_vec(&plan)
            .map_err(|_| node::Error::Codec("cached facade plan cannot be re-encoded"))?;
        if canonical.as_slice() != self.bytes.as_ref() {
            return Err(node::Error::Codec(
                "cached facade plan is not canonically encoded",
            ));
        }
        Ok(())
    }
}

impl node::Value for Product {
    const KIND: node::Kind = node::Kind::Facade;
    const SCHEMA: u32 = 1;

    fn encode(&self, limit: usize) -> Result<Box<[u8]>, node::Error> {
        if self.bytes.len() > limit {
            return Err(node::Error::Limit {
                resource: "cached facade plan bytes",
                limit,
                actual: self.bytes.len(),
            });
        }
        Ok(self.bytes.clone())
    }

    fn decode(bytes: &[u8], limit: usize) -> Result<Self, node::Error> {
        if bytes.len() > limit {
            return Err(node::Error::Limit {
                resource: "cached facade plan bytes",
                limit,
                actual: bytes.len(),
            });
        }
        Self::capture(bytes.to_vec().into_boxed_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::{Plan, Product};
    use crate::node::Value as _;

    #[test]
    fn facade_product_roundtrips_and_rejects_noncanonical_or_trailing_bytes() {
        let limits = crate::cargo::Limits::standard();
        let contract = crate::test_fixture::graph()
            .classify(&crate::classify::Policy::standard(), &limits)
            .expect("the fixture graph classifies");
        let bytes = Plan::encode(&contract, &limits).expect("the facade encodes");
        let product = Product::capture(bytes.clone()).expect("the facade product captures");
        let encoded = product
            .encode(limits.node_output_bytes())
            .expect("the facade product encodes");
        assert_eq!(encoded, bytes);
        assert!(Product::decode(&encoded, limits.node_output_bytes()).is_ok());

        let mut trailing = encoded.into_vec();
        trailing.push(b' ');
        assert!(Product::decode(&trailing, limits.node_output_bytes()).is_err());
    }
}
