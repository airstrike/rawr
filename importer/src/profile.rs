//! Framework-neutral compile and runtime profile proofs.

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

pub mod activation;
pub(crate) mod cached;
pub mod contract;
pub mod demand;
mod error;
pub mod identity;
pub mod message;
pub mod runtime;
pub mod trusted;

pub use contract::Contract;
pub use demand::Demand;
pub use error::Error;
pub use identity::{Identity, Version};

const VERSION: u32 = 15;

#[cfg(feature = "test-fixtures")]
const FIXTURE_LIFECYCLE_SOURCE: &[u8] = br#"
mod __rawr_profile {
    #[derive(Clone)]
    pub struct ApplicationContract;

    #[derive(Clone, Copy)]
    pub enum Mode { Check, Run, Preview, SnapshotCheck, SnapshotUpdate }

    struct Signal {
        exited: std::sync::Mutex<bool>,
        wake: std::sync::Condvar,
    }

    pub struct Running(std::sync::Arc<Signal>);
    pub struct Transport(std::sync::Arc<Signal>);
    pub struct PendingInstall;
    pub struct InstallError;
    impl std::fmt::Display for InstallError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("fixture installation rejected")
        }
    }
    impl InstallError {
        pub fn not_installed_diagnostic(&self) -> Option<&str> {
            Some("fixture installation rejected")
        }
        pub fn is_superseded(&self) -> bool { false }
    }

    impl Running {
        pub fn run(self) -> Result<(), &'static str> {
            let mut exited = self.0.exited.lock().map_err(|_| "lifecycle lock")?;
            while !*exited {
                exited = self.0.wake.wait(exited).map_err(|_| "lifecycle wait")?;
            }
            Ok(())
        }
    }

    impl Transport {
        pub fn take_refresh(&self) -> bool { false }
        pub fn begin_install(&self) -> Result<PendingInstall, InstallError> { Ok(PendingInstall) }
        pub fn resident_eligible(&self, _pending: &PendingInstall) -> bool { false }
        pub fn request_is_current(&self, _pending: &PendingInstall) -> Result<bool, InstallError> { Ok(true) }
        pub fn observe_install(&self, _pending: &mut PendingInstall, _bytes: &[u8]) -> Result<(), InstallError> { Ok(()) }
        pub fn cancel_install(&self, _pending: PendingInstall) -> Result<(), InstallError> { Ok(()) }
        pub fn install(
            &self,
            _pending: PendingInstall,
            _candidate: __rawr_internal::reload::Candidate,
        ) -> Result<(), InstallError> {
            Ok(())
        }

        pub fn report(&self, _report: &str) -> Result<(), &'static str> { Ok(()) }

        pub fn exit(&self) -> Result<(), &'static str> {
            let mut exited = self.0.exited.lock().map_err(|_| "lifecycle lock")?;
            *exited = true;
            self.0.wake.notify_all();
            Ok(())
        }

        pub fn retire(&self) -> Result<bool, &'static str> {
            self.exit()?;
            Ok(self.take_refresh())
        }
    }

    pub fn compile_initial(
        engine: &__rawr_internal::Engine,
        snapshot: &__rawr_internal::package::snapshot::Snapshot,
        _contract: &ApplicationContract,
        _expected: &[u8; 32],
    ) -> std::io::Result<(Option<__rawr_internal::reload::Candidate>, __rawr_internal::AST)> {
        engine.compile_package_snapshot(snapshot)
            .map(|ast| (None, ast))
            .map_err(|error| std::io::Error::other(error.to_string()))
    }

    pub fn compile_generation(
        _engine: &__rawr_internal::Engine,
        _snapshot: &__rawr_internal::package::snapshot::Snapshot,
        _contract: &ApplicationContract,
        _expected: &[u8; 32],
    ) -> std::io::Result<Option<__rawr_internal::reload::Candidate>> { Ok(None) }

    pub fn compile_resident(
        _engine: &__rawr_internal::Engine,
        _snapshot: &__rawr_internal::package::snapshot::Snapshot,
        _contract: &ApplicationContract,
    ) -> Result<__rawr_internal::package::import::resident::Outcome, __rawr_internal::package::import::resident::Error> {
        Ok(__rawr_internal::package::import::resident::Outcome::NeedsPreparation)
    }

    pub fn bind_runtime(
        _definitions: &[__rawr_internal::host::facade::Definition],
        _builders: &mut [__rawr_internal::host::runtime::Builder],
    ) -> Result<(), &'static str> { Ok(()) }

    pub fn install_compiler_facades(
        engine: &mut __rawr_internal::Engine,
        native: &mut __rawr_internal::package::import::runner::account::Native,
    ) -> std::io::Result<(Box<[__rawr_internal::host::facade::Definition]>, ApplicationContract)> {
        if super::IMPORTED_FACADES.is_empty() {
            return Ok((Box::default(), ApplicationContract));
        }
        __rawr_internal::package::import::runner::install_retained(engine, super::IMPORTED_FACADES, native)
            .map(|definitions| (definitions, ApplicationContract))
            .map_err(|error| std::io::Error::other(error.to_string()))
    }

    pub struct Runtime { stage: u8 }

    impl Runtime {
        pub fn new(_mode: Mode, _root: std::path::PathBuf) -> Self { Self { stage: 0 } }

        pub fn prepare(&mut self) -> Result<(), &'static str> {
            if self.stage != 0 { return Err("prepare order"); }
            self.stage = 1;
            Ok(())
        }

        pub fn activate(&mut self) -> Result<(), &'static str> {
            if self.stage != 1 { return Err("activate order"); }
            self.stage = 2;
            Ok(())
        }

        pub fn admit(&mut self, program: Option<&mut super::Program>) -> Result<(), &'static str> {
            if self.stage != 2 || program.is_none() { return Err("admit order"); }
            self.stage = 3;
            Ok(())
        }

        pub fn message(&mut self, payload: &[u8]) -> Result<Vec<u8>, &'static str> {
            if self.stage != 3 { return Err("message before admission"); }
            Ok(payload.to_vec())
        }

        pub fn observe(&mut self, _bytes: &[u8]) -> Result<(), &'static str> {
            if self.stage != 3 { return Err("observe before admission"); }
            Ok(())
        }

        pub fn start(
            &mut self,
            program: Option<&mut super::Program>,
        ) -> Result<(Running, Transport), &'static str> {
            if self.stage != 3 || program.is_none() { return Err("start order"); }
            self.stage = 4;
            let signal = std::sync::Arc::new(Signal {
                exited: std::sync::Mutex::new(false),
                wake: std::sync::Condvar::new(),
            });
            Ok((Running(std::sync::Arc::clone(&signal)), Transport(signal)))
        }

        pub fn snapshot(&mut self) -> Result<Box<[u8]>, &'static str> {
            if self.stage != 3 { return Err("snapshot before admission"); }
            Ok(Box::default())
        }

        pub fn retire(&mut self) { self.stage = 5; }
    }
}
"#;

