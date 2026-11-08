//! Graph-origin profile classification proof.

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::{Error, Identity, Version};

/// Closed compiler-evidence class selectable by a profile.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum Kind {
    Definition,
    Export,
    Implementation,
}

/// One exact compiler-evidence selection retained by a profile contract.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct Evidence {
    kind: Kind,
    index: u32,
    digest: [u8; 32],
}

/// One target generic argument proven by an exact direct projection pair.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum Argument {
    /// Reuse one zero-based source type parameter.
    Source(u32),
    /// Infer a fresh target parameter inside the checked projection site.
    Fresh,
}

/// Opaque selector for one compiler-authenticated facade alias target.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct AliasKey([u8; 32]);

/// Opaque selector for one compiler-authenticated projection source site.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct SourceKey(pub(crate) [u8; 32]);

/// One total profile-owned operation formal specialization.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum EndpointArgument {
    /// Bind one function lifetime formal to the fresh view region.
    Region { parameter: u32 },
    /// Bind one function type formal to an exact retained definition.
    Definition {
        parameter: u32,
        definition: Evidence,
    },
    /// Transform one owned Rawr String plan value into the exact borrowed input.
    OwnedString { parameter: u32, lifetime: u32 },
}

/// One explicit closed ABI specialization argument.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub enum AbiArgument {
    /// Apply the compiler-proved `f32` default to one exact type formal.
    DefaultF32 { parameter: u32 },
}

/// One total source-alias substitution relation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum SourceArgument {
    /// Bind the alias and source lifetime formals to the same fresh view region.
    Region { alias: u32, source: u32 },
    /// Preserve one type or const formal through the alias normalization.
    Parameter {
        kind: crate::surface::facade::ParameterKind,
        alias: u32,
        source: u32,
    },
}

/// One exact source alias and normalized semantic target proof.
#[derive(Deserialize, Serialize)]
pub struct AliasTarget {
    site: crate::surface::facade::AliasSite,
    export: Evidence,
    alias: Evidence,
    target: Evidence,
    selector: [u8; 32],
}

/// One support-only source alias joined to exact projection implementation evidence.
#[derive(Deserialize, Serialize)]
pub struct ProjectionSourceSite {
    site: crate::surface::facade::ProjectionSourceSite,
    export: Evidence,
    alias: Evidence,
    source: Evidence,
    widget_trait: Evidence,
    widget: Evidence,
    target: AliasKey,
    arguments: Box<[SourceArgument]>,
    selector: [u8; 32],
}

/// One profile-owned support-only extension ABI proof bundle.
#[derive(Deserialize, Serialize)]
pub struct ExtensionAbi {
    site: crate::surface::facade::AbiSite,
    export: Evidence,
    definition: Evidence,
    role: Box<str>,
    linked: [u8; 32],
    arguments: Box<[AbiArgument]>,
    selector: [u8; 32],
}

/// One exact support-only operation endpoint closed by a framework profile.
#[derive(Deserialize, Serialize)]
pub struct OperationEndpoint {
    site: crate::surface::facade::OperationSite,
    export: Evidence,
    definition: Evidence,
    source: SourceKey,
    projection: [u8; 32],
    arguments: Box<[EndpointArgument]>,
    linked: [u8; 32],
    selector: [u8; 32],
}

/// One exact source-owned projection selected from compiler implementation evidence.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct Projection {
    source: SourceKey,
    alias: AliasKey,
    target: Evidence,
    arguments: Box<[Argument]>,
    selector: [u8; 32],
}

/// Closed profile evidence from which one private mapped invocation may prove
/// its exact Widget-to-Element projectability.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct EventProjectionRules {
    profile: [u8; 32],
    version: u32,
    rules: [u8; 32],
    source: [u8; 32],
    projection: [u8; 32],
    target: [u8; 32],
    widget_trait: [u8; 32],
}

/// Immutable profile contract bound to one extracted graph.
pub struct Contract {
    identity: Identity,
    version: Version,
    graph: crate::api::Fingerprint,
    semantic_graph: crate::api::Fingerprint,
    evidence: Box<[Evidence]>,
    aliases: Box<[AliasTarget]>,
    sources: Box<[ProjectionSourceSite]>,
    abis: Box<[ExtensionAbi]>,
    endpoints: Box<[OperationEndpoint]>,
    projections: Box<[Projection]>,
    rules: [u8; 32],
    chain: Option<crate::node::Chain>,
}

