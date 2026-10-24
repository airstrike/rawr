//! Pinned iced profile definition.

use std::hash::{Hash, Hasher};

use rawr_import::profile;
use sha2::{Digest as _, Sha256};

use super::{evidence, projection, resolution};

const IDENTITY: &str = "rawr.framework.iced";
pub(super) const VERSION: u32 = 17;

/// Exact checked-program proof consumed by iced profile demand.
pub struct Checked {
    fingerprint: [u8; 32],
    semantic: [u8; 32],
    chain: Option<rawr_import::node::Chain>,
}

/// The generic profile for the pinned iced dependency universe.
pub struct Definition {
    identity: profile::Identity,
    version: profile::Version,
    universe: rawr_import::cargo::universe::Catalog,
}

impl Checked {
    /// Bind profile demand to one compiler-checked application contract.
    #[must_use]
    pub fn from_application(application: &crate::application::Checked) -> Self {
        Self::from_descriptor(application.descriptor())
    }

    /// Bind profile demand to one exact compiler-normalized application descriptor.
    #[doc(hidden)]
    #[must_use]
    pub fn from_descriptor(application: &rawr::application::Descriptor) -> Self {
        let mut hasher = ProofHasher::new();
        application.fingerprint().hash(&mut hasher);
        let fingerprint = hasher.seal();
        Self {
            fingerprint,
            semantic: fingerprint,
            chain: None,
        }
    }

    /// Bind imported profile demand to a compiler-only normalized application
    /// contract and its complete exact staging-facade cycle evidence.
    #[doc(hidden)]
    #[must_use]
    pub fn from_checked_contract(
        application: &rawr::application::CheckedContract,
        cycle: &rawr::host::facade::ProfileCycle,
    ) -> Self {
        let semantic = *application.fingerprint().as_bytes();
        let mut checked = CheckedHasher::new();
        semantic.hash(&mut checked);
        cycle.hash(&mut checked);
        Self {
            fingerprint: checked.seal(),
            semantic,
            chain: None,
        }
    }

    /// Bind a test or generated-runner caller to an already-canonical digest.
    #[doc(hidden)]
    #[must_use]
    pub const fn from_fingerprint(fingerprint: [u8; 32]) -> Self {
        Self {
            fingerprint,
            semantic: fingerprint,
            chain: None,
        }
    }

    /// Reconstruct one exact checked proof from a validated cached checked node.
    #[doc(hidden)]
    #[must_use]
    pub fn from_cached(cached: &rawr_import::checked::Cached) -> Option<Self> {
        let semantic = cached.checked_payload().try_into().ok()?;
        Some(Self {
            fingerprint: cached.checked_fingerprint(),
            semantic,
            chain: Some(cached.node_chain().clone()),
        })
    }

    /// Inspect authenticated historical checked bytes without a current node chain.
    #[doc(hidden)]
    #[must_use]
    pub fn from_retained(cached: &rawr_import::checked::Retained) -> Option<Self> {
        Some(Self {
            fingerprint: cached.checked_fingerprint(),
            semantic: cached.checked_payload().try_into().ok()?,
            chain: None,
        })
    }

    /// Return the exact cached checked-node payload without exposing constructors.
    #[doc(hidden)]
    #[must_use]
    pub const fn cached_parts(&self) -> ([u8; 32], [u8; 32]) {
        (self.fingerprint, self.semantic)
    }
}

impl Definition {
    /// Construct the pinned iced profile under the importer's active limits.
    pub fn new(limits: &rawr_import::cargo::Limits) -> Result<Self, profile::Error> {
        let identity = profile::Identity::new(IDENTITY, limits)?;
        let version = profile::Version::new(VERSION)?;
        let universe =
            resolution::catalog(limits).map_err(|_| profile::Error::ResolutionUniverse)?;
        Ok(Self {
            identity,
            version,
            universe,
        })
    }

