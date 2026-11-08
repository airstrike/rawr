//! Checked-program-bound profile hook demand and generation.

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::{contract, Contract, Error, Identity, Proof, Version};

#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod cached;

/// One exact demanded profile hook bound to compiler evidence.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct Hook {
    evidence: contract::Evidence,
    identity: [u8; 32],
}

/// Sealed profile demand bound to classified and checked evidence.
pub struct Demand {
    identity: Identity,
    version: Version,
    rules: [u8; 32],
    checked: [u8; 32],
    payload: Box<[u8]>,
    hooks: Box<[Hook]>,
    aliases: Box<[contract::AliasTarget]>,
    sources: Box<[contract::ProjectionSourceSite]>,
    abis: Box<[contract::ExtensionAbi]>,
    endpoints: Box<[contract::OperationEndpoint]>,
    projections: Box<[contract::Projection]>,
    digest: [u8; 32],
    chain: Option<crate::node_parts::Chain>,
}

/// Canonically validated owned demand, without current graph authority.
pub(crate) struct Parts(Demand);

impl Parts {
    pub(crate) const fn value(&self) -> &Demand {
        &self.0
    }

    pub(crate) fn into_current(self, chain: crate::node_parts::Chain) -> Demand {
        Demand {
            chain: Some(chain),
            ..self.0
        }
    }
}

/// Contract-bound builder for demanded semantic hooks.
pub struct Builder<'contract> {
    contract: &'contract Contract,
    checked: [u8; 32],
    payload: Box<[u8]>,
    hooks: Vec<Hook>,
    aliases: Vec<contract::AliasTarget>,
    sources: Vec<contract::ProjectionSourceSite>,
    abis: Vec<contract::ExtensionAbi>,
    endpoints: Vec<contract::OperationEndpoint>,
    projections: Vec<contract::Projection>,
}

/// Demand-bound output for exact generated profile projections.
pub struct Generator<'demand> {
    demand: &'demand Demand,
    chunks: Vec<Chunk>,
    bytes: usize,
}

struct Chunk {
    hook: Hook,
    source: Box<[u8]>,
}

#[derive(Serialize)]
struct Canonical<'a> {
    schema: u32,
    identity: &'a [u8; 32],
    version: u32,
    rules: &'a [u8; 32],
    checked: &'a [u8; 32],
    payload: &'a [u8],
    hooks: &'a [Hook],
    aliases: &'a [contract::AliasTarget],
    sources: &'a [contract::ProjectionSourceSite],
    abis: &'a [contract::ExtensionAbi],
    endpoints: &'a [contract::OperationEndpoint],
    projections: &'a [contract::Projection],
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredCanonical {
    schema: u32,
    identity: [u8; 32],
    version: u32,
    rules: [u8; 32],
    checked: [u8; 32],
    payload: Box<[u8]>,
    hooks: Box<[Hook]>,
    aliases: Box<[contract::AliasTarget]>,
    sources: Box<[contract::ProjectionSourceSite]>,
    abis: Box<[contract::ExtensionAbi]>,
    endpoints: Box<[contract::OperationEndpoint]>,
    projections: Box<[contract::Projection]>,
}

impl Demand {
    pub(super) fn encode(&self) -> Result<Box<[u8]>, Error> {
        let canonical = serde_json::to_vec(&Canonical {
            schema: 7,
            identity: self.identity.as_bytes(),
            version: self.version.get(),
            rules: &self.rules,
            checked: &self.checked,
            payload: &self.payload,
            hooks: &self.hooks,
            aliases: &self.aliases,
            sources: &self.sources,
            abis: &self.abis,
            endpoints: &self.endpoints,
            projections: &self.projections,
        })
        .expect("a profile demand serializes");
        assert_eq!(
            <[u8; 32]>::from(Sha256::digest(&canonical)),
            self.digest,
            "a profile demand encodes to the bytes it was sealed from"
        );
        Ok(canonical.into_boxed_slice())
    }

    pub(super) fn decode<C: super::Checked>(
        bytes: &[u8],
        contract: &Contract,
        checked: &C,
        chain: crate::node_parts::Chain,
    ) -> Result<Self, Error> {
        Self::decode_owned(
            bytes,
            contract,
            checked.profile_fingerprint(),
            checked.profile_payload(),
        )
        .map(|parts| parts.into_current(chain))
    }