/// Core-owned checked semantic evidence borrowed by a profile demand pass.
pub trait Checked {
    /// Return the stable digest of the complete checked program proof.
    fn profile_fingerprint(&self) -> [u8; 32];

    /// Borrow bounded opaque checked data copied into generated profile source.
    /// The importer fingerprints and transports these bytes but never interprets them.
    #[doc(hidden)]
    fn profile_payload(&self) -> &[u8] {
        &[]
    }

    /// Borrow the current checked-program node when this proof came from DAG replay.
    #[doc(hidden)]
    fn node_chain(&self) -> Option<&crate::node::Chain> {
        None
    }
}

/// Deterministic framework profile definition.
pub trait Definition<C: Checked>: Send + Sync + 'static {
    /// Return the profile's stable semantic identity.
    fn identity(&self) -> Identity;

    /// Return the profile rule version.
    fn version(&self) -> Version;

    /// Select the generated child's product-lifecycle owner protocol. Profiles
    /// that do not own a native product lifecycle retain the established
    /// prepare/activate/admit/message runner.
    #[doc(hidden)]
    fn owns_product_lifecycle(&self) -> bool {
        false
    }

    /// Select exact compiler evidence used by this profile.
    fn classify(
        &self,
        graph: &crate::api::Graph<crate::api::Extracted>,
        output: &mut contract::Builder<'_>,
    ) -> Result<(), Error>;

    /// Select demanded hooks from the classified proof and checked program.
    fn demand(
        &self,
        contract: &Contract,
        checked: &C,
        output: &mut demand::Builder<'_>,
    ) -> Result<(), Error>;

    /// Emit only projections already present in the sealed demand.
    fn generate(&self, demand: &Demand, output: &mut demand::Generator<'_>) -> Result<(), Error>;
}

