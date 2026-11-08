//! Content-addressed framework-profile classification.

use crate::node;

use super::{Checked, Contract, Definition, Error};

const DOMAIN: &[u8] = b"rawr.profile.classification/v2";

pub(super) fn classify<C: Checked, D: Definition<C>>(
    definition: &D,
    graph: &crate::api::Graph<crate::api::Extracted>,
    limits: &crate::cargo::Limits,
) -> Result<Contract, Error> {
    let parent = graph
        .node_chain()
        .cloned()
        .expect("the cached profile's graph has no node");
    let identity = definition.identity();
    let version = definition.version();
    let observations = graph.callback_observations(graph.freshness_fingerprint());
    let authority = authority(identity, version, Some(&observations))?;
    let product = parent
        .direct::<Product>(&authority, || {
            let contract = super::classify_direct(definition, graph)
                .map_err(|error| node::Error::Transform(error.to_string().into_boxed_str()))?;
            Product::capture(&contract)
                .map_err(|error| node::Error::Transform(error.to_string().into_boxed_str()))
        })
        .map_err(|error| Error::Node(Box::new(error)))?;
    let chain = parent.child(&product);
    product
        .value()
        .replay(identity, version, graph, chain, limits)
        .map_err(Error::fresh)
}

pub(crate) fn authority(
    identity: super::Identity,
    version: super::Version,
    exact: Option<&[u8; 96]>,
) -> Result<Vec<u8>, Error> {
    let mut authority = Vec::new();
    authority
        .try_reserve_exact(
            DOMAIN
                .len()
                .saturating_add(41 + exact.map_or(0, |bytes| bytes.len())),
        )
        .map_err(|_| Error::Allocation)?;
    authority.extend_from_slice(DOMAIN);
    authority.extend_from_slice(&super::VERSION.to_le_bytes());
    authority.extend_from_slice(identity.as_bytes());
    authority.extend_from_slice(&version.get().to_le_bytes());
    authority.push(u8::from(exact.is_some()));
    if let Some(exact) = exact {
        authority.extend_from_slice(exact);
    }
    Ok(authority)
}

pub(crate) struct Product {
    canonical: Box<[u8]>,
}

impl Product {
    pub(crate) fn capture(contract: &Contract) -> Result<Self, Error> {
        Ok(Self {
            canonical: contract.encode()?,
        })
    }

    pub(crate) fn capture_parts(
        parts: &super::contract::Parts,
    ) -> Result<Self, Error> {
        Ok(Self {
            canonical: parts.encode()?,
        })
    }

    pub(crate) fn owned(
        &self,
        identity: super::Identity,
        version: super::Version,
        graph: &crate::api::Graph<impl crate::api::Phase>,
        limits: &crate::cargo::Limits,
    ) -> Result<super::contract::Parts, Error> {
        super::contract::Parts::decode(&self.canonical, identity, version, graph, limits)
    }

    /// Decode under the API identities recorded by a resolved product (no graph).
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn owned_resolved(
        &self,
        identity: super::Identity,
        version: super::Version,
        graph: (crate::api::Fingerprint, crate::api::Fingerprint),
        limits: &crate::cargo::Limits,
    ) -> Result<super::contract::Parts, Error> {
        super::contract::Parts::decode_resolved(&self.canonical, identity, version, graph)
    }

    fn replay(
        &self,
        identity: super::Identity,
        version: super::Version,
        graph: &crate::api::Graph<crate::api::Extracted>,
        chain: node::Chain,
        limits: &crate::cargo::Limits,
    ) -> Result<Contract, Error> {
        Contract::decode(&self.canonical, identity, version, graph, chain, limits)
    }
}

impl node::Value for Product {
    const KIND: node::Kind = node::Kind::Profile;
    const SCHEMA: u32 = 2;

    fn encode(&self) -> Result<Box<[u8]>, node::Error> {
        Ok(self.canonical.clone())
    }