    pub(crate) fn decode_owned(
        bytes: &[u8],
        contract: &Contract,
        checked_fingerprint: [u8; 32],
        checked_payload: &[u8],
    ) -> Result<Parts, Error> {
        let stored: StoredCanonical = serde_json::from_slice(bytes).map_err(|_| Error::Stale)?;
        let canonical = serde_json::to_vec(&stored).expect("a profile demand serializes");
        if canonical.as_slice() != bytes
            || stored.schema != 7
            || stored.identity != *contract.identity().as_bytes()
            || stored.version != contract.version().get()
            || stored.rules != *contract.rules_digest()
            || stored.checked != checked_fingerprint
            || stored.payload.as_ref() != checked_payload
            || stored.hooks.windows(2).any(|pair| pair[0] >= pair[1])
            || stored
                .aliases
                .windows(2)
                .any(|pair| pair[0].selector() >= pair[1].selector())
            || stored
                .sources
                .windows(2)
                .any(|pair| pair[0].selector() >= pair[1].selector())
            || stored
                .abis
                .windows(2)
                .any(|pair| pair[0].selector() >= pair[1].selector())
            || stored
                .endpoints
                .windows(2)
                .any(|pair| pair[0].selector() >= pair[1].selector())
            || stored
                .projections
                .windows(2)
                .any(|pair| pair[0].selector() >= pair[1].selector())
            || stored
                .hooks
                .iter()
                .any(|hook| !contract.contains(&hook.evidence))
            || stored
                .aliases
                .iter()
                .any(|alias| !contract.contains_alias_target(alias))
            || stored
                .sources
                .iter()
                .any(|source| !contract.contains_projection_source(source))
            || stored.abis.iter().any(|abi| !contract.contains_abi(abi))
            || stored
                .endpoints
                .iter()
                .any(|endpoint| !contract.contains_endpoint(endpoint))
            || stored
                .projections
                .iter()
                .any(|projection| !contract.contains_projection(projection))
        {
            return Err(Error::Stale);
        }
        Ok(Parts(Self {
            identity: contract.identity(),
            version: contract.version(),
            rules: stored.rules,
            checked: stored.checked,
            payload: stored.payload,
            hooks: stored.hooks,
            aliases: stored.aliases,
            sources: stored.sources,
            abis: stored.abis,
            endpoints: stored.endpoints,
            projections: stored.projections,
            digest: Sha256::digest(bytes).into(),
            chain: None,
        }))
    }

    /// Borrow the current demand node for generated-proof joins.
    #[doc(hidden)]
    #[must_use]
    pub const fn node_chain(&self) -> Option<&crate::node_parts::Chain> {
        self.chain.as_ref()
    }

    /// Return the exact profile identity.
    #[must_use]
    pub const fn identity(&self) -> Identity {
        self.identity
    }

    /// Return the exact semantic profile rule version.
    #[must_use]
    pub const fn version(&self) -> Version {
        self.version
    }

    /// Iterate demanded semantic hooks in canonical order.
    pub fn hooks(&self) -> impl ExactSizeIterator<Item = &Hook> {
        self.hooks.iter()
    }

    /// Iterate demanded support-only alias targets in canonical selector order.
    pub fn alias_targets(&self) -> impl ExactSizeIterator<Item = &contract::AliasTarget> {
        self.aliases.iter()
    }

    /// Iterate demanded support-only projection source sites.
    pub fn projection_sources(
        &self,
    ) -> impl ExactSizeIterator<Item = &contract::ProjectionSourceSite> {
        self.sources.iter()
    }

    /// Iterate demanded support-only extension ABI proofs.
    pub fn extension_abis(&self) -> impl ExactSizeIterator<Item = &contract::ExtensionAbi> {
        self.abis.iter()
    }

    /// Iterate demanded support-only operation endpoints.
    pub fn operation_endpoints(
        &self,
    ) -> impl ExactSizeIterator<Item = &contract::OperationEndpoint> {
        self.endpoints.iter()
    }

    /// Iterate demanded direct projection pairs in canonical selector order.
    pub fn projections(&self) -> impl ExactSizeIterator<Item = &contract::Projection> {
        self.projections.iter()
    }

