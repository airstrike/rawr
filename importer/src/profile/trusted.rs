//! Explicit semantic trust boundary for audited profile operations.
//!
//! This adapter does not certify purity. Its author must bind every successful
//! output to the supplied owned evidence and immutable versioned configuration.
//! Filesystem, environment, clock, child work and other undeclared inputs must not
//! affect output. Diagnostic writes may not become semantic inputs. Ordinary
//! `Definition` implementations do not acquire this contract automatically.

use super::{contract, demand, Error, Identity, Version};

/// Audited operations on owned data, without fresh current-node capabilities.
/// Implementing this trait is an explicit semantic trust assertion, not a Rust
/// memory-safety obligation. Configuration, fixture rules and lifecycle selection
/// must be immutable and covered by the supplied semantic identity and version.
pub trait Operations {
    /// Classify authenticated owned graph data without current input authority.
    fn classify<P: crate::api::Phase>(
        &self,
        graph: &crate::api::Graph<P>,
        output: &mut contract::Builder<'_, P>,
    ) -> Result<(), Error>;
    /// Select demand using only captured checked data and owned contract data.
    fn demand(
        &self,
        contract: Contract<'_>,
        checked: Checked<'_>,
        output: &mut demand::Builder<'_>,
    ) -> Result<(), Error>;
    /// Generate source from owned demand data.
    fn generate(&self, demand: Demand<'_>, output: &mut demand::Generator<'_>)
        -> Result<(), Error>;
}

/// Explicitly selected compiled integration implementing the trust obligations.
pub struct Adapter<T> {
    operations: T,
    identity: Identity,
    version: Version,
    lifecycle: bool,
    semantic_api: bool,
}

impl<T: Operations> Adapter<T> {
    /// Assert that these operations and immutable semantic configuration satisfy
    /// this module's contract. This constructor performs no independent audit.
    /// No conversion from an ordinary `Definition` is provided.
    pub const fn trusted(
        operations: T,
        identity: Identity,
        version: Version,
        lifecycle: bool,
    ) -> Self {
        Self {
            operations,
            identity,
            version,
            lifecycle,
            semantic_api: false,
        }
    }

    /// Select the complete semantic API view for an audited classifier.
    ///
    /// This additionally asserts that classification reads only compiler
    /// ledgers and universe semantics. Exact raw bytes, fingerprints, freshness,
    /// transaction accounting and node capabilities must not affect its output.
    /// The callback still receives the current graph; no observation is rewritten.
    pub const fn semantic_api(mut self) -> Self {
        self.semantic_api = true;
        self
    }

    pub(crate) const fn uses_semantic_api(&self) -> bool {
        self.semantic_api
    }

    /// Return the captured semantic identity.
    pub const fn identity(&self) -> Identity {
        self.identity
    }
    /// Return the captured semantic version.
    pub const fn version(&self) -> Version {
        self.version
    }
    /// Return the captured lifecycle selection.
    pub const fn owns_product_lifecycle(&self) -> bool {
        self.lifecycle
    }

    pub(crate) fn demand_owned(
        &self,
        contract: &super::Contract,
        fingerprint: [u8; 32],
        payload: &[u8],
        limits: &crate::cargo::Limits,
    ) -> Result<super::Demand, Error> {
        if self.identity != contract.identity() || self.version != contract.version() {
            return Err(Error::DefinitionDrift);
        }
        let mut output = demand::Builder::new(contract, fingerprint, payload, limits)?;
        self.operations.demand(
            Contract::from_current(contract),
            Checked::captured(&fingerprint, payload),
            &mut output,
        )?;
        output.seal()
    }

    pub(crate) fn generate_owned(
        &self,
        demand: &super::Demand,
        limits: &crate::cargo::Limits,
    ) -> Result<super::Proof, Error> {
        if self.identity != demand.identity() || self.version != demand.version() {
            return Err(Error::DefinitionDrift);
        }
        let mut output = demand::Generator::new(demand, limits);
        self.operations
            .generate(Demand::from_current(demand), &mut output)?;
        output.seal(self.lifecycle)
    }

    pub(crate) fn classify_parts<P: crate::api::Phase>(
        &self,
        graph: &crate::api::Graph<P>,
        limits: &crate::cargo::Limits,
    ) -> Result<contract::Parts, Error> {
        let mut output = contract::Builder::new(self.identity, self.version, graph, limits);
        self.operations.classify(graph, &mut output)?;
        output.seal_parts()
    }
}

/// Captured checked evidence. No user-supplied accessor is run by staged work.
#[derive(Clone, Copy)]
pub struct Checked<'a> {
    fingerprint: &'a [u8; 32],
    payload: &'a [u8],
}
impl<'a> Checked<'a> {
    pub(crate) const fn captured(fingerprint: &'a [u8; 32], payload: &'a [u8]) -> Self {
        Self {
            fingerprint,
            payload,
        }
    }
    /// Borrow the authenticated checked fingerprint.
    pub const fn fingerprint(self) -> &'a [u8; 32] {
        self.fingerprint
    }
    /// Borrow the authenticated bounded checked payload.
    pub const fn payload(self) -> &'a [u8] {
        self.payload
    }
}