/// Graph-bound profile classification builder.
pub struct Builder<'graph, P: crate::api::Phase = crate::api::Extracted> {
    identity: Identity,
    version: Version,
    graph: &'graph crate::api::Graph<P>,
    evidence: Vec<Evidence>,
    aliases: Vec<AliasTarget>,
    sources: Vec<ProjectionSourceSite>,
    abis: Vec<ExtensionAbi>,
    endpoints: Vec<OperationEndpoint>,
    projections: Vec<Projection>,
}

#[derive(Serialize)]
struct Canonical<'a> {
    schema: u32,
    identity: &'a [u8; 32],
    version: u32,
    graph: &'a [u8; 32],
    evidence: &'a [Evidence],
    aliases: &'a [AliasTarget],
    sources: &'a [ProjectionSourceSite],
    abis: &'a [ExtensionAbi],
    endpoints: &'a [OperationEndpoint],
    projections: &'a [Projection],
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredCanonical {
    schema: u32,
    identity: [u8; 32],
    version: u32,
    graph: [u8; 32],
    evidence: Box<[Evidence]>,
    aliases: Box<[AliasTarget]>,
    sources: Box<[ProjectionSourceSite]>,
    abis: Box<[ExtensionAbi]>,
    endpoints: Box<[OperationEndpoint]>,
    projections: Box<[Projection]>,
}

/// Owned classification evidence without current-node authority.
/// Only explicit conversion with a current profile chain can produce a cached Contract.
pub(crate) struct Parts {
    identity: Identity,
    version: Version,
    stored: StoredCanonical,
    graph: crate::api::Fingerprint,
    rules: [u8; 32],
}

impl Parts {
    pub(crate) const fn identity(&self) -> Identity {
        self.identity
    }
    pub(crate) const fn version(&self) -> Version {
        self.version
    }
    pub(crate) const fn rules_digest(&self) -> &[u8; 32] {
        &self.rules
    }
    pub(crate) const fn graph_fingerprint(&self) -> crate::api::Fingerprint {
        self.graph
    }

    pub(crate) fn decode(
        bytes: &[u8],
        identity: Identity,
        version: Version,
        graph: &crate::api::Graph<impl crate::api::Phase>,
    ) -> Result<Self, Error> {
        Self::decode_with(
            bytes,
            identity,
            version,
            (graph.fingerprint(), graph.semantic_fingerprint()),
            |evidence| evidence_matches_graph(evidence, graph),
        )
    }

    /// Decode a product whose graph evidence was checked when it was resolved
    /// under this exact API node; only the API identities are compared here.
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn decode_resolved(
        bytes: &[u8],
        identity: Identity,
        version: Version,
        graph: (crate::api::Fingerprint, crate::api::Fingerprint),
    ) -> Result<Self, Error> {
        Self::decode_with(bytes, identity, version, graph, |_| true)
    }

    fn decode_with(
        bytes: &[u8],
        identity: Identity,
        version: Version,
        (fingerprint, semantic): (crate::api::Fingerprint, crate::api::Fingerprint),
        matches_graph: impl Fn(&Evidence) -> bool,
    ) -> Result<Self, Error> {
        let stored: StoredCanonical = serde_json::from_slice(bytes).map_err(|_| Error::Stale)?;
        let canonical = serde_json::to_vec(&stored).expect("a profile contract serializes");
        if canonical.as_slice() != bytes
            || stored.schema != 7
            || stored.identity != *identity.as_bytes()
            || stored.version != version.get()
            || stored.graph != *semantic.as_bytes()
            || stored.evidence.windows(2).any(|pair| pair[0] >= pair[1])
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
                .evidence
                .iter()
                .any(|evidence| !matches_graph(evidence))
        {
            return Err(Error::Stale);
        }
        Ok(Self {
            identity,
            version,
            stored,
            graph: fingerprint,
            rules: Sha256::digest(bytes).into(),
        })
    }