    /// Borrow the complete demand digest.
    #[must_use]
    pub const fn digest(&self) -> &[u8; 32] {
        &self.digest
    }

    /// Borrow opaque checked-profile data retained for deterministic generation.
    #[doc(hidden)]
    pub fn checked_payload(&self) -> &[u8] {
        &self.payload
    }

    /// Borrow the exact classified profile-rules digest.
    #[doc(hidden)]
    pub const fn rules_digest(&self) -> &[u8; 32] {
        &self.rules
    }

    pub(crate) const fn checked_fingerprint(&self) -> &[u8; 32] {
        &self.checked
    }

    pub(crate) fn contains_alias_target(&self, alias: &contract::AliasTarget) -> bool {
        self.aliases
            .binary_search_by_key(alias.selector(), |candidate| *candidate.selector())
            .is_ok_and(|index| canonical_equal(&self.aliases[index], alias))
    }

    pub(crate) fn contains_projection_source(
        &self,
        source: &contract::ProjectionSourceSite,
    ) -> bool {
        self.sources
            .binary_search_by_key(source.selector(), |candidate| *candidate.selector())
            .is_ok_and(|index| canonical_equal(&self.sources[index], source))
    }

    pub(crate) fn contains_abi(&self, abi: &contract::ExtensionAbi) -> bool {
        self.abis
            .binary_search_by_key(abi.selector(), |candidate| *candidate.selector())
            .is_ok_and(|index| canonical_equal(&self.abis[index], abi))
    }

    pub(crate) fn contains_endpoint(&self, endpoint: &contract::OperationEndpoint) -> bool {
        self.endpoints
            .binary_search_by_key(endpoint.selector(), |candidate| *candidate.selector())
            .is_ok_and(|index| canonical_equal(&self.endpoints[index], endpoint))
    }

    pub(crate) fn contains_projection(&self, projection: &contract::Projection) -> bool {
        self.projections.binary_search(projection).is_ok()
    }
}

fn canonical_equal<T: Serialize>(left: &T, right: &T) -> bool {
    serde_json::to_vec(left)
        .ok()
        .zip(serde_json::to_vec(right).ok())
        .is_some_and(|(left, right)| left == right)
}

impl Hook {
    /// Borrow the exact graph-origin evidence for this hook.
    #[must_use]
    pub const fn evidence(&self) -> &contract::Evidence {
        &self.evidence
    }

    /// Borrow the semantic hook identity.
    #[must_use]
    pub const fn identity(&self) -> &[u8; 32] {
        &self.identity
    }
}

impl<'contract> Builder<'contract> {
    pub(crate) fn new(
        contract: &'contract Contract,
        checked: [u8; 32],
        payload: &[u8],
    ) -> Result<Self, Error> {
        // The reservation below is the physical bound.
        let mut retained = Vec::new();
        retained
            .try_reserve_exact(payload.len())
            .map_err(|_| Error::Allocation)?;
        retained.extend_from_slice(payload);
        Ok(Self {
            contract,
            checked,
            payload: retained.into_boxed_slice(),
            hooks: Vec::new(),
            aliases: Vec::new(),
            sources: Vec::new(),
            abis: Vec::new(),
            endpoints: Vec::new(),
            projections: Vec::new(),
        })
    }

    /// Demand one named hook derived from classified compiler evidence.
    pub fn hook(&mut self, evidence: contract::Evidence, name: &str) -> Result<Hook, Error> {
        assert!(
            self.contract.contains(&evidence) && !name.is_empty(),
            "a profile demands named hooks on its own contract's evidence"
        );
        let mut digest = Sha256::new();
        digest.update(b"rawr.import.profile.hook/v1");
        digest.update(evidence.digest());
        digest.update(u64::try_from(name.len()).unwrap_or(u64::MAX).to_le_bytes());
        digest.update(name.as_bytes());
        let hook = Hook {
            evidence,
            identity: digest.finalize().into(),
        };
        match self.hooks.binary_search(&hook) {
            Ok(_) => panic!("a profile demands each hook once"),
            Err(position) => {
                self.hooks
                    .try_reserve_exact(1)
                    .map_err(|_| Error::Allocation)?;
                self.hooks.insert(position, hook);
                Ok(hook)
            }
        }
    }

