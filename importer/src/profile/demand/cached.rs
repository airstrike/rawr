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
    limits: &crate::cargo::Limits,
) -> Result<Demand, Error> {
    let parent = checked
        .node_chain()
        .or_else(|| contract.node_chain())
        .cloned()
        .ok_or(Error::ForeignEvidence)?;
    if parent.output_limit() != limits.node_output_bytes()
        || definition.identity() != contract.identity()
        || definition.version() != contract.version()
    {
        return Err(Error::DefinitionDrift);
    }
    let checked_fingerprint = checked.profile_fingerprint();
    let payload = checked.profile_payload();
    // Preserve the callback order: the payload target is recorded before
    // asking an arbitrary definition for its authority fields again.
    validate_payload(payload, limits)?;
    let authority = demand_authority(
        definition.identity(),
        definition.version(),
        contract.rules_digest(),
        &checked_fingerprint,
        payload,
        limits,
        Some(&contract.graph_fingerprint()),
    )?;
    let product = parent
        .direct::<Product>(&authority, || {
            let demand = crate::profile::demand_direct(definition, contract, checked, limits)
                .map_err(|error| node::Error::Transform(error.to_string().into_boxed_str()))?;
            Product::capture(&demand, limits.node_output_bytes())
                .map_err(|error| node::Error::Transform(error.to_string().into_boxed_str()))
        })
        .map_err(|error| Error::Node(Box::new(error)))?;
    let chain = parent.child(&product);
    product.value().replay(contract, checked, chain, limits)
}

pub(crate) fn generate<C: crate::profile::Checked, D: crate::profile::Definition<C>>(
    definition: &D,
    demand: &Demand,
    limits: &crate::cargo::Limits,
) -> Result<crate::profile::Proof, Error> {
    let parent = demand.node_chain().cloned().ok_or(Error::ForeignEvidence)?;
    if parent.output_limit() != limits.node_output_bytes()
        || definition.identity() != demand.identity()
        || definition.version() != demand.version()
    {
        return Err(Error::DefinitionDrift);
    }
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
            let proof = crate::profile::Proof::generate_direct(definition, demand, limits)
                .map_err(|error| node::Error::Transform(error.to_string().into_boxed_str()))?;
            GeneratedProduct::capture(&proof, limits.node_output_bytes())
                .map_err(|error| node::Error::Transform(error.to_string().into_boxed_str()))
        })
        .map_err(|error| Error::Node(Box::new(error)))?;
    let chain = parent.child(&product);
    product
        .value()
        .replay(demand, product_lifecycle, chain, limits)
}

