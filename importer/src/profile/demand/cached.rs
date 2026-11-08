//! Content-addressed checked-program profile demand.

use sha2::{Digest as _, Sha256};

use crate::node;

use super::{Contract, Demand, Error};

const DOMAIN: &[u8] = b"rawr.profile.demand/v2";
const PROOF_DOMAIN: &[u8] = b"rawr.profile.generated/v1";

pub(crate) fn evaluate<C: crate::profile::Checked, D: crate::profile::Definition<C>>(
    definition: &D,
    contract: &Contract,
    checked: &C,
) -> Result<Demand, Error> {
    let parent = checked
        .node_chain()
        .or_else(|| contract.node_chain())
        .cloned()
        .expect("the cached profile's contract has no node");
    assert!(
        definition.identity() == contract.identity() && definition.version() == contract.version(),
        "the profile demanding hooks differs from the one that classified the contract"
    );
    let checked_fingerprint = checked.profile_fingerprint();
    let payload = checked.profile_payload();
    let authority = demand_authority(
        definition.identity(),
        definition.version(),
        contract.rules_digest(),
        &checked_fingerprint,
        payload,
        Some(&contract.graph_fingerprint()),
    )?;
    let product = parent
        .direct::<Product>(&authority, || {
            let demand = crate::profile::demand_direct(definition, contract, checked)
                .map_err(|error| node::Error::Transform(error.to_string().into_boxed_str()))?;
            Product::capture(&demand)
                .map_err(|error| node::Error::Transform(error.to_string().into_boxed_str()))
        })
        .map_err(|error| Error::Node(Box::new(error)))?;
    let chain = parent.child(&product);
    product
        .value()
        .replay(contract, checked, chain)
        .map_err(Error::fresh)
}

pub(crate) fn generate<C: crate::profile::Checked, D: crate::profile::Definition<C>>(
    definition: &D,
    demand: &Demand,
) -> Result<crate::profile::Proof, Error> {
    let parent = demand
        .node_chain()
        .cloned()
        .expect("the cached profile's demand has no node");
    assert!(
        definition.identity() == demand.identity() && definition.version() == demand.version(),
        "the profile generating the source differs from the one that demanded it"
    );
    let product_lifecycle = definition.owns_product_lifecycle();
    let authority = generated_authority(
        definition.identity(),
        definition.version(),
        demand.rules_digest(),
        demand.checked_fingerprint(),
        demand.digest(),
        product_lifecycle,
    )?;
    let product = parent
        .direct::<GeneratedProduct>(&authority, || {
            let proof = crate::profile::Proof::generate_direct(definition, demand)
                .map_err(|error| node::Error::Transform(error.to_string().into_boxed_str()))?;
            GeneratedProduct::capture(&proof)
                .map_err(|error| node::Error::Transform(error.to_string().into_boxed_str()))
        })
        .map_err(|error| Error::Node(Box::new(error)))?;
    let chain = parent.child(&product);
    product
        .value()
        .replay(demand, product_lifecycle, chain)
        .map_err(Error::fresh)
}

/// Encode the existing demand identity from captured, immutable inputs.
pub(crate) fn demand_authority(
    identity: crate::profile::Identity,
    version: crate::profile::Version,
    rules_digest: &[u8; 32],
    checked_fingerprint: &[u8; 32],
    payload: &[u8],
    exact_graph: Option<&crate::api::Fingerprint>,
) -> Result<Vec<u8>, Error> {
    let mut authority = Vec::new();
    authority
        .try_reserve_exact(
            DOMAIN
                .len()
                .saturating_add(145 + exact_graph.map_or(0, |_| 32)),
        )
        .map_err(|_| Error::Allocation)?;
    authority.extend_from_slice(DOMAIN);
    authority.extend_from_slice(&crate::profile::version().to_le_bytes());
    authority.extend_from_slice(identity.as_bytes());
    authority.extend_from_slice(&version.get().to_le_bytes());
    authority.extend_from_slice(rules_digest);
    authority.extend_from_slice(checked_fingerprint);
    authority.extend_from_slice(
        &u64::try_from(payload.len())
            .unwrap_or(u64::MAX)
            .to_le_bytes(),
    );
    authority.extend_from_slice(&<[u8; 32]>::from(Sha256::digest(payload)));
    authority.push(u8::from(exact_graph.is_some()));
    if let Some(graph) = exact_graph {
        authority.extend_from_slice(graph.as_bytes());
    }
    Ok(authority)
}

/// Encode the existing generated identity without invoking profile callbacks.
pub(crate) fn generated_authority(
    identity: crate::profile::Identity,
    version: crate::profile::Version,
    rules_digest: &[u8; 32],
    checked_fingerprint: &[u8; 32],
    demand_digest: &[u8; 32],
    product_lifecycle: bool,
) -> Result<Vec<u8>, Error> {
    let mut authority = Vec::new();
    authority
        .try_reserve_exact(PROOF_DOMAIN.len().saturating_add(105))
        .map_err(|_| Error::Allocation)?;
    authority.extend_from_slice(PROOF_DOMAIN);
    authority.extend_from_slice(&crate::profile::version().to_le_bytes());
    authority.extend_from_slice(identity.as_bytes());
    authority.extend_from_slice(&version.get().to_le_bytes());
    authority.extend_from_slice(rules_digest);
    authority.extend_from_slice(checked_fingerprint);
    authority.extend_from_slice(demand_digest);
    authority.push(u8::from(product_lifecycle));
    Ok(authority)
}