    pub(crate) fn seal(self) -> Result<Demand, Error> {
        let canonical = serde_json::to_vec(&Canonical {
            schema: 7,
            identity: self.contract.identity().as_bytes(),
            version: self.contract.version().get(),
            rules: self.contract.rules_digest(),
            checked: &self.checked,
            payload: &self.payload,
            hooks: &self.hooks,
            aliases: &self.aliases,
            sources: &self.sources,
            abis: &self.abis,
            endpoints: &self.endpoints,
            projections: &self.projections,
        })
        .expect("a profile demand serializes");
        Ok(Demand {
            identity: self.contract.identity(),
            version: self.contract.version(),
            rules: *self.contract.rules_digest(),
            checked: self.checked,
            payload: self.payload,
            hooks: self.hooks.into_boxed_slice(),
            aliases: self.aliases.into_boxed_slice(),
            sources: self.sources.into_boxed_slice(),
            abis: self.abis.into_boxed_slice(),
            endpoints: self.endpoints.into_boxed_slice(),
            projections: self.projections.into_boxed_slice(),
            digest: Sha256::digest(canonical).into(),
            chain: None,
        })
    }
}

impl<'demand> Generator<'demand> {
    pub(crate) fn new(demand: &'demand Demand) -> Self {
        Self {
            demand,
            chunks: Vec::new(),
            bytes: 0,
        }
    }

    /// Emit one bounded Rust projection for an exact demanded hook.
    ///
    /// The reservations are the physical bound and checked-arithmetic
    /// overflow is terminal.
    pub fn projection(&mut self, hook: Hook, source: &[u8]) -> Result<(), Error> {
        assert!(
            self.demand.hooks.binary_search(&hook).is_ok() && !source.is_empty(),
            "a profile generates source for its own demanded hooks"
        );
        assert!(
            self.chunks.iter().all(|chunk| chunk.hook != hook),
            "a profile generates each hook's source once"
        );
        let actual = self
            .bytes
            .checked_add(source.len())
            .ok_or(Error::Allocation)?;
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(source.len())
            .map_err(|_| Error::Allocation)?;
        owned.extend_from_slice(source);
        self.chunks
            .try_reserve_exact(1)
            .map_err(|_| Error::Allocation)?;
        self.chunks.push(Chunk {
            hook,
            source: owned.into_boxed_slice(),
        });
        self.bytes = actual;
        Ok(())
    }

    pub(crate) fn seal(mut self, product_lifecycle: bool) -> Result<Proof, Error> {
        self.chunks.sort_unstable_by_key(|chunk| chunk.hook);
        assert_eq!(
            self.chunks.len(),
            self.demand.hooks.len(),
            "a profile generates source for every hook it demanded"
        );
        let framing = self
            .chunks
            .len()
            .checked_mul(1 + 32 + 8)
            .ok_or(Error::Allocation)?;
        let capacity = self.bytes.checked_add(framing).ok_or(Error::Allocation)?;
        let mut source = Vec::new();
        source
            .try_reserve_exact(capacity)
            .map_err(|_| Error::Allocation)?;
        for chunk in self.chunks {
            source.extend_from_slice(b"\n");
            source.extend_from_slice(&chunk.source);
            source.extend_from_slice(b"\n");
        }
        let mut projections = Vec::new();
        projections
            .try_reserve_exact(self.demand.projections.len())
            .map_err(|_| Error::Allocation)?;
        projections.extend_from_slice(&self.demand.projections);
        let mut aliases = Vec::new();
        aliases
            .try_reserve_exact(self.demand.aliases.len())
            .map_err(|_| Error::Allocation)?;
        for alias in &self.demand.aliases {
            aliases.push(alias.try_copy()?);
        }
        let mut sources = Vec::new();
        sources
            .try_reserve_exact(self.demand.sources.len())
            .map_err(|_| Error::Allocation)?;
        for source in &self.demand.sources {
            sources.push(source.try_copy()?);
        }
        let mut abis = Vec::new();
        abis.try_reserve_exact(self.demand.abis.len())
            .map_err(|_| Error::Allocation)?;
        for abi in &self.demand.abis {
            abis.push(abi.try_copy()?);
        }
        let mut endpoints = Vec::new();
        endpoints
            .try_reserve_exact(self.demand.endpoints.len())
            .map_err(|_| Error::Allocation)?;
        for endpoint in &self.demand.endpoints {
            endpoints.push(endpoint.try_copy()?);
        }
        let application = super::application_semantic(self.demand)
            .expect("a checked program's profile payload is its application identity");
        let canonical = serde_json::to_vec(&super::OwnerCanonical {
            schema: 9,
            identity: self.demand.identity.as_bytes(),
            version: self.demand.version.get(),
            rules: &self.demand.rules,
            product_lifecycle,
            source: &source,
            aliases: &aliases,
            sources: &sources,
            abis: &abis,
            endpoints: &endpoints,
            projections: &projections,
        })
        .expect("a generated profile proof serializes");
        let owner = super::owner_digest(&canonical);
        Ok(Proof {
            digest: super::proof_digest(self.demand, owner, application, product_lifecycle),
            owner,
            application,
            identity: self.demand.identity,
            version: self.demand.version,
            rules: self.demand.rules,
            product_lifecycle,
            source: source.into_boxed_slice(),
            aliases: aliases.into_boxed_slice(),
            sources: sources.into_boxed_slice(),
            abis: abis.into_boxed_slice(),
            endpoints: endpoints.into_boxed_slice(),
            projections: projections.into_boxed_slice(),
            chain: None,
        })
    }
}

