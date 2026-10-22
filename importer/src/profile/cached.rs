//! Content-addressed framework-profile classification.

use crate::node;

use super::{Checked, Contract, Definition, Error};

const DOMAIN: &[u8] = b"rawr.profile.classification/v2";

pub(super) fn classify<C: Checked, D: Definition<C>>(
    definition: &D,
    graph: &crate::api::Graph<crate::api::Extracted>,
    limits: &crate::cargo::Limits,
) -> Result<Contract, Error> {
    let parent = graph.node_chain().cloned().ok_or(Error::ForeignEvidence)?;
    if parent.output_limit() != limits.node_output_bytes() {
        return Err(Error::DefinitionDrift);
    }
    let identity = definition.identity();
    let version = definition.version();
    let observations = graph.callback_observations(graph.freshness_fingerprint());
    let authority = authority(identity, version, Some(&observations))?;
    let product = parent
        .direct::<Product>(&authority, || {
            let contract = super::classify_direct(definition, graph, limits)
                .map_err(|error| node::Error::Transform(error.to_string().into_boxed_str()))?;
            Product::capture(&contract, limits.node_output_bytes())
                .map_err(|error| node::Error::Transform(error.to_string().into_boxed_str()))
        })
        .map_err(|error| Error::Node(Box::new(error)))?;
    let chain = parent.child(&product);
    product
        .value()
        .replay(identity, version, graph, chain, limits)
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
    pub(crate) fn capture(contract: &Contract, limit: usize) -> Result<Self, Error> {
        Ok(Self {
            canonical: contract.encode(limit)?,
        })
    }

    pub(crate) fn capture_parts(
        parts: &super::contract::Parts,
        limit: usize,
    ) -> Result<Self, Error> {
        Ok(Self {
            canonical: parts.encode(limit)?,
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
        super::contract::Parts::decode_resolved(&self.canonical, identity, version, graph, limits)
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

    fn encode(&self, limit: usize) -> Result<Box<[u8]>, node::Error> {
        if self.canonical.len() > limit {
            return Err(node::Error::Limit {
                resource: "cached profile contract bytes",
                limit,
                actual: self.canonical.len(),
            });
        }
        Ok(self.canonical.clone())
    }

    fn decode(bytes: &[u8], limit: usize) -> Result<Self, node::Error> {
        if bytes.len() > limit {
            return Err(node::Error::Limit {
                resource: "cached profile contract bytes",
                limit,
                actual: bytes.len(),
            });
        }
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
            crate::profile::Identity::new(
                "rawr.profile.cached-test",
                &crate::cargo::Limits::standard(),
            )
            .expect("the test profile identity seals")
        }

        fn version(&self) -> crate::profile::Version {
            crate::profile::Version::new(1).expect("the test profile version seals")
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
        let contract = crate::profile::classify_direct(&Definition, &graph, &limits)
            .expect("the empty profile contract seals");
        let product = Product::capture(&contract, limits.node_output_bytes())
            .expect("the profile product captures");
        let bytes = product
            .encode(limits.node_output_bytes())
            .expect("the profile product encodes");
        assert!(Product::decode(&bytes, limits.node_output_bytes()).is_ok());
        assert!(Product::decode(&[], limits.node_output_bytes()).is_err());

        let mut trailing = bytes.into_vec();
        trailing.push(b' ');
        let decoded = Product::decode(&trailing, limits.node_output_bytes())
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
        let direct = crate::profile::classify_direct(&definition, &graph, &limits)
            .expect("the conservative profile contract seals");
        let mut builder =
            crate::profile::contract::Builder::new(identity, version, &graph, &limits);
        definition
            .classify(&graph, &mut builder)
            .expect("classification succeeds");
        let parts = builder.seal_parts().expect("owned classification seals");
        let product = Product::capture_parts(&parts, limits.node_output_bytes())
            .expect("owned classification captures without a current contract");
        let expected = direct
            .encode(limits.node_output_bytes())
            .expect("contract encodes");
        assert_eq!(product.canonical, expected);
        let replay = product
            .owned(identity, version, &graph, &limits)
            .expect("owned classification validates its exact graph identity");
        assert_eq!(
            replay
                .encode(limits.node_output_bytes())
                .expect("parts encode"),
            expected
        );
        assert!(replay.encode(expected.len() - 1).is_err());
        let foreign = crate::profile::Identity::new("foreign.profile", &limits)
            .expect("foreign profile identity seals");
        assert!(product.owned(foreign, version, &graph, &limits).is_err());
        assert!(product
            .owned(
                identity,
                crate::profile::Version::new(2).expect("version seals"),
                &graph,
                &limits
            )
            .is_err());
        let mut trailing = product.canonical.into_vec();
        trailing.push(b' ');
        let product =
            Product::decode(&trailing, limits.node_output_bytes()).expect("bounded framing");
        assert!(product.owned(identity, version, &graph, &limits).is_err());
    }
}