/// Exact profile proof consumed by generated-runner identity.
pub struct Proof {
    digest: [u8; 32],
    owner: [u8; 32],
    application: [u8; 32],
    identity: Identity,
    version: Version,
    rules: [u8; 32],
    product_lifecycle: bool,
    source: Box<[u8]>,
    aliases: Box<[contract::AliasTarget]>,
    sources: Box<[contract::ProjectionSourceSite]>,
    abis: Box<[contract::ExtensionAbi]>,
    endpoints: Box<[contract::OperationEndpoint]>,
    projections: Box<[contract::Projection]>,
    chain: Option<crate::node::Chain>,
}

/// Canonically validated generated owner, without current graph authority.
pub(crate) struct Parts(Proof);

impl Parts {
    pub(crate) const fn value(&self) -> &Proof {
        &self.0
    }

    pub(crate) fn into_current(self, chain: crate::node::Chain) -> Proof {
        Proof {
            chain: Some(chain),
            ..self.0
        }
    }
}

#[derive(Serialize)]
struct OwnerCanonical<'a> {
    schema: u32,
    identity: &'a [u8; 32],
    version: u32,
    rules: &'a [u8; 32],
    product_lifecycle: bool,
    source: &'a [u8],
    aliases: &'a [contract::AliasTarget],
    sources: &'a [contract::ProjectionSourceSite],
    abis: &'a [contract::ExtensionAbi],
    endpoints: &'a [contract::OperationEndpoint],
    projections: &'a [contract::Projection],
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredOwnerCanonical {
    schema: u32,
    identity: [u8; 32],
    version: u32,
    rules: [u8; 32],
    product_lifecycle: bool,
    source: Box<[u8]>,
    aliases: Box<[contract::AliasTarget]>,
    sources: Box<[contract::ProjectionSourceSite]>,
    abis: Box<[contract::ExtensionAbi]>,
    endpoints: Box<[contract::OperationEndpoint]>,
    projections: Box<[contract::Projection]>,
}

impl Proof {
    /// Select no framework integration while preserving the Phase-4 identity.
    #[must_use]
    pub fn neutral() -> Self {
        let mut digest = Sha256::new();
        digest.update(b"rawr.import.profile/neutral");
        digest.update(VERSION.to_le_bytes());
        Self {
            digest: digest.finalize().into(),
            owner: [0; 32],
            application: [0; 32],
            identity: Identity([0; 32]),
            version: Version(1),
            rules: [0; 32],
            product_lifecycle: false,
            source: Box::default(),
            aliases: Box::default(),
            sources: Box::default(),
            abis: Box::default(),
            endpoints: Box::default(),
            projections: Box::default(),
            chain: None,
        }
    }

    /// Attach one test-only current parent to the neutral profile proof.
    #[cfg(feature = "test-fixtures")]
    #[doc(hidden)]
    #[must_use]
    pub fn neutral_with_chain(chain: crate::node::Chain) -> Self {
        Self::neutral().with_chain(chain)
    }

    /// Attach one test-only current parent to a generated profile proof.
    #[cfg(feature = "test-fixtures")]
    #[doc(hidden)]
    #[must_use]
    pub fn with_chain(mut self, chain: crate::node::Chain) -> Self {
        self.chain = Some(chain);
        self
    }

    /// Attach current test application evidence without changing the stable owner.
    #[cfg(feature = "test-fixtures")]
    #[doc(hidden)]
    #[must_use]
    pub(crate) fn neutral_with_application(application: [u8; 32]) -> Self {
        let mut proof = Self::neutral();
        proof.application = application;
        let mut digest = Sha256::new();
        digest.update(b"rawr.import.profile/neutral-application/v1");
        digest.update(VERSION.to_le_bytes());
        digest.update(application);
        proof.digest = digest.finalize().into();
        proof
    }