    pub(crate) fn encode(&self) -> Result<Box<[u8]>, Error> {
        let bytes = serde_json::to_vec(&self.stored).expect("a profile contract serializes");
        assert_eq!(
            <[u8; 32]>::from(Sha256::digest(&bytes)),
            self.rules,
            "a profile contract encodes to the bytes it was sealed from"
        );
        Ok(bytes.into_boxed_slice())
    }

    /// The internal caller supplies this product's exact authenticated or promoted
    /// profile receipt, never an API ancestor or an unrelated current chain.
    pub(crate) fn into_current(self, chain: crate::node::Chain) -> Contract {
        self.into_contract(Some(chain))
    }

    fn into_contract(self, chain: Option<crate::node::Chain>) -> Contract {
        Contract {
            identity: self.identity,
            version: self.version,
            graph: self.graph,
            semantic_graph: crate::api::Fingerprint::from_bytes(self.stored.graph),
            evidence: self.stored.evidence,
            aliases: self.stored.aliases,
            sources: self.stored.sources,
            abis: self.stored.abis,
            endpoints: self.stored.endpoints,
            projections: self.stored.projections,
            rules: self.rules,
            chain,
        }
    }
}

impl Contract {
    pub(super) fn encode(&self) -> Result<Box<[u8]>, Error> {
        let canonical = serde_json::to_vec(&Canonical {
            schema: 7,
            identity: self.identity.as_bytes(),
            version: self.version.get(),
            graph: self.semantic_graph.as_bytes(),
            evidence: &self.evidence,
            aliases: &self.aliases,
            sources: &self.sources,
            abis: &self.abis,
            endpoints: &self.endpoints,
            projections: &self.projections,
        })
        .expect("a profile contract serializes");
        assert_eq!(
            <[u8; 32]>::from(Sha256::digest(&canonical)),
            self.rules,
            "a profile contract encodes to the bytes it was sealed from"
        );
        Ok(canonical.into_boxed_slice())
    }

    pub(super) fn decode(
        bytes: &[u8],
        identity: Identity,
        version: Version,
        graph: &crate::api::Graph<crate::api::Extracted>,
        chain: crate::node::Chain,
    ) -> Result<Self, Error> {
        Ok(Parts::decode(bytes, identity, version, graph)?.into_current(chain))
    }

    /// Borrow the current profile-classification node for demand joins.
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

    /// Return the authoritative imported graph fingerprint.
    #[must_use]
    pub const fn graph_fingerprint(&self) -> crate::api::Fingerprint {
        self.graph
    }

    /// Iterate exact compiler-evidence selections in canonical order.
    pub fn evidence(&self) -> impl ExactSizeIterator<Item = &Evidence> {
        self.evidence.iter()
    }

    /// Iterate exact support-only facade aliases in canonical selector order.
    pub fn alias_targets(&self) -> impl ExactSizeIterator<Item = &AliasTarget> {
        self.aliases.iter()
    }

    /// Iterate exact support-only projection source sites.
    pub fn projection_sources(&self) -> impl ExactSizeIterator<Item = &ProjectionSourceSite> {
        self.sources.iter()
    }

    /// Iterate exact support-only extension ABI proofs.
    pub fn extension_abis(&self) -> impl ExactSizeIterator<Item = &ExtensionAbi> {
        self.abis.iter()
    }

    /// Iterate exact support-only operation endpoints.
    pub fn operation_endpoints(&self) -> impl ExactSizeIterator<Item = &OperationEndpoint> {
        self.endpoints.iter()
    }

    /// Iterate exact direct projection pairs in canonical selector order.
    pub fn projections(&self) -> impl ExactSizeIterator<Item = &Projection> {
        self.projections.iter()
    }

    /// Borrow the digest of the complete profile classification rule proof.
    #[must_use]
    pub const fn rules_digest(&self) -> &[u8; 32] {
        &self.rules
    }

    pub(crate) fn contains(&self, evidence: &Evidence) -> bool {
        self.evidence.binary_search(evidence).is_ok()
    }

    pub(crate) fn contains_projection(&self, projection: &Projection) -> bool {
        self.projections
            .binary_search_by_key(&projection.selector, |candidate| candidate.selector)
            .is_ok_and(|index| self.projections[index] == *projection)
    }