    /// The iced paths the profile's evidence selection reads,
    /// demanded on every preparation (kept beside the lookups in `evidence`).
    pub const DEMAND: &'static [&'static str] = evidence::DEMAND;

    /// Clone the one preselected Rawr-owned Cargo universe for request sealing.
    #[doc(hidden)]
    #[must_use]
    pub fn resolution_universe(&self) -> rawr_import::cargo::universe::Catalog {
        self.universe.clone()
    }

    /// The alias the profile's evidence paths start with.
    pub const FRAMEWORK: &'static str = "iced";

    /// The `iced` crate the profile reads its evidence from, at the exact
    /// revision rawr-iced links. A program that declares no `iced`
    /// dependency is imported with this one, so a program using only other
    /// crates still gives the profile its evidence.
    pub fn framework_dependency(
        limits: &rawr_import::cargo::Limits,
    ) -> rawr_import::Result<rawr_import::cargo::config::Dependency> {
        rawr_import::cargo::config::Dependency::git(
            Self::FRAMEWORK,
            "https://github.com/iced-rs/iced",
            crate::native::ICED_REVISION,
            limits,
        )?
        .features(["advanced"], false)
    }

    /// Select this exact crate as the generated runner's semantic profile
    /// host, with the widget `features` the project's iced has and nothing
    /// else, so it adds no feature to that iced (`crate::features`).
    pub fn cargo_dependency(
        features: &[crate::features::Feature],
        limits: &rawr_import::cargo::Limits,
    ) -> rawr_import::Result<rawr_import::cargo::config::Dependency> {
        let dependency = rawr_import::cargo::config::Dependency::profile_path(
            "__rawr_profile_host",
            "rawr-iced",
            &rawr_import::host::root()?.join("iced"),
            limits,
        )?;
        let names = features.iter().map(|feature| feature.name());
        dependency.features(names, false)
    }
}

impl profile::Checked for Checked {
    fn profile_fingerprint(&self) -> [u8; 32] {
        self.fingerprint
    }

    fn profile_payload(&self) -> &[u8] {
        &self.semantic
    }

    fn node_chain(&self) -> Option<&rawr_import::node::Chain> {
        self.chain.as_ref()
    }
}

impl profile::Definition<Checked> for Definition {
    fn identity(&self) -> profile::Identity {
        self.identity
    }

    fn version(&self) -> profile::Version {
        self.version
    }

    fn owns_product_lifecycle(&self) -> bool {
        true
    }

    fn classify(
        &self,
        graph: &rawr_import::api::Graph<rawr_import::api::Extracted>,
        output: &mut profile::contract::Builder<'_>,
    ) -> Result<(), profile::Error> {
        self.classify_owned(graph, output)
    }
    fn demand(
        &self,
        contract: &profile::Contract,
        _checked: &Checked,
        output: &mut profile::demand::Builder<'_>,
    ) -> Result<(), profile::Error> {
        self.demand_owned(profile::trusted::Contract::from_current(contract), output)
    }
    fn generate(
        &self,
        demand: &profile::Demand,
        output: &mut profile::demand::Generator<'_>,
    ) -> Result<(), profile::Error> {
        self.generate_owned(profile::trusted::Demand::from_current(demand), output)
    }
}

impl Definition {
    /// Select the audited owned-data adapter. The pinned universe, fixture,
    /// rule version and lifecycle are immutable semantic configuration; the
    /// shared operations read no undeclared mutable inputs.
    pub fn trusted_adapter(&self) -> profile::trusted::Adapter<&Self> {
        profile::trusted::Adapter::trusted(self, self.identity, self.version, true).semantic_api()
    }
    fn classify_owned<P: rawr_import::api::Phase>(
        &self,
        graph: &rawr_import::api::Graph<P>,
        output: &mut profile::contract::Builder<'_, P>,
    ) -> Result<(), profile::Error> {
        match graph.resolution_universe() {
            Some(actual) if actual == self.universe.identity() => {}
            Some(_) | None => return Err(profile::Error::ForeignEvidence),
        }
        let selected = evidence::Set::select(graph)?;
        let stage = evidence::Diagnostic::ContractEvidence;
        let element_export = evidence::diagnosed(output.export(selected.element_export()), stage)?;
        let element_alias =
            evidence::diagnosed(output.definition(selected.element_alias()), stage)?;
        let widget_definition = evidence::diagnosed(output.definition(selected.widget()), stage)?;
        let element = evidence::diagnosed(output.definition(selected.element()), stage)?;
        let element_alias = evidence::diagnosed(
            output.alias_target(element_export, element_alias, element),
            stage,
        )?;
        let source_export = evidence::diagnosed(output.export(selected.source_export()), stage)?;
        let source_alias = evidence::diagnosed(output.definition(selected.source_alias()), stage)?;
        let source_definition = evidence::diagnosed(output.definition(selected.source()), stage)?;
        let mut widgets = Vec::new();
        for implementation in selected.widgets() {
            widgets.push((
                implementation,
                evidence::diagnosed(output.implementation(implementation), stage)?,
            ));
        }
        let mut endpoint_source = None;
        let mut endpoint_projection = None;
        for pair in selected.projections() {
            if pair.target().id() != selected.element().id()
                || pair.source().id() != selected.source().id()
            {
                return Err(profile::Error::ForeignEvidence);
            }
            let widget = widgets
                .iter()
                .find(|(implementation, _)| *implementation == pair.widget())
                .map(|(_, evidence)| *evidence)
                .ok_or(profile::Error::ForeignEvidence)?;
            let source = evidence::diagnosed(
                output.projection_source_site(
                    source_export,
                    source_alias,
                    source_definition,
                    widget_definition,
                    widget,
                    element_alias,
                ),
                stage,
            )?;
            let projection = evidence::diagnosed(
                output.projection(source, element_alias, pair.arguments()),
                stage,
            )?;
            if endpoint_source.replace(source).is_some()
                || endpoint_projection.replace(projection).is_some()
            {
                return Err(profile::Error::Canonical);
            }
        }
        for abi in selected.abis() {
            let export = evidence::diagnosed(output.export(abi.export()), stage)?;
            let definition = evidence::diagnosed(output.definition(abi.definition()), stage)?;
            evidence::diagnosed(
                output.extension_abi(
                    export,
                    definition,
                    abi.role().name(),
                    abi.role().linked_fingerprint(),
                    abi.arguments(),
                ),
                stage,
            )?;
        }
        let operation_export =
            evidence::diagnosed(output.export(selected.operation_export()), stage)?;
        let operation_definition =
            evidence::diagnosed(output.definition(selected.operation_definition()), stage)?;
        let theme = evidence::diagnosed(output.definition(selected.operation_theme()), stage)?;
        let source = endpoint_source.ok_or(profile::Error::ForeignEvidence)?;
        let projection = endpoint_projection.ok_or(profile::Error::ForeignEvidence)?;
        evidence::diagnosed(
            output.operation_endpoint(
                operation_export,
                operation_definition,
                source,
                &projection,
                &[
                    profile::contract::EndpointArgument::Region { parameter: 0 },
                    profile::contract::EndpointArgument::Definition {
                        parameter: 1,
                        definition: theme,
                    },
                    profile::contract::EndpointArgument::OwnedString {
                        parameter: 2,
                        lifetime: 0,
                    },
                ],
                evidence::operation_linked_fingerprint(),
            ),
            stage,
        )?;
        Ok(())
    }