#[cfg(test)]
mod owned_tests {
    use super::{Builder, Demand, Generator};
    use crate::profile::{contract, Contract, Identity, Proof, Version};

    fn contract() -> Contract {
        let graph = crate::test_fixture::graph();
        contract::Builder::new(
            Identity::new("rawr.profile.owned-demand-test"),
            Version::new(1),
            &graph,
        )
        .seal()
        .expect("empty contract")
    }

    #[test]
    fn owned_demand_replays_exact_captured_checked_data_without_current_authority() {
        let contract = contract();
        let demand = Builder::new(&contract, [7; 32], b"checked payload")
            .expect("builder")
            .seal()
            .expect("demand");
        let bytes = demand.encode().expect("canonical demand");
        let parts = Demand::decode_owned(&bytes, &contract, [7; 32], b"checked payload")
            .expect("owned replay");
        assert!(parts.value().node_chain().is_none());
        assert_eq!(parts.value().encode().expect("reencode"), bytes);
        assert_eq!(parts.value().digest(), demand.digest());
        assert!(Demand::decode_owned(&bytes, &contract, [8; 32], b"checked payload").is_err());
        assert!(Demand::decode_owned(&bytes, &contract, [7; 32], b"other payload").is_err());
        let mut noncanonical = bytes.to_vec();
        noncanonical.push(b' ');
        assert!(
            Demand::decode_owned(&noncanonical, &contract, [7; 32], b"checked payload",).is_err()
        );
    }

    #[test]
    fn owned_generated_replay_preserves_digests_and_rejects_lifecycle_and_version_drift() {
        let contract = contract();
        let demand = Builder::new(&contract, [7; 32], &[7; 32])
            .expect("builder")
            .seal()
            .expect("demand");
        for lifecycle in [false, true] {
            let proof = Generator::new(&demand).seal(lifecycle).expect("proof");
            let bytes = proof.encode_owner().expect("canonical owner");
            let parts = Proof::decode_owned(&bytes, &demand, lifecycle).expect("owned replay");
            assert!(parts.value().node_chain().is_none());
            assert_eq!(parts.value().encode_owner().expect("reencode"), bytes);
            assert_eq!(parts.value().fingerprint(), proof.fingerprint());
            assert_eq!(parts.value().owner_fingerprint(), proof.owner_fingerprint());
            assert_eq!(
                parts.value().application_semantic(),
                proof.application_semantic()
            );
            assert!(Proof::decode_owned(&bytes, &demand, !lifecycle).is_err());
            let mut stored: super::super::StoredOwnerCanonical =
                serde_json::from_slice(&bytes).expect("stored owner");
            stored.version += 1;
            let foreign = serde_json::to_vec(&stored).expect("foreign canonical owner");
            assert!(Proof::decode_owned(&foreign, &demand, lifecycle).is_err());
            let mut noncanonical = bytes.to_vec();
            noncanonical.push(b' ');
            assert!(Proof::decode_owned(&noncanonical, &demand, lifecycle).is_err());
        }
    }
}