    pub(crate) fn contains_alias_target(&self, alias: &AliasTarget) -> bool {
        self.aliases
            .binary_search_by_key(&alias.selector, |candidate| candidate.selector)
            .is_ok_and(|index| canonical_equal(&self.aliases[index], alias))
    }

    pub(crate) fn contains_projection_source(&self, source: &ProjectionSourceSite) -> bool {
        self.sources
            .binary_search_by_key(&source.selector, |candidate| candidate.selector)
            .is_ok_and(|index| canonical_equal(&self.sources[index], source))
    }

    pub(crate) fn contains_abi(&self, abi: &ExtensionAbi) -> bool {
        self.abis
            .binary_search_by_key(&abi.selector, |candidate| candidate.selector)
            .is_ok_and(|index| canonical_equal(&self.abis[index], abi))
    }

    pub(crate) fn contains_endpoint(&self, endpoint: &OperationEndpoint) -> bool {
        self.endpoints
            .binary_search_by_key(&endpoint.selector, |candidate| candidate.selector)
            .is_ok_and(|index| canonical_equal(&self.endpoints[index], endpoint))
    }
}

fn canonical_equal<T: Serialize>(left: &T, right: &T) -> bool {
    serde_json::to_vec(left)
        .ok()
        .zip(serde_json::to_vec(right).ok())
        .is_some_and(|(left, right)| left == right)
}

impl EventProjectionRules {
    pub(crate) const fn profile(&self) -> &[u8; 32] {
        &self.profile
    }

    pub(crate) const fn version(&self) -> u32 {
        self.version
    }

    pub(crate) const fn rules(&self) -> &[u8; 32] {
        &self.rules
    }

    pub(crate) const fn source(&self) -> &[u8; 32] {
        &self.source
    }

    pub(crate) const fn projection(&self) -> &[u8; 32] {
        &self.projection
    }

    pub(crate) const fn target(&self) -> &[u8; 32] {
        &self.target
    }

    pub(crate) const fn widget_trait(&self) -> &[u8; 32] {
        &self.widget_trait
    }
}

impl Evidence {
    /// Return the selected compiler-evidence class.
    #[must_use]
    pub const fn kind(&self) -> Kind {
        self.kind
    }

    /// Return the stable canonical graph index.
    #[must_use]
    pub const fn index(&self) -> u32 {
        self.index
    }

    #[doc(hidden)]
    pub const fn digest(&self) -> &[u8; 32] {
        &self.digest
    }
}

impl Argument {
    /// Reuse one zero-based source type parameter.
    #[must_use]
    pub const fn source(index: u32) -> Self {
        Self::Source(index)
    }

    /// Infer a fresh target parameter at the checked projection site.
    #[must_use]
    pub const fn fresh() -> Self {
        Self::Fresh
    }

    /// Return the reused source parameter, or `None` for a fresh argument.
    #[must_use]
    pub const fn source_index(self) -> Option<u32> {
        match self {
            Self::Source(index) => Some(index),
            Self::Fresh => None,
        }
    }
}

impl AliasTarget {
    /// Return the exact facade root containing the source alias.
    pub fn root(&self) -> &str {
        self.site.root()
    }

    /// Return the exact facade-relative source alias path.
    pub fn path(&self) -> &str {
        self.site.path()
    }

    /// Return the exact Rust namespace.
    pub const fn namespace(&self) -> crate::surface::facade::Namespace {
        self.site.namespace()
    }

    /// Borrow the exact export identity.
    pub const fn export_identity(&self) -> &[u8; 32] {
        self.site.export_identity()
    }

    /// Borrow the source alias definition identity.
    pub const fn alias_identity(&self) -> &[u8; 32] {
        self.site.alias_identity()
    }

    /// Borrow the normalized semantic target identity.
    pub const fn target_identity(&self) -> &[u8; 32] {
        self.site.target_identity()
    }

    /// Borrow the complete ordered formal-parameter mapping.
    pub const fn parameters(&self) -> &[crate::surface::facade::AliasParameter] {
        self.site.parameters()
    }