    fn demand_owned(
        &self,
        contract: profile::trusted::Contract<'_>,
        output: &mut profile::demand::Builder<'_>,
    ) -> Result<(), profile::Error> {
        let source = contract
            .projection_sources()
            .next()
            .ok_or(profile::Error::ForeignEvidence)?;
        let hook = projection::HOOK;
        output.hook(source.implementation(), hook)?;
        for pair in contract.projections() {
            output.projection(pair)?;
        }
        for abi in contract.extension_abis() {
            output.extension_abi(abi)?;
        }
        for endpoint in contract.operation_endpoints() {
            output.operation_endpoint(endpoint)?;
        }
        Ok(())
    }

    fn generate_owned(
        &self,
        demand: profile::trusted::Demand<'_>,
        output: &mut profile::demand::Generator<'_>,
    ) -> Result<(), profile::Error> {
        let hook = demand
            .hooks()
            .next()
            .copied()
            .ok_or(profile::Error::ForeignEvidence)?;
        output.projection(hook, projection::source(hook, demand).as_bytes())?;
        Ok(())
    }
}

impl profile::trusted::Operations for &Definition {
    fn classify<P: rawr_import::api::Phase>(
        &self,
        graph: &rawr_import::api::Graph<P>,
        output: &mut profile::contract::Builder<'_, P>,
    ) -> Result<(), profile::Error> {
        self.classify_owned(graph, output)
    }
    fn demand(
        &self,
        contract: profile::trusted::Contract<'_>,
        _checked: profile::trusted::Checked<'_>,
        output: &mut profile::demand::Builder<'_>,
    ) -> Result<(), profile::Error> {
        self.demand_owned(contract, output)
    }
    fn generate(
        &self,
        demand: profile::trusted::Demand<'_>,
        output: &mut profile::demand::Generator<'_>,
    ) -> Result<(), profile::Error> {
        self.generate_owned(demand, output)
    }
}

struct ProofHasher(Sha256);

struct CheckedHasher(Sha256);

impl ProofHasher {
    fn new() -> Self {
        let mut digest = Sha256::new();
        digest.update(b"rawr.iced.profile.checked/v1");
        Self(digest)
    }

    fn seal(self) -> [u8; 32] {
        self.0.finalize().into()
    }
}

impl Hasher for ProofHasher {
    fn finish(&self) -> u64 {
        0
    }

    fn write(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
}

impl CheckedHasher {
    fn new() -> Self {
        let mut digest = Sha256::new();
        digest.update(b"rawr.iced.profile.checked/v2");
        Self(digest)
    }

    fn seal(self) -> [u8; 32] {
        self.0.finalize().into()
    }
}

impl Hasher for CheckedHasher {
    fn finish(&self) -> u64 {
        0
    }

    fn write(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
}
