//! Checked-program-bound profile hook demand and generation.

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::{contract, Contract, Error, Identity, Proof, Version};

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
    chain: Option<crate::node::Chain>,
}

/// Canonically validated owned demand, without current graph authority.
pub(crate) struct Parts(Demand);

impl Parts {
    pub(crate) const fn value(&self) -> &Demand {
        &self.0
    }

    pub(crate) fn into_current(self, chain: crate::node::Chain) -> Demand {
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
    item_limit: usize,
    byte_limit: usize,
    string_limit: usize,
    account: crate::surface::account::Account,
}

/// Demand-bound output for exact generated profile projections.
pub struct Generator<'demand> {
    demand: &'demand Demand,
    chunks: Vec<Chunk>,
    byte_limit: usize,
    string_limit: usize,
    bytes: usize,
    account: crate::surface::account::Account,
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
    pub(super) fn encode(&self, limit: usize) -> Result<Box<[u8]>, Error> {
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
        .map_err(|_| Error::Encoding)?;
        if canonical.len() > limit {
            return Err(Error::Limit {
                resource: "profile demand bytes",
                limit,
                actual: canonical.len(),
            });
        }
        if <[u8; 32]>::from(Sha256::digest(&canonical)) != self.digest {
            return Err(Error::Canonical);
        }
        Ok(canonical.into_boxed_slice())
    }

    pub(super) fn decode<C: super::Checked>(
        bytes: &[u8],
        contract: &Contract,
        checked: &C,
        chain: crate::node::Chain,
        limits: &crate::cargo::Limits,
    ) -> Result<Self, Error> {
        Self::decode_owned(
            bytes,
            contract,
            checked.profile_fingerprint(),
            checked.profile_payload(),
            limits,
        )
        .map(|parts| parts.into_current(chain))
    }

    pub(crate) fn decode_owned(
        bytes: &[u8],
        contract: &Contract,
        checked_fingerprint: [u8; 32],
        checked_payload: &[u8],
        limits: &crate::cargo::Limits,
    ) -> Result<Parts, Error> {
        // Document, payload and item sizes are accounting targets
        // recorded on a profile account for this replay; the physical bound is
        // the node content ceiling that already admitted the bytes.
        let mut account = crate::surface::account::Account::profile();
        Self::decode_owned_accounted(
            bytes,
            contract,
            checked_fingerprint,
            checked_payload,
            limits,
            &mut account,
        )
    }