    /// Borrow the facade alias-site selector.
    pub const fn site_selector(&self) -> &[u8; 32] {
        self.site.selector()
    }

    /// Borrow the profile alias-target selector.
    pub const fn selector(&self) -> &[u8; 32] {
        &self.selector
    }

    pub(crate) fn try_copy(&self) -> Result<Self, Error> {
        Ok(Self {
            site: self.site.try_copy().map_err(surface_error)?,
            export: self.export,
            alias: self.alias,
            target: self.target,
            selector: self.selector,
        })
    }
}

impl ProjectionSourceSite {
    /// Return the exact facade root containing the source alias.
    pub fn root(&self) -> &str {
        self.site.root()
    }
    /// Return the exact facade-relative source alias path.
    pub fn path(&self) -> &str {
        self.site.path()
    }
    /// Return the source alias namespace.
    pub const fn namespace(&self) -> crate::surface::facade::Namespace {
        self.site.namespace()
    }
    /// Borrow the exact source export identity.
    pub const fn export_identity(&self) -> &[u8; 32] {
        self.site.export_identity()
    }
    /// Borrow the exact source alias definition identity.
    pub const fn alias_identity(&self) -> &[u8; 32] {
        self.site.alias_identity()
    }
    /// Borrow the exact normalized source definition identity.
    pub const fn source_identity(&self) -> &[u8; 32] {
        self.site.source_identity()
    }
    /// Return the validated source shape.
    pub const fn shape(&self) -> crate::surface::facade::SourceShape {
        self.site.shape()
    }
    /// Borrow the complete ordered alias-to-source mapping.
    pub const fn parameters(&self) -> &[crate::surface::facade::AliasParameter] {
        self.site.parameters()
    }
    /// Borrow the explicit total source substitution.
    pub const fn arguments(&self) -> &[SourceArgument] {
        &self.arguments
    }
    /// Borrow the exact facade source-site selector.
    pub const fn site_selector(&self) -> &[u8; 32] {
        self.site.selector()
    }
    /// Borrow the exact Widget implementation identity.
    pub const fn widget_evidence(&self) -> &[u8; 32] {
        self.widget.digest()
    }
    /// Borrow the exact Widget trait definition identity.
    pub const fn widget_trait_evidence(&self) -> &[u8; 32] {
        self.widget_trait.digest()
    }
    /// Return the Widget implementation evidence that authorizes the
    /// generated hook (the projection is the Widget's own `_boxed`).
    #[doc(hidden)]
    pub const fn implementation(&self) -> Evidence {
        self.widget
    }
    /// Borrow the exact target AliasTarget selector.
    pub const fn target_selector(&self) -> &[u8; 32] {
        &self.target.0
    }
    /// Borrow the domain-separated source proof selector.
    pub const fn selector(&self) -> &[u8; 32] {
        &self.selector
    }

    pub(crate) fn try_copy(&self) -> Result<Self, Error> {
        let mut arguments = Vec::new();
        arguments
            .try_reserve_exact(self.arguments.len())
            .map_err(|_| Error::Allocation)?;
        arguments.extend_from_slice(&self.arguments);
        Ok(Self {
            site: self.site.try_copy().map_err(surface_error)?,
            export: self.export,
            alias: self.alias,
            source: self.source,
            widget_trait: self.widget_trait,
            widget: self.widget,
            target: self.target,
            arguments: arguments.into_boxed_slice(),
            selector: self.selector,
        })
    }
}

impl ExtensionAbi {
    /// Return the profile-owned closed role name.
    pub fn role(&self) -> &str {
        &self.role
    }

    /// Return the exact selected public root.
    pub fn root(&self) -> &str {
        self.site.root()
    }

    /// Return the exact selected public path.
    pub fn path(&self) -> &str {
        self.site.path()
    }

    /// Return the selected Rust namespace.
    pub const fn namespace(&self) -> crate::surface::facade::Namespace {
        self.site.namespace()
    }

    /// Borrow the exact public export identity.
    pub const fn export_identity(&self) -> &[u8; 32] {
        self.site.export_identity()
    }

    /// Borrow the exact underlying definition identity.
    pub const fn definition_identity(&self) -> &[u8; 32] {
        self.site.definition_identity()
    }