/// Encode the existing demand identity from captured, immutable inputs.
pub(crate) fn demand_authority(
    identity: crate::profile::Identity,
    version: crate::profile::Version,
    rules_digest: &[u8; 32],
    checked_fingerprint: &[u8; 32],
    payload: &[u8],
    limits: &crate::cargo::Limits,
    exact_graph: Option<&crate::api::Fingerprint>,
) -> Result<Vec<u8>, Error> {
    validate_payload(payload, limits)?;
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

/// The payload size is an accounting target recorded on a profile account;
/// it never refuses. Kept as a `Result` so the callback order stays explicit.
fn validate_payload(payload: &[u8], limits: &crate::cargo::Limits) -> Result<(), Error> {
    crate::surface::account::Account::profile().exceeded(
        "profile checked payload bytes",
        limits.string_bytes,
        payload.len(),
    );
    Ok(())
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
    pub(crate) fn capture(demand: &Demand, limit: usize) -> Result<Self, Error> {
        Ok(Self {
            canonical: demand.encode(limit)?,
        })
    }

    pub(crate) fn owned(
        &self,
        contract: &Contract,
        checked_fingerprint: [u8; 32],
        checked_payload: &[u8],
        limits: &crate::cargo::Limits,
    ) -> Result<super::Parts, Error> {
        Demand::decode_owned(
            &self.canonical,
            contract,
            checked_fingerprint,
            checked_payload,
            limits,
        )
    }

    fn replay<C: crate::profile::Checked>(
        &self,
        contract: &Contract,
        checked: &C,
        chain: node::Chain,
        limits: &crate::cargo::Limits,
    ) -> Result<Demand, Error> {
        Demand::decode(&self.canonical, contract, checked, chain, limits)
    }
}

impl node::Value for Product {
    const KIND: node::Kind = node::Kind::ProfileProof;
    const SCHEMA: u32 = 1;

    fn encode(&self, limit: usize) -> Result<Box<[u8]>, node::Error> {
        if self.canonical.len() > limit {
            return Err(node::Error::Limit {
                resource: "cached profile demand bytes",
                limit,
                actual: self.canonical.len(),
            });
        }
        Ok(self.canonical.clone())
    }

    fn decode(bytes: &[u8], limit: usize) -> Result<Self, node::Error> {
        if bytes.len() > limit {
            return Err(node::Error::Limit {
                resource: "cached profile demand bytes",
                limit,
                actual: bytes.len(),
            });
        }
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
    pub(crate) fn capture(proof: &crate::profile::Proof, limit: usize) -> Result<Self, Error> {
        Ok(Self {
            canonical: proof.encode_owner(limit)?,
        })
    }

    pub(crate) fn owned(
        &self,
        demand: &Demand,
        product_lifecycle: bool,
        limits: &crate::cargo::Limits,
    ) -> Result<crate::profile::Parts, Error> {
        crate::profile::Proof::decode_owned(&self.canonical, demand, product_lifecycle, limits)
    }

    fn replay(
        &self,
        demand: &Demand,
        product_lifecycle: bool,
        chain: node::Chain,
        limits: &crate::cargo::Limits,
    ) -> Result<crate::profile::Proof, Error> {
        crate::profile::Proof::decode_owner(
            &self.canonical,
            demand,
            product_lifecycle,
            chain,
            limits,
        )
    }
}

impl node::Value for GeneratedProduct {
    const KIND: node::Kind = node::Kind::ProfileProof;
    const SCHEMA: u32 = 2;

    fn encode(&self, limit: usize) -> Result<Box<[u8]>, node::Error> {
        if self.canonical.len() > limit {
            return Err(node::Error::Limit {
                resource: "cached generated profile proof bytes",
                limit,
                actual: self.canonical.len(),
            });
        }
        Ok(self.canonical.clone())
    }

    fn decode(bytes: &[u8], limit: usize) -> Result<Self, node::Error> {
        if bytes.len() > limit {
            return Err(node::Error::Limit {
                resource: "cached generated profile proof bytes",
                limit,
                actual: bytes.len(),
            });
        }
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

        let limits = crate::cargo::Limits::standard();
        let identity = crate::profile::Identity([1; 32]);
        let version = crate::profile::Version::new(7).expect("nonzero version");
        let demand =
            super::demand_authority(identity, version, &[2; 32], &[3; 32], b"abc", &limits, None)
                .expect("bounded demand authority");
        assert_eq!(demand.len(), 167);
        assert_eq!(
            format!("{:x}", Sha256::digest(&demand)),
            "887525e1ec00cc72a58391c0983e3fc89b44f9ea7dd4d52f00b4d27280b3ef7d",
        );
        let exact = crate::api::Fingerprint::from_bytes([5; 32]);
        let conservative = super::demand_authority(
            identity,
            version,
            &[2; 32],
            &[3; 32],
            b"abc",
            &limits,
            Some(&exact),
        )
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
                &limits,
                Some(&crate::api::Fingerprint::from_bytes([6; 32]))
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

    /// The payload string target is accounting. Exactly at the target
    /// and one over both produce the same authority bytes; nothing refuses.
    #[test]
    fn captured_demand_authority_records_payload_target_without_refusal() {
        let mut limits = crate::cargo::Limits::standard();
        limits.string_bytes = 3;
        let identity = crate::profile::Identity([1; 32]);
        let version = crate::profile::Version::new(7).expect("nonzero version");
        let exact =
            super::demand_authority(identity, version, &[2; 32], &[3; 32], b"abc", &limits, None)
                .expect("the exact payload target admits");
        let over = super::demand_authority(
            identity, version, &[2; 32], &[3; 32], b"abcd", &limits, None,
        )
        .expect("a payload past the string target is recorded, not refused");
        let standard = super::demand_authority(
            identity,
            version,
            &[2; 32],
            &[3; 32],
            b"abcd",
            &crate::cargo::Limits::standard(),
            None,
        )
        .expect("the standard target admits");
        assert_eq!(over, standard);
        assert_ne!(over, exact);
    }

    #[test]
    fn profile_demand_product_roundtrips_and_rejects_empty_payload() {
        let product = Product {
            canonical: br#"{"schema":7}"#.to_vec().into_boxed_slice(),
        };
        let bytes = product.encode(1024).expect("the demand product encodes");
        assert!(Product::decode(&bytes, 1024).is_ok());
        assert!(Product::decode(&[], 1024).is_err());
        assert!(Product::decode(&bytes, 1).is_err());
    }

    #[test]
    fn generated_profile_product_roundtrips_and_rejects_empty_payload() {
        let product = GeneratedProduct {
            canonical: br#"{"schema":9}"#.to_vec().into_boxed_slice(),
        };
        let bytes = product
            .encode(1024)
            .expect("the generated profile product encodes");
        assert!(GeneratedProduct::decode(&bytes, 1024).is_ok());
        assert!(GeneratedProduct::decode(&[], 1024).is_err());
        assert!(GeneratedProduct::decode(&bytes, 1).is_err());
    }
}