    pub(crate) fn decode_owned_accounted(
        bytes: &[u8],
        contract: &Contract,
        checked_fingerprint: [u8; 32],
        checked_payload: &[u8],
        limits: &crate::cargo::Limits,
        account: &mut crate::surface::account::Account,
    ) -> Result<Parts, Error> {
        account.exceeded("profile demand bytes", limits.json_bytes, bytes.len());
        let stored: StoredCanonical = serde_json::from_slice(bytes).map_err(|_| Error::Encoding)?;
        let canonical = serde_json::to_vec(&stored).map_err(|_| Error::Encoding)?;
        let items = stored
            .hooks
            .len()
            .checked_add(stored.aliases.len())
            .and_then(|items| items.checked_add(stored.sources.len()))
            .and_then(|items| items.checked_add(stored.abis.len()))
            .and_then(|items| items.checked_add(stored.endpoints.len()))
            .and_then(|items| items.checked_add(stored.projections.len()))
            .ok_or(Error::Limit {
                resource: "profile demand items",
                limit: limits.items,
                actual: usize::MAX,
            })?;
        account.exceeded(
            "profile checked payload bytes",
            limits.string_bytes,
            stored.payload.len(),
        );
        account.exceeded("profile demand items", limits.items, items);
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
            return Err(Error::Canonical);
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
    pub const fn node_chain(&self) -> Option<&crate::node::Chain> {
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
        limits: &crate::cargo::Limits,
    ) -> Result<Self, Error> {
        // The payload size is an accounting target recorded on this
        // demand's profile account; the reservation below is the physical bound.
        let mut account = crate::surface::account::Account::profile();
        account.exceeded(
            "profile checked payload bytes",
            limits.string_bytes,
            payload.len(),
        );
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
            item_limit: limits.items,
            byte_limit: limits.json_bytes,
            string_limit: limits.string_bytes,
            account,
        })
    }

    /// The accounting axes this demand crossed so far.
    #[doc(hidden)]
    #[must_use]
    pub const fn exceedances(&self) -> crate::exceedance::Record {
        self.account.exceedances()
    }

    /// Demand one named hook derived from classified compiler evidence.
    pub fn hook(&mut self, evidence: contract::Evidence, name: &str) -> Result<Hook, Error> {
        if !self.contract.contains(&evidence) || name.is_empty() {
            return Err(Error::ForeignEvidence);
        }
        // The name length is an accounting target, recorded not refused.
        self.account
            .exceeded("profile hook name bytes", self.string_limit, name.len());
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
            Ok(_) => Err(Error::Canonical),
            Err(position) => {
                let actual = self.hooks.len().checked_add(1).ok_or(Error::Limit {
                    resource: "profile hook items",
                    limit: self.item_limit,
                    actual: usize::MAX,
                })?;
                self.account
                    .exceeded("profile hook items", self.item_limit, actual);
                self.hooks
                    .try_reserve_exact(1)
                    .map_err(|_| Error::Allocation)?;
                self.hooks.insert(position, hook);
                Ok(hook)
            }
        }
    }

    /// Demand one exact compiler-classified direct projection pair.
    pub fn projection(&mut self, projection: &contract::Projection) -> Result<(), Error> {
        if !self.contract.contains_projection(projection) {
            return Err(Error::ForeignEvidence);
        }
        let alias = self
            .contract
            .alias_targets()
            .find(|candidate| candidate.selector() == projection.alias_selector())
            .ok_or(Error::ForeignEvidence)?;
        let source = self
            .contract
            .source(contract::SourceKey(*projection.source_selector()))
            .ok_or(Error::ForeignEvidence)?;
        match self
            .aliases
            .binary_search_by_key(alias.selector(), |candidate| *candidate.selector())
        {
            Ok(_) => {}
            Err(position) => {
                self.aliases
                    .try_reserve_exact(1)
                    .map_err(|_| Error::Allocation)?;
                self.aliases.insert(position, alias.try_copy()?);
            }
        }
        match self
            .sources
            .binary_search_by_key(source.selector(), |candidate| *candidate.selector())
        {
            Ok(_) => {}
            Err(position) => {
                self.sources
                    .try_reserve_exact(1)
                    .map_err(|_| Error::Allocation)?;
                self.sources.insert(position, source.try_copy()?);
            }
        }
        match self
            .projections
            .binary_search_by_key(projection.selector(), |candidate| *candidate.selector())
        {
            Ok(_) => Err(Error::Canonical),
            Err(position) => {
                let actual = self
                    .hooks
                    .len()
                    .checked_add(self.aliases.len())
                    .and_then(|items| items.checked_add(self.sources.len()))
                    .and_then(|items| items.checked_add(self.abis.len()))
                    .and_then(|items| items.checked_add(self.endpoints.len()))
                    .and_then(|items| items.checked_add(self.projections.len()))
                    .and_then(|items| items.checked_add(1))
                    .ok_or(Error::Limit {
                        resource: "profile demand items",
                        limit: self.item_limit,
                        actual: usize::MAX,
                    })?;
                self.account
                    .exceeded("profile demand items", self.item_limit, actual);
                self.projections
                    .try_reserve_exact(1)
                    .map_err(|_| Error::Allocation)?;
                self.projections.insert(position, projection.clone());
                Ok(())
            }
        }
    }

    /// Demand one exact support-only ABI proof already sealed by the profile.
    pub fn extension_abi(&mut self, abi: &contract::ExtensionAbi) -> Result<(), Error> {
        if !self.contract.contains_abi(abi) {
            return Err(Error::ForeignEvidence);
        }
        match self
            .abis
            .binary_search_by_key(abi.selector(), |candidate| *candidate.selector())
        {
            Ok(_) => Err(Error::Canonical),
            Err(position) => {
                let actual = self
                    .hooks
                    .len()
                    .checked_add(self.aliases.len())
                    .and_then(|items| items.checked_add(self.sources.len()))
                    .and_then(|items| items.checked_add(self.abis.len()))
                    .and_then(|items| items.checked_add(self.endpoints.len()))
                    .and_then(|items| items.checked_add(self.projections.len()))
                    .and_then(|items| items.checked_add(1))
                    .ok_or(Error::Limit {
                        resource: "profile ABI demand items",
                        limit: self.item_limit,
                        actual: usize::MAX,
                    })?;
                self.account
                    .exceeded("profile ABI demand items", self.item_limit, actual);
                self.abis
                    .try_reserve_exact(1)
                    .map_err(|_| Error::Allocation)?;
                self.abis.insert(position, abi.try_copy()?);
                Ok(())
            }
        }
    }

    /// Demand one exact support-only operation endpoint.
    pub fn operation_endpoint(
        &mut self,
        endpoint: &contract::OperationEndpoint,
    ) -> Result<(), Error> {
        if !self.contract.contains_endpoint(endpoint) {
            return Err(Error::ForeignEvidence);
        }
        match self
            .endpoints
            .binary_search_by_key(endpoint.selector(), |candidate| *candidate.selector())
        {
            Ok(_) => Err(Error::Canonical),
            Err(position) => {
                self.endpoints
                    .try_reserve_exact(1)
                    .map_err(|_| Error::Allocation)?;
                self.endpoints.insert(position, endpoint.try_copy()?);
                Ok(())
            }
        }
    }

    pub(crate) fn seal(mut self) -> Result<Demand, Error> {
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
        .map_err(|_| Error::Encoding)?;
        self.account
            .exceeded("profile demand bytes", self.byte_limit, canonical.len());
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
    pub(crate) fn new(demand: &'demand Demand, limits: &crate::cargo::Limits) -> Self {
        Self {
            demand,
            chunks: Vec::new(),
            byte_limit: limits.json_bytes,
            string_limit: limits.string_bytes,
            bytes: 0,
            account: crate::surface::account::Account::profile(),
        }
    }

    /// The accounting axes this generation crossed so far.
    #[doc(hidden)]
    #[must_use]
    pub const fn exceedances(&self) -> crate::exceedance::Record {
        self.account.exceedances()
    }

    /// Emit one bounded Rust projection for an exact demanded hook.
    ///
    /// Projection and cumulative byte sizes are accounting targets
    /// recorded on the profile account; the reservations are the physical
    /// bound and checked-arithmetic overflow stays terminal.
    pub fn projection(&mut self, hook: Hook, source: &[u8]) -> Result<(), Error> {
        if self.demand.hooks.binary_search(&hook).is_err() || source.is_empty() {
            return Err(Error::ForeignEvidence);
        }
        if self.chunks.iter().any(|chunk| chunk.hook == hook) {
            return Err(Error::Canonical);
        }
        self.account.exceeded(
            "generated profile projection bytes",
            self.string_limit,
            source.len(),
        );
        let actual = self.bytes.checked_add(source.len()).ok_or(Error::Limit {
            resource: "generated profile bytes",
            limit: self.byte_limit,
            actual: usize::MAX,
        })?;
        self.account
            .exceeded("generated profile bytes", self.byte_limit, actual);
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
        if self.chunks.len() != self.demand.hooks.len() {
            return Err(Error::ForeignEvidence);
        }
        let framing = self
            .chunks
            .len()
            .checked_mul(1 + 32 + 8)
            .ok_or(Error::Allocation)?;
        let capacity = self.bytes.checked_add(framing).ok_or(Error::Allocation)?;
        self.account
            .exceeded("generated profile bytes", self.byte_limit, capacity);
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
        let application = super::application_semantic(self.demand)?;
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
        .map_err(|_| Error::Encoding)?;
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

    fn contract(limits: &crate::cargo::Limits) -> Contract {
        let graph = crate::test_fixture::graph();
        contract::Builder::new(
            Identity::new("rawr.profile.owned-demand-test", limits).expect("identity"),
            Version::new(1).expect("version"),
            &graph,
            limits,
        )
        .seal()
        .expect("empty contract")
    }

    #[test]
    fn owned_demand_replays_exact_captured_checked_data_without_current_authority() {
        let limits = crate::cargo::Limits::standard();
        let contract = contract(&limits);
        let demand = Builder::new(&contract, [7; 32], b"checked payload", &limits)
            .expect("builder")
            .seal()
            .expect("demand");
        let bytes = demand.encode(limits.json_bytes).expect("canonical demand");
        let parts = Demand::decode_owned(&bytes, &contract, [7; 32], b"checked payload", &limits)
            .expect("owned replay");
        assert!(parts.value().node_chain().is_none());
        assert_eq!(
            parts.value().encode(limits.json_bytes).expect("reencode"),
            bytes
        );
        assert_eq!(parts.value().digest(), demand.digest());
        assert!(
            Demand::decode_owned(&bytes, &contract, [8; 32], b"checked payload", &limits).is_err()
        );
        assert!(
            Demand::decode_owned(&bytes, &contract, [7; 32], b"other payload", &limits).is_err()
        );
        let mut noncanonical = bytes.to_vec();
        noncanonical.push(b' ');
        assert!(Demand::decode_owned(
            &noncanonical,
            &contract,
            [7; 32],
            b"checked payload",
            &limits
        )
        .is_err());
        // One byte under the document size is an accounting target: the
        // demand still replays and the axis records the truthful size.
        let mut bounded = crate::cargo::Limits::standard();
        bounded.json_bytes = bytes.len() - 1;
        let mut account = crate::surface::account::Account::profile();
        let replayed = Demand::decode_owned_accounted(
            &bytes,
            &contract,
            [7; 32],
            b"checked payload",
            &bounded,
            &mut account,
        )
        .expect("the owned demand replays past the JSON target");
        assert_eq!(replayed.value().digest(), demand.digest());
        let axis = *account
            .exceedances()
            .axis("profile demand bytes")
            .expect("the demand byte target records its axis");
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            (
                (bytes.len() - 1) as u64,
                bytes.len() as u64,
                bytes.len() as u64
            )
        );
        let mut exact = crate::cargo::Limits::standard();
        exact.json_bytes = bytes.len();
        let mut account = crate::surface::account::Account::profile();
        Demand::decode_owned_accounted(
            &bytes,
            &contract,
            [7; 32],
            b"checked payload",
            &exact,
            &mut account,
        )
        .expect("the exact JSON target admits");
        assert!(!account.exceedances().is_exceeded());
    }

    /// The checked payload and hook name string targets are accounting
    /// on the demand builder; the demand still seals and records them.
    #[test]
    fn demand_builder_string_targets_record_without_refusal() {
        let limits = crate::cargo::Limits::standard();
        let contract = contract(&limits);
        let payload = b"checked payload";
        let mut exact = limits;
        exact.string_bytes = payload.len();
        let builder = Builder::new(&contract, [7; 32], payload, &exact).expect("exact builder");
        assert!(!builder.exceedances().is_exceeded());
        builder.seal().expect("exact demand seals");
        let mut short = limits;
        short.string_bytes = payload.len() - 1;
        let builder = Builder::new(&contract, [7; 32], payload, &short)
            .expect("a payload past the string target still builds");
        let axis = *builder
            .exceedances()
            .axis("profile checked payload bytes")
            .expect("the payload target records its axis");
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            (
                (payload.len() - 1) as u64,
                payload.len() as u64,
                payload.len() as u64
            )
        );
        let demand = builder
            .seal()
            .expect("the demand seals past the string target");
        assert_eq!(demand.payload.as_ref(), payload);
    }

    #[test]
    fn owned_generated_replay_preserves_digests_and_rejects_lifecycle_and_version_drift() {
        let limits = crate::cargo::Limits::standard();
        let contract = contract(&limits);
        let demand = Builder::new(&contract, [7; 32], &[7; 32], &limits)
            .expect("builder")
            .seal()
            .expect("demand");
        for lifecycle in [false, true] {
            let proof = Generator::new(&demand, &limits)
                .seal(lifecycle)
                .expect("proof");
            let bytes = proof
                .encode_owner(limits.json_bytes)
                .expect("canonical owner");
            let parts =
                Proof::decode_owned(&bytes, &demand, lifecycle, &limits).expect("owned replay");
            assert!(parts.value().node_chain().is_none());
            assert_eq!(
                parts
                    .value()
                    .encode_owner(limits.json_bytes)
                    .expect("reencode"),
                bytes
            );
            assert_eq!(parts.value().fingerprint(), proof.fingerprint());
            assert_eq!(parts.value().owner_fingerprint(), proof.owner_fingerprint());
            assert_eq!(
                parts.value().application_semantic(),
                proof.application_semantic()
            );
            assert!(Proof::decode_owned(&bytes, &demand, !lifecycle, &limits).is_err());
            let mut stored: super::super::StoredOwnerCanonical =
                serde_json::from_slice(&bytes).expect("stored owner");
            stored.version += 1;
            let foreign = serde_json::to_vec(&stored).expect("foreign canonical owner");
            assert!(Proof::decode_owned(&foreign, &demand, lifecycle, &limits).is_err());
            let mut noncanonical = bytes.to_vec();
            noncanonical.push(b' ');
            assert!(Proof::decode_owned(&noncanonical, &demand, lifecycle, &limits).is_err());
            // One byte under the proof size is an accounting target: the
            // proof still replays and the axis records the truthful size.
            let mut bounded = crate::cargo::Limits::standard();
            bounded.json_bytes = bytes.len() - 1;
            let mut account = crate::surface::account::Account::profile();
            let replayed =
                Proof::decode_owned_accounted(&bytes, &demand, lifecycle, &bounded, &mut account)
                    .expect("the owned proof replays past the JSON target");
            assert_eq!(replayed.value().fingerprint(), proof.fingerprint());
            let axis = *account
                .exceedances()
                .axis("generated profile proof bytes")
                .expect("the proof byte target records its axis");
            assert_eq!(
                (axis.target, axis.first_requested, axis.high_water),
                (
                    (bytes.len() - 1) as u64,
                    bytes.len() as u64,
                    bytes.len() as u64
                )
            );
            let mut exact = crate::cargo::Limits::standard();
            exact.json_bytes = bytes.len();
            let mut account = crate::surface::account::Account::profile();
            Proof::decode_owned_accounted(&bytes, &demand, lifecycle, &exact, &mut account)
                .expect("the exact JSON target admits");
            assert!(!account.exceedances().is_exceeded());
        }
    }
}