pub(crate) struct Product {
    canonical: Box<[u8]>,
}

impl Product {
    pub(crate) fn capture(demand: &Demand) -> Result<Self, Error> {
        Ok(Self {
            canonical: demand.encode()?,
        })
    }

    pub(crate) fn owned(
        &self,
        contract: &Contract,
        checked_fingerprint: [u8; 32],
        checked_payload: &[u8],
    ) -> Result<super::Parts, Error> {
        Demand::decode_owned(
            &self.canonical,
            contract,
            checked_fingerprint,
            checked_payload,
        )
    }

    fn replay<C: crate::profile::Checked>(
        &self,
        contract: &Contract,
        checked: &C,
        chain: node::Chain,
    ) -> Result<Demand, Error> {
        Demand::decode(&self.canonical, contract, checked, chain)
    }
}

impl node::Value for Product {
    const KIND: node::Kind = node::Kind::ProfileProof;
    const SCHEMA: u32 = 1;

    fn encode(&self) -> Result<Box<[u8]>, node::Error> {
        Ok(self.canonical.clone())
    }

    fn decode(bytes: &[u8]) -> Result<Self, node::Error> {
        if bytes.is_empty() {
            return Err(node::Error::Codec("cached profile demand is empty"));
        }
        Ok(Self {
            canonical: bytes.to_vec().into_boxed_slice(),
        })
    }
}

pub(crate) struct GeneratedProduct {
    canonical: Box<[u8]>,
}

impl GeneratedProduct {
    pub(crate) fn capture(proof: &crate::profile::Proof) -> Result<Self, Error> {
        Ok(Self {
            canonical: proof.encode_owner()?,
        })
    }

    pub(crate) fn owned(
        &self,
        demand: &Demand,
        product_lifecycle: bool,
    ) -> Result<crate::profile::Parts, Error> {
        crate::profile::Proof::decode_owned(&self.canonical, demand, product_lifecycle)
    }

    fn replay(
        &self,
        demand: &Demand,
        product_lifecycle: bool,
        chain: node::Chain,
    ) -> Result<crate::profile::Proof, Error> {
        crate::profile::Proof::decode_owner(&self.canonical, demand, product_lifecycle, chain)
    }
}

impl node::Value for GeneratedProduct {
    const KIND: node::Kind = node::Kind::ProfileProof;
    const SCHEMA: u32 = 2;

    fn encode(&self) -> Result<Box<[u8]>, node::Error> {
        Ok(self.canonical.clone())
    }

    fn decode(bytes: &[u8]) -> Result<Self, node::Error> {
        if bytes.is_empty() {
            return Err(node::Error::Codec(
                "cached generated profile proof is empty",
            ));
        }
        Ok(Self {
            canonical: bytes.to_vec().into_boxed_slice(),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::node::Value as _;

    use super::{GeneratedProduct, Product};

    // Golden hashes freeze the v15 semantic demand and generated authority encodings.
    // Exact graph observations select a distinct conservative demand recipe.
    #[test]
    fn captured_authorities_separate_semantic_and_exact_graph_inputs() {
        use sha2::{Digest as _, Sha256};

        let identity = crate::profile::Identity([1; 32]);
        let version = crate::profile::Version::new(7);
        let demand = super::demand_authority(identity, version, &[2; 32], &[3; 32], b"abc", None)
            .expect("bounded demand authority");
        assert_eq!(demand.len(), 167);
        assert_eq!(
            format!("{:x}", Sha256::digest(&demand)),
            "887525e1ec00cc72a58391c0983e3fc89b44f9ea7dd4d52f00b4d27280b3ef7d",
        );
        let exact = crate::api::Fingerprint::from_bytes([5; 32]);
        let conservative =
            super::demand_authority(identity, version, &[2; 32], &[3; 32], b"abc", Some(&exact))
                .unwrap();
        assert_ne!(demand, conservative);
        assert_ne!(
            conservative,
            super::demand_authority(
                identity,
                version,
                &[2; 32],
                &[3; 32],
                b"abc",
                Some(&crate::api::Fingerprint::from_bytes([6; 32])),
            )
            .unwrap()
        );
        let generated =
            super::generated_authority(identity, version, &[2; 32], &[3; 32], &[4; 32], true)
                .expect("generated authority");
        assert_eq!(generated.len(), 162);
        assert_eq!(
            format!("{:x}", Sha256::digest(&generated)),
            "38b2f5c393851ac3d5502582d19e5e7b079485910692eb8ce9108d1c9f4ff149",
        );
        let other_lifecycle =
            super::generated_authority(identity, version, &[2; 32], &[3; 32], &[4; 32], false)
                .expect("generated authority");
        assert_ne!(generated, other_lifecycle);
    }

    #[test]
    fn profile_demand_product_roundtrips_and_rejects_empty_payload() {
        let product = Product {
            canonical: br#"{"schema":7}"#.to_vec().into_boxed_slice(),
        };
        let bytes = product.encode().expect("the demand product encodes");
        assert!(Product::decode(&bytes).is_ok());
        assert!(Product::decode(&[]).is_err());
    }

    #[test]
    fn generated_profile_product_roundtrips_and_rejects_empty_payload() {
        let product = GeneratedProduct {
            canonical: br#"{"schema":9}"#.to_vec().into_boxed_slice(),
        };
        let bytes = product
            .encode()
            .expect("the generated profile product encodes");
        assert!(GeneratedProduct::decode(&bytes).is_ok());
        assert!(GeneratedProduct::decode(&[]).is_err());
    }
}