    /// Borrow the complete compiler-source-shape digest.
    pub const fn shape_digest(&self) -> &[u8; 32] {
        self.site.shape_digest()
    }

    /// Borrow the generic support-site selector.
    pub const fn site_selector(&self) -> &[u8; 32] {
        self.site.selector()
    }

    /// Borrow the linked-host semantic fingerprint.
    pub const fn linked_fingerprint(&self) -> &[u8; 32] {
        &self.linked
    }

    /// Borrow the explicit ordered default application.
    pub const fn arguments(&self) -> &[AbiArgument] {
        &self.arguments
    }

    /// Borrow the domain-separated profile ABI selector.
    pub const fn selector(&self) -> &[u8; 32] {
        &self.selector
    }

    pub(crate) fn try_copy(&self) -> Result<Self, Error> {
        let mut arguments = Vec::new();
        arguments
            .try_reserve_exact(self.arguments.len())
            .map_err(|_| Error::Allocation)?;
        arguments.extend_from_slice(&self.arguments);
        Ok(Self {
            site: self.site.try_copy().map_err(surface_error)?,
            export: self.export,
            definition: self.definition,
            role: copy_string(&self.role)?,
            linked: self.linked,
            arguments: arguments.into_boxed_slice(),
            selector: self.selector,
        })
    }
}

impl OperationEndpoint {
    /// Borrow the compiler-authenticated generic operation site.
    pub const fn site(&self) -> &crate::surface::facade::OperationSite {
        &self.site
    }

    /// Borrow the exact source-site selector consumed by this endpoint.
    pub const fn source_selector(&self) -> &[u8; 32] {
        &self.source.0
    }

    /// Borrow the exact immediate projection selector.
    pub const fn projection_selector(&self) -> &[u8; 32] {
        &self.projection
    }

    /// Borrow the total ordered formal specialization.
    pub const fn arguments(&self) -> &[EndpointArgument] {
        &self.arguments
    }

    /// Borrow the linked lowering implementation fingerprint.
    pub const fn linked(&self) -> &[u8; 32] {
        &self.linked
    }

    /// Borrow the profile-owned endpoint selector.
    pub const fn selector(&self) -> &[u8; 32] {
        &self.selector
    }

    pub(crate) fn try_copy(&self) -> Result<Self, Error> {
        let mut arguments = Vec::new();
        arguments
            .try_reserve_exact(self.arguments.len())
            .map_err(|_| Error::Allocation)?;
        arguments.extend_from_slice(&self.arguments);
        Ok(Self {
            site: self.site.try_copy().map_err(surface_error)?,
            export: self.export,
            definition: self.definition,
            source: self.source,
            projection: self.projection,
            arguments: arguments.into_boxed_slice(),
            linked: self.linked,
            selector: self.selector,
        })
    }
}

impl Projection {
    /// Borrow the exact profile projection-source-site selector.
    #[must_use]
    pub const fn source_selector(&self) -> &[u8; 32] {
        &self.source.0
    }

    /// Borrow the target semantic definition identity.
    #[must_use]
    pub const fn target_identity(&self) -> &[u8; 32] {
        self.target.digest()
    }

    /// Borrow the exact profile alias-target selector.
    #[must_use]
    pub const fn alias_selector(&self) -> &[u8; 32] {
        &self.alias.0
    }

    /// Borrow the exact target argument mapping.
    #[must_use]
    pub const fn arguments(&self) -> &[Argument] {
        &self.arguments
    }

    /// Borrow the canonical direct-projection selector.
    #[must_use]
    pub const fn selector(&self) -> &[u8; 32] {
        &self.selector
    }
}

impl<'graph, P: crate::api::Phase> Builder<'graph, P> {
    pub(crate) fn new(
        identity: Identity,
        version: Version,
        graph: &'graph crate::api::Graph<P>,
    ) -> Self {
        Self {
            identity,
            version,
            graph,
            evidence: Vec::new(),
            aliases: Vec::new(),
            sources: Vec::new(),
            abis: Vec::new(),
            endpoints: Vec::new(),
            projections: Vec::new(),
        }
    }