    fn decode(bytes: &[u8]) -> Result<Self, node::Error> {
        if bytes.is_empty() {
            return Err(node::Error::Codec("cached profile contract is empty"));
        }
        Ok(Self {
            canonical: bytes.to_vec().into_boxed_slice(),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::node::Value as _;

    use super::Product;

    struct Checked;
    struct Definition;

    impl crate::profile::Checked for Checked {
        fn profile_fingerprint(&self) -> [u8; 32] {
            [3; 32]
        }
    }

    impl crate::profile::Definition<Checked> for Definition {
        fn identity(&self) -> crate::profile::Identity {
            crate::profile::Identity::new("rawr.profile.cached-test")
        }

        fn version(&self) -> crate::profile::Version {
            crate::profile::Version::new(1)
        }

        fn classify(
            &self,
            _graph: &crate::api::Graph<crate::api::Extracted>,
            _output: &mut crate::profile::contract::Builder<'_>,
        ) -> Result<(), crate::profile::Error> {
            Ok(())
        }

        fn demand(
            &self,
            _contract: &crate::profile::Contract,
            _checked: &Checked,
            _output: &mut crate::profile::demand::Builder<'_>,
        ) -> Result<(), crate::profile::Error> {
            Ok(())
        }

        fn generate(
            &self,
            _demand: &crate::profile::Demand,
            _output: &mut crate::profile::demand::Generator<'_>,
        ) -> Result<(), crate::profile::Error> {
            Ok(())
        }
    }

    #[test]
    fn profile_product_roundtrips_and_rejects_empty_payload() {
        let limits = crate::cargo::Limits::standard();
        let graph = crate::test_fixture::graph();
        let contract = crate::profile::classify_direct(&Definition, &graph)
            .expect("the empty profile contract seals");
        let product = Product::capture(&contract)
            .expect("the profile product captures");
        let bytes = product
            .encode()
            .expect("the profile product encodes");
        assert!(Product::decode(&bytes).is_ok());
        assert!(Product::decode(&[]).is_err());

        let mut trailing = bytes.into_vec();
        trailing.push(b' ');
        let decoded = Product::decode(&trailing)
            .expect("framing alone retains bounded bytes");
        assert_ne!(decoded.canonical.as_ref(), product.canonical.as_ref());
    }

    #[test]
    fn owned_profile_parts_preserve_canonical_contract_and_reject_drift() {
        use crate::profile::Definition as _;

        let limits = crate::cargo::Limits::standard();
        let graph = crate::test_fixture::graph();
        let definition = Definition;
        let identity = definition.identity();
        let version = definition.version();
        let direct = crate::profile::classify_direct(&definition, &graph)
            .expect("the conservative profile contract seals");
        let mut builder = crate::profile::contract::Builder::new(identity, version, &graph);
        definition
            .classify(&graph, &mut builder)
            .expect("classification succeeds");
        let parts = builder.seal_parts().expect("owned classification seals");
        let product = Product::capture_parts(&parts)
            .expect("owned classification captures without a current contract");
        let expected = direct
            .encode()
            .expect("contract encodes");
        assert_eq!(product.canonical, expected);
        let replay = product
            .owned(identity, version, &graph, &limits)
            .expect("owned classification validates its exact graph identity");
        assert_eq!(
            replay
                .encode()
                .expect("parts encode"),
            expected
        );
        assert!(replay.encode(expected.len() - 1).is_err());
        // Read under other inputs, a stored contract is stale: the replay
        // rebuilds it, and no message reaches the program (#538).
        let stale = |result: Result<crate::profile::contract::Parts, crate::profile::Error>| {
            matches!(result, Err(crate::profile::Error::Stale))
        };
        let foreign = crate::profile::Identity::new("foreign.profile");
        assert!(stale(product.owned(foreign, version, &graph, &limits)));
        assert!(stale(product.owned(
            identity,
            crate::profile::Version::new(2),
            &graph,
            &limits
        )));
        let mut trailing = product.canonical.into_vec();
        trailing.push(b' ');
        let product =
            Product::decode(&trailing).expect("bounded framing");
        assert!(stale(product.owned(identity, version, &graph, &limits)));
    }

    /// An entry keyed by every input it decodes under can't be stale, so a
    /// hit or fresh build that doesn't decode is a rawr bug and panics
    /// naming the invariant; other failures pass through (#538, D664).
    #[test]
    #[should_panic(
        expected = "a cached profile entry went stale under a key that holds all its inputs"
    )]
    fn a_keyed_profile_entry_that_does_not_decode_is_a_rawr_bug() {
        let allocation = crate::profile::Error::Allocation.fresh();
        assert!(matches!(allocation, crate::profile::Error::Allocation));
        let _ = crate::profile::Error::Stale.fresh();
    }
}