    /// Attach one test DAG parent after current application evidence is sealed.
    #[cfg(feature = "test-fixtures")]
    #[doc(hidden)]
    #[must_use]
    pub fn neutral_with_chain_application(
        chain: crate::node::Chain,
        application: [u8; 32],
    ) -> Self {
        let mut proof = Self::neutral_with_application(application);
        proof.chain = Some(chain);
        proof
    }

    /// Attach one test-only non-iced lifecycle implementation to a current DAG.
    #[cfg(feature = "test-fixtures")]
    #[doc(hidden)]
    pub fn fixture_lifecycle_with_chain_application(
        chain: crate::node::Chain,
        application: [u8; 32],
        limits: &crate::cargo::Limits,
    ) -> Result<Self, Error> {
        // Source and proof sizes are accounting targets recorded on a
        // profile account; nothing here refuses on them.
        let mut account = crate::surface::account::Account::profile();
        account.exceeded(
            "fixture lifecycle profile source bytes",
            limits.json_bytes,
            FIXTURE_LIFECYCLE_SOURCE.len(),
        );
        let mut proof = Self::neutral_with_application(application);
        proof.product_lifecycle = true;
        proof.source = Box::from(FIXTURE_LIFECYCLE_SOURCE);
        let canonical = serde_json::to_vec(&OwnerCanonical {
            schema: 9,
            identity: proof.identity.as_bytes(),
            version: proof.version.get(),
            rules: &proof.rules,
            product_lifecycle: proof.product_lifecycle,
            source: &proof.source,
            aliases: &proof.aliases,
            sources: &proof.sources,
            abis: &proof.abis,
            endpoints: &proof.endpoints,
            projections: &proof.projections,
        })
        .map_err(|_| Error::Encoding)?;
        account.exceeded(
            "fixture lifecycle profile proof bytes",
            limits.json_bytes,
            canonical.len(),
        );
        proof.owner = owner_digest(&canonical);
        let mut digest = Sha256::new();
        digest.update(b"rawr.import.profile/fixture-lifecycle/v1");
        digest.update(proof.owner);
        digest.update(application);
        proof.digest = digest.finalize().into();
        proof.chain = Some(chain);
        Ok(proof)
    }

    /// Transform one sealed demand through its exact definition.
    pub fn generate<C: Checked, D: Definition<C>>(
        definition: &D,
        demand: &Demand,
        limits: &crate::cargo::Limits,
    ) -> Result<Self, Error> {
        if demand.node_chain().is_some() {
            return demand::cached::generate(definition, demand, limits);
        }
        Self::generate_direct(definition, demand, limits)
    }

    fn generate_direct<C: Checked, D: Definition<C>>(
        definition: &D,
        demand: &Demand,
        limits: &crate::cargo::Limits,
    ) -> Result<Self, Error> {
        if definition.identity() != demand.identity() || definition.version() != demand.version() {
            return Err(Error::DefinitionDrift);
        }
        let mut output = demand::Generator::new(demand, limits);
        definition.generate(demand, &mut output)?;
        output.seal(definition.owns_product_lifecycle())
    }

    fn encode_owner(&self, limit: usize) -> Result<Box<[u8]>, Error> {
        let canonical = serde_json::to_vec(&OwnerCanonical {
            schema: 9,
            identity: self.identity.as_bytes(),
            version: self.version.get(),
            rules: &self.rules,
            product_lifecycle: self.product_lifecycle,
            source: &self.source,
            aliases: &self.aliases,
            sources: &self.sources,
            abis: &self.abis,
            endpoints: &self.endpoints,
            projections: &self.projections,
        })
        .map_err(|_| Error::Encoding)?;
        if canonical.len() > limit {
            return Err(Error::Limit {
                resource: "generated profile proof bytes",
                limit,
                actual: canonical.len(),
            });
        }
        if owner_digest(&canonical) != self.owner {
            return Err(Error::Canonical);
        }
        Ok(canonical.into_boxed_slice())
    }

    fn decode_owner(
        bytes: &[u8],
        demand: &Demand,
        product_lifecycle: bool,
        chain: crate::node::Chain,
        limits: &crate::cargo::Limits,
    ) -> Result<Self, Error> {
        Self::decode_owned(bytes, demand, product_lifecycle, limits)
            .map(|parts| parts.into_current(chain))
    }