    /// Retain one exact definition borrowed from the authoritative graph.
    pub fn definition(
        &mut self,
        definition: &crate::api::model::Definition,
    ) -> Result<Evidence, Error> {
        let index = self
            .graph
            .definitions()
            .position(|candidate| candidate == definition)
            .expect("a profile classifies definitions of the graph it was given");
        self.insert(
            Kind::Definition,
            index,
            *crate::surface::identity::Definition::of(definition.id()).as_bytes(),
        )
    }

    fn insert(&mut self, kind: Kind, index: usize, digest: [u8; 32]) -> Result<Evidence, Error> {
        let index = u32::try_from(index).map_err(|_| Error::Limit {
            resource: "profile graph index",
            limit: u32::MAX as usize,
            actual: index,
        })?;
        let evidence = Evidence {
            kind,
            index,
            digest,
        };
        match self.evidence.binary_search(&evidence) {
            Ok(_) => panic!("a profile classifies each piece of evidence once"),
            Err(position) => {
                self.evidence
                    .try_reserve_exact(1)
                    .map_err(|_| Error::Allocation)?;
                self.evidence.insert(position, evidence);
                Ok(evidence)
            }
        }
    }

    pub(crate) fn seal_parts(self) -> Result<Parts, Error> {
        let canonical = serde_json::to_vec(&Canonical {
            schema: 7,
            identity: self.identity.as_bytes(),
            version: self.version.get(),
            graph: self.graph.semantic_fingerprint().as_bytes(),
            evidence: &self.evidence,
            aliases: &self.aliases,
            sources: &self.sources,
            abis: &self.abis,
            endpoints: &self.endpoints,
            projections: &self.projections,
        })
        .expect("a profile contract serializes");
        Ok(Parts {
            identity: self.identity,
            version: self.version,
            graph: self.graph.fingerprint(),
            stored: StoredCanonical {
                schema: 7,
                identity: *self.identity.as_bytes(),
                version: self.version.get(),
                graph: *self.graph.semantic_fingerprint().as_bytes(),
                evidence: self.evidence.into_boxed_slice(),
                aliases: self.aliases.into_boxed_slice(),
                sources: self.sources.into_boxed_slice(),
                abis: self.abis.into_boxed_slice(),
                endpoints: self.endpoints.into_boxed_slice(),
                projections: self.projections.into_boxed_slice(),
            },
            rules: Sha256::digest(canonical).into(),
        })
    }
}

fn evidence_matches_graph(
    evidence: &Evidence,
    graph: &crate::api::Graph<impl crate::api::Phase>,
) -> bool {
    let index = usize::try_from(evidence.index).unwrap_or(usize::MAX);
    let digest = match evidence.kind {
        Kind::Definition => graph.definitions().nth(index).map(|definition| {
            *crate::surface::identity::Definition::of(definition.id()).as_bytes()
        }),
        Kind::Export => graph.exports().nth(index).map(|export| match export {
            crate::api::ledger::Path::Imported(export) => {
                *crate::surface::identity::Export::imported(export).as_bytes()
            }
            crate::api::ledger::Path::Excluded { key, .. }
            | crate::api::ledger::Path::Rejected { key, .. } => {
                *crate::surface::identity::Export::unresolved(key).as_bytes()
            }
        }),
        Kind::Implementation => graph
            .implementations()
            .nth(index)
            .and_then(|implementation| {
                crate::surface::identity::Implementation::of(implementation).ok()
            })
            .map(|implementation| *implementation.as_bytes()),
    };
    digest == Some(evidence.digest)
}

/// Copying a bindings site fails only for want of memory.
fn surface_error(error: crate::surface::Error) -> Error {
    match error {
        crate::surface::Error::Allocation => Error::Allocation,
        error => panic!("cannot copy a bindings site: {error}"),
    }
}

fn copy_string(value: &str) -> Result<Box<str>, Error> {
    let mut output = String::new();
    output
        .try_reserve_exact(value.len())
        .map_err(|_| Error::Allocation)?;
    output.push_str(value);
    Ok(output.into_boxed_str())
}

impl Builder<'_, crate::api::Extracted> {
    pub(crate) fn seal(self) -> Result<Contract, Error> {
        Ok(self.seal_parts()?.into_contract(None))
    }
}