/// Borrowed contract data with no current-node access.
#[derive(Clone, Copy)]
pub struct Contract<'a> {
    value: &'a super::Contract,
}
impl<'a> Contract<'a> {
    /// Forget current authority and borrow only already-owned semantic data.
    pub const fn from_current(value: &'a super::Contract) -> Self {
        Self { value }
    }
    /// Borrow owned projection sources data.
    pub fn projection_sources(
        self,
    ) -> impl ExactSizeIterator<Item = &'a contract::ProjectionSourceSite> {
        self.value.projection_sources()
    }
    /// Borrow owned projections data.
    pub fn projections(self) -> impl ExactSizeIterator<Item = &'a contract::Projection> {
        self.value.projections()
    }
    /// Borrow owned extension abis data.
    pub fn extension_abis(self) -> impl ExactSizeIterator<Item = &'a contract::ExtensionAbi> {
        self.value.extension_abis()
    }
    /// Borrow owned operation endpoints data.
    pub fn operation_endpoints(
        self,
    ) -> impl ExactSizeIterator<Item = &'a contract::OperationEndpoint> {
        self.value.operation_endpoints()
    }
}

/// Borrowed demand data with no current-node access.
#[derive(Clone, Copy)]
pub struct Demand<'a> {
    value: &'a super::Demand,
}
impl<'a> Demand<'a> {
    /// Forget current authority and borrow only already-owned semantic data.
    pub const fn from_current(value: &'a super::Demand) -> Self {
        Self { value }
    }
    /// Borrow the owned semantic rule digest.
    pub fn rules_digest(self) -> &'a [u8; 32] {
        self.value.rules_digest()
    }
    /// Borrow owned hook data.
    pub fn hooks(self) -> impl ExactSizeIterator<Item = &'a demand::Hook> {
        self.value.hooks()
    }
    /// Borrow owned alias targets data.
    pub fn alias_targets(self) -> impl ExactSizeIterator<Item = &'a contract::AliasTarget> {
        self.value.alias_targets()
    }
    /// Borrow owned projection sources data.
    pub fn projection_sources(
        self,
    ) -> impl ExactSizeIterator<Item = &'a contract::ProjectionSourceSite> {
        self.value.projection_sources()
    }
    /// Borrow owned projections data.
    pub fn projections(self) -> impl ExactSizeIterator<Item = &'a contract::Projection> {
        self.value.projections()
    }
    /// Borrow owned extension abis data.
    pub fn extension_abis(self) -> impl ExactSizeIterator<Item = &'a contract::ExtensionAbi> {
        self.value.extension_abis()
    }
    /// Borrow owned operation endpoints data.
    pub fn operation_endpoints(
        self,
    ) -> impl ExactSizeIterator<Item = &'a contract::OperationEndpoint> {
        self.value.operation_endpoints()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Empty;
    impl Operations for Empty {
        fn classify<P: crate::api::Phase>(
            &self,
            _graph: &crate::api::Graph<P>,
            _output: &mut contract::Builder<'_, P>,
        ) -> Result<(), Error> {
            Ok(())
        }
        fn demand(
            &self,
            _contract: Contract<'_>,
            _checked: Checked<'_>,
            _output: &mut demand::Builder<'_>,
        ) -> Result<(), Error> {
            Ok(())
        }
        fn generate(
            &self,
            _demand: Demand<'_>,
            _output: &mut demand::Generator<'_>,
        ) -> Result<(), Error> {
            Ok(())
        }
    }

    #[test]
    fn explicit_adapter_classifies_owned_parts_without_current_authority() {
        let limits = crate::cargo::Limits::standard();
        let graph = crate::test_fixture::graph();
        let identity = Identity::new("trusted.empty", &limits).expect("identity");
        let version = Version::new(1).expect("version");
        let adapter = Adapter::trusted(Empty, identity, version, false);
        let parts = adapter
            .classify_parts(&graph, &limits)
            .expect("owned classification");
        let ordinary = contract::Builder::new(identity, version, &graph, &limits)
            .seal()
            .expect("ordinary classification");
        assert_eq!(
            parts
                .encode(limits.node_output_bytes())
                .expect("parts encode"),
            ordinary
                .encode(limits.node_output_bytes())
                .expect("contract encodes")
        );
        let demand = adapter
            .demand_owned(&ordinary, [3; 32], &[], &limits)
            .expect("owned demand");
        let expected_demand = demand::Builder::new(&ordinary, [3; 32], &[], &limits)
            .expect("ordinary builder")
            .seal()
            .expect("ordinary demand");
        assert!(demand.node_chain().is_none());
        assert_eq!(
            demand
                .encode(limits.node_output_bytes())
                .expect("owned demand encodes"),
            expected_demand
                .encode(limits.node_output_bytes())
                .expect("ordinary demand encodes")
        );
        let generated = adapter
            .generate_owned(&demand, &limits)
            .expect("owned generation");
        let expected_generated = demand::Generator::new(&expected_demand, &limits)
            .seal(false)
            .expect("ordinary generation");
        assert!(generated.node_chain().is_none());
        assert_eq!(generated.fingerprint(), expected_generated.fingerprint());
        assert_eq!(
            generated.generated_source(),
            expected_generated.generated_source()
        );

        assert_eq!(adapter.identity(), identity);
        assert_eq!(adapter.version(), version);
        assert!(!adapter.owns_product_lifecycle());
        assert_eq!(parts.graph_fingerprint(), graph.fingerprint());
    }
}