    pub(crate) fn decode_owned(
        bytes: &[u8],
        demand: &Demand,
        product_lifecycle: bool,
        limits: &crate::cargo::Limits,
    ) -> Result<Parts, Error> {
        // Document, source and item sizes are accounting targets recorded
        // on a profile account for this replay; the physical bound is the node
        // content ceiling that already admitted the bytes. Checked-arithmetic
        // overflow stays terminal.
        let mut account = crate::surface::account::Account::profile();
        Self::decode_owned_accounted(bytes, demand, product_lifecycle, limits, &mut account)
    }

    pub(crate) fn decode_owned_accounted(
        bytes: &[u8],
        demand: &Demand,
        product_lifecycle: bool,
        limits: &crate::cargo::Limits,
        account: &mut crate::surface::account::Account,
    ) -> Result<Parts, Error> {
        account.exceeded(
            "generated profile proof bytes",
            limits.json_bytes,
            bytes.len(),
        );
        let stored: StoredOwnerCanonical =
            serde_json::from_slice(bytes).map_err(|_| Error::Encoding)?;
        let canonical = serde_json::to_vec(&stored).map_err(|_| Error::Encoding)?;
        let items = stored
            .aliases
            .len()
            .checked_add(stored.sources.len())
            .and_then(|items| items.checked_add(stored.abis.len()))
            .and_then(|items| items.checked_add(stored.endpoints.len()))
            .and_then(|items| items.checked_add(stored.projections.len()))
            .ok_or(Error::Limit {
                resource: "generated profile proof items",
                limit: limits.items,
                actual: usize::MAX,
            })?;
        account.exceeded(
            "generated profile proof source bytes",
            limits.json_bytes,
            stored.source.len(),
        );
        account.exceeded("generated profile proof items", limits.items, items);
        if canonical.as_slice() != bytes
            || stored.schema != 9
            || stored.identity != *demand.identity().as_bytes()
            || stored.version != demand.version().get()
            || stored.rules != *demand.rules_digest()
            || stored.product_lifecycle != product_lifecycle
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
                .aliases
                .iter()
                .any(|alias| !demand.contains_alias_target(alias))
            || stored
                .sources
                .iter()
                .any(|source| !demand.contains_projection_source(source))
            || stored.abis.iter().any(|abi| !demand.contains_abi(abi))
            || stored
                .endpoints
                .iter()
                .any(|endpoint| !demand.contains_endpoint(endpoint))
            || stored
                .projections
                .iter()
                .any(|projection| !demand.contains_projection(projection))
        {
            return Err(Error::Canonical);
        }
        let owner = owner_digest(bytes);
        let application = application_semantic(demand)?;
        Ok(Parts(Self {
            digest: proof_digest(demand, owner, application, product_lifecycle),
            owner,
            application,
            identity: demand.identity(),
            version: demand.version(),
            rules: stored.rules,
            product_lifecycle,
            source: stored.source,
            aliases: stored.aliases,
            sources: stored.sources,
            abis: stored.abis,
            endpoints: stored.endpoints,
            projections: stored.projections,
            chain: None,
        }))
    }

    /// Borrow the current generated-profile node for runner joins.
    #[doc(hidden)]
    #[must_use]
    pub const fn node_chain(&self) -> Option<&crate::node::Chain> {
        self.chain.as_ref()
    }

    /// Borrow the stable compiled-profile owner identity. Current application
    /// evidence is deliberately excluded and is rebound during preparation.
    pub(crate) const fn owner_fingerprint(&self) -> &[u8; 32] {
        &self.owner
    }

    /// Borrow the current application semantic admitted with the snapshot.
    pub(crate) const fn application_semantic(&self) -> &[u8; 32] {
        &self.application
    }

    pub(crate) const fn identity(&self) -> Identity {
        self.identity
    }

    pub(crate) const fn version(&self) -> Version {
        self.version
    }

    pub(crate) const fn rules_digest(&self) -> &[u8; 32] {
        &self.rules
    }

    /// Borrow the exact profile fingerprint sealed into runner identity.
    #[must_use]
    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.digest
    }

    /// Return whether this proof contributes generated runner projections.
    #[must_use]
    pub fn has_generated_projections(&self) -> bool {
        !self.source.is_empty()
    }

    /// Return whether this exact profile selected the generated native product
    /// lifecycle protocol before runner materialization.
    #[doc(hidden)]
    #[must_use]
    pub const fn owns_product_lifecycle(&self) -> bool {
        self.product_lifecycle
    }

    /// Borrow deterministic generated source for runner materialization.
    #[doc(hidden)]
    pub fn generated_source(&self) -> &[u8] {
        &self.source
    }

    pub(crate) const fn projections(&self) -> &[contract::Projection] {
        &self.projections
    }

    pub(crate) const fn alias_targets(&self) -> &[contract::AliasTarget] {
        &self.aliases
    }

    /// Borrow generated support-only extension ABI proofs.
    #[doc(hidden)]
    pub const fn extension_abis(&self) -> &[contract::ExtensionAbi] {
        &self.abis
    }

    /// Borrow generated support-only operation endpoint proofs.
    #[doc(hidden)]
    pub const fn operation_endpoints(&self) -> &[contract::OperationEndpoint] {
        &self.endpoints
    }

    pub(crate) fn source_for_projection(
        &self,
        projection: &contract::Projection,
    ) -> Option<&contract::ProjectionSourceSite> {
        self.sources
            .binary_search_by_key(projection.source_selector(), |candidate| {
                *candidate.selector()
            })
            .ok()
            .map(|index| &self.sources[index])
    }
}

fn application_semantic(demand: &Demand) -> Result<[u8; 32], Error> {
    if demand.hooks().len() == 0 && demand.checked_payload().is_empty() {
        return Ok([0; 32]);
    }
    demand
        .checked_payload()
        .try_into()
        .map_err(|_| Error::ForeignEvidence)
}

fn owner_digest(canonical: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"rawr.import.profile.owner/v1");
    digest.update(canonical);
    digest.finalize().into()
}

fn proof_digest(
    demand: &Demand,
    owner: [u8; 32],
    application: [u8; 32],
    product_lifecycle: bool,
) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"rawr.import.profile.proof/v8");
    digest.update(demand.identity().as_bytes());
    digest.update(demand.version().get().to_le_bytes());
    digest.update(demand.rules_digest());
    digest.update(demand.checked_fingerprint());
    digest.update(demand.digest());
    digest.update(owner);
    digest.update(application);
    digest.update([u8::from(product_lifecycle)]);
    digest.finalize().into()
}

impl Default for Proof {
    fn default() -> Self {
        Self::neutral()
    }
}

/// Transform an extracted graph into one graph-bound profile contract.
pub fn classify<C: Checked, D: Definition<C>>(
    definition: &D,
    graph: &crate::api::Graph<crate::api::Extracted>,
    limits: &crate::cargo::Limits,
) -> Result<Contract, Error> {
    if graph.node_chain().is_some() {
        return cached::classify(definition, graph, limits);
    }
    classify_direct(definition, graph, limits)
}

fn classify_direct<C: Checked, D: Definition<C>>(
    definition: &D,
    graph: &crate::api::Graph<crate::api::Extracted>,
    limits: &crate::cargo::Limits,
) -> Result<Contract, Error> {
    let identity = definition.identity();
    let version = definition.version();
    let mut output = contract::Builder::new(identity, version, graph, limits);
    definition.classify(graph, &mut output)?;
    output.seal()
}

/// Transform checked core evidence into one contract-bound profile demand.
pub fn demand<C: Checked, D: Definition<C>>(
    definition: &D,
    contract: &Contract,
    checked: &C,
    limits: &crate::cargo::Limits,
) -> Result<Demand, Error> {
    if contract.node_chain().is_some() {
        return demand::cached::evaluate(definition, contract, checked, limits);
    }
    demand_direct(definition, contract, checked, limits)
}

fn demand_direct<C: Checked, D: Definition<C>>(
    definition: &D,
    contract: &Contract,
    checked: &C,
    limits: &crate::cargo::Limits,
) -> Result<Demand, Error> {
    if definition.identity() != contract.identity() || definition.version() != contract.version() {
        return Err(Error::DefinitionDrift);
    }
    let mut output = demand::Builder::new(
        contract,
        checked.profile_fingerprint(),
        checked.profile_payload(),
        limits,
    )?;
    definition.demand(contract, checked, &mut output)?;
    output.seal()
}

pub(crate) const fn version() -> u32 {
    VERSION
}
