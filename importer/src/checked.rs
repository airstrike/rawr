//! Content-addressed checked Rawr application evidence.

use crate::node;

pub(crate) mod retained;
pub use retained::Retained;

const DOMAIN: &[u8] = b"rawr.checked.application/v2";
const MAGIC: &[u8; 8] = b"RAWRCHK1";
const HEADER_BYTES: usize = 96;

/// One validated checked-program node ready for exact demand replay.
pub struct Cached {
    product: node::Validated<Product>,
    chain: node::Chain,
}

/// Distinguish a product transform failure from cache/infrastructure failure.
#[derive(Debug)]
pub enum Evaluation<E> {
    /// The supplied cold transform ran and failed.
    Transform(E),
    /// Node evaluation, validation, or replay failed.
    Cache(Error),
}

/// Evaluate or replay one exact checked-application node.
pub fn evaluate<B, C, E>(
    contract: &crate::Contract<crate::Classified>,
    profile: &crate::profile::Contract,
    facade: &crate::surface::facade::Cached,
    snapshot: crate::surface::Snapshot,
    limits: &crate::cargo::Limits,
    execute: impl FnOnce() -> Result<(crate::Demand<crate::Sealed, B>, C), E>,
) -> Result<Cached, Evaluation<E>>
where
    C: crate::profile::Checked,
{
    let profile_parent = profile
        .node_chain()
        .ok_or(Evaluation::Cache(Error::ForeignEvidence))?;
    let observations = contract
        .raw_graph()
        .callback_observations(contract.raw_graph().freshness_fingerprint());
    let authority = authority(
        contract.fingerprint(),
        contract.raw_fingerprint(),
        profile.identity(),
        profile.version(),
        profile.rules_digest(),
        profile.graph_fingerprint(),
        snapshot,
        Some(&observations),
    )
    .map_err(Evaluation::Cache)?;
    let parent = facade.node_chain();
    let mut transform = Transform::new();
    let product =
        parent.direct_with_parents::<Product>(&authority, &[profile_parent.parent()], || {
            transform.capture(contract.fingerprint(), snapshot, limits, execute)
        });
    let product = transform.resolve(product)?;
    Cached::from_validated(parent.context().clone(), product, None).map_err(Evaluation::Cache)
}

/// Prepare the unchanged checked action identity from authenticated owned data.
/// This does not grant current authority or execute a source consumer.
pub(crate) fn authority(
    contract: crate::contract::fingerprint::Fingerprint,
    graph: crate::api::Fingerprint,
    identity: crate::profile::Identity,
    version: crate::profile::Version,
    rules: &[u8; 32],
    profile_graph: crate::api::Fingerprint,
    snapshot: crate::surface::Snapshot,
    exact: Option<&[u8; 96]>,
) -> Result<Vec<u8>, Error> {
    if graph != profile_graph {
        return Err(Error::DefinitionDrift);
    }
    let mut authority = Vec::new();
    authority
        .try_reserve_exact(
            DOMAIN
                .len()
                .saturating_add(137 + exact.map_or(0, |bytes| bytes.len())),
        )
        .map_err(|_| Error::Allocation)?;
    authority.extend_from_slice(DOMAIN);
    authority.extend_from_slice(contract.as_bytes());
    authority.extend_from_slice(identity.digest());
    authority.extend_from_slice(&version.value().to_le_bytes());
    authority.extend_from_slice(rules);
    authority.extend_from_slice(snapshot.as_bytes());
    authority.push(u8::from(exact.is_some()));
    if let Some(exact) = exact {
        authority.extend_from_slice(exact);
    }
    Ok(authority)
}

/// Keep the caller's typed source failure separate from cache failures while
/// the evaluator transports its existing node error type.
pub(crate) struct Transform<E> {
    error: Option<E>,
}

impl<E> Transform<E> {
    pub(crate) const fn new() -> Self {
        Self { error: None }
    }

    /// The caller establishes current parent authority before invoking this
    /// actual source callback; captured accessors are never staged pure work.
    pub(crate) fn capture<B, C: crate::profile::Checked>(
        &mut self,
        contract: crate::contract::fingerprint::Fingerprint,
        snapshot: crate::surface::Snapshot,
        limits: &crate::cargo::Limits,
        execute: impl FnOnce() -> Result<(crate::Demand<crate::Sealed, B>, C), E>,
    ) -> Result<Product, node::Error> {
        match execute() {
            Ok((demand, checked)) => {
                Product::capture(&demand, &checked, contract, snapshot, limits)
                    .map_err(|error| node::Error::Transform(error.to_string().into_boxed_str()))
            }
            Err(error) => {
                self.error = Some(error);
                Err(node::Error::Transform(
                    "checked application transform failed".into(),
                ))
            }
        }
    }

    pub(crate) fn resolve<T>(self, result: Result<T, node::Error>) -> Result<T, Evaluation<E>> {
        match result {
            Ok(value) => Ok(value),
            Err(node::Error::Transform(_)) if self.error.is_some() => {
                let Some(error) = self.error else {
                    return Err(Evaluation::Cache(Error::Node(Box::new(
                        node::Error::State("checked transform failure disappeared"),
                    ))));
                };
                Err(Evaluation::Transform(error))
            }
            Err(error) => Err(Evaluation::Cache(Error::Node(Box::new(error)))),
        }
    }
}

impl Cached {
    /// Construct from this exact checked result, preserving retained provenance
    /// when the connected evaluator carries it across the consumer boundary.
    pub(crate) fn from_validated(
        context: crate::context::Context,
        product: node::Validated<Product>,
        retention: Option<node::read_set::Retention>,
    ) -> Result<Self, Error> {
        let chain = match retention {
            Some(retention) => node::Chain::validated_retained(context, &product, retention)
                .map_err(|error| Error::Node(Box::new(error)))?,
            None => node::Chain::validated(context, &product),
        };
        Ok(Self { product, chain })
    }

    /// Reconstruct the exact sealed symbolic demand under a caller-owned brand.
    pub fn replay_demand<B>(
        &self,
        brand: B,
        contract: &crate::Contract<crate::Classified>,
        snapshot: crate::surface::Snapshot,
        limits: &crate::cargo::Limits,
    ) -> Result<crate::Demand<crate::Sealed, B>, Error> {
        self.product
            .value()
            .replay_demand(brand, contract, snapshot, limits)
    }

    /// Return the complete checked-program profile fingerprint.
    #[must_use]
    pub fn checked_fingerprint(&self) -> [u8; 32] {
        self.product.value().checked
    }

    /// Borrow the exact profile-owned checked payload.
    #[must_use]
    pub fn checked_payload(&self) -> &[u8] {
        &self.product.value().payload
    }

    /// Borrow this checked node as the parent of profile-demand evaluation.
    #[must_use]
    pub const fn node_chain(&self) -> &node::Chain {
        &self.chain
    }
}

pub(crate) struct Product {
    retained_items: usize,
    retained_bytes: usize,
    checked: [u8; 32],
    payload: Box<[u8]>,
    demand: Box<[u8]>,
}

impl Product {
    pub(crate) fn capture<B, C: crate::profile::Checked>(
        demand: &crate::Demand<crate::Sealed, B>,
        checked: &C,
        contract: crate::contract::fingerprint::Fingerprint,
        snapshot: crate::surface::Snapshot,
        limits: &crate::cargo::Limits,
    ) -> Result<Self, Error> {
        if demand.contract_fingerprint() != contract || demand.snapshot() != snapshot {
            return Err(Error::DemandDrift);
        }
        let payload = checked.profile_payload();
        // The payload size is an accounting target recorded on a
        // classification-stage account; the capture continues.
        crate::classify::Account::new().exceeded(
            "checked application profile payload bytes",
            limits.string_bytes,
            payload.len(),
        );
        let mut retained_payload = Vec::new();
        retained_payload
            .try_reserve_exact(payload.len())
            .map_err(|_| Error::Allocation)?;
        retained_payload.extend_from_slice(payload);
        let canonical = demand.canonical_bytes();
        let mut retained_demand = Vec::new();
        retained_demand
            .try_reserve_exact(canonical.len())
            .map_err(|_| Error::Allocation)?;
        retained_demand.extend_from_slice(canonical);
        let (retained_items, retained_bytes) = demand.cached_retained_usage();
        Ok(Self {
            retained_items,
            retained_bytes,
            checked: checked.profile_fingerprint(),
            payload: retained_payload.into_boxed_slice(),
            demand: retained_demand.into_boxed_slice(),
        })
    }

    /// Identity of the exact symbolic demand bytes a resolved product replays.
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn demand_digest(&self) -> [u8; 32] {
        use sha2::Digest as _;
        sha2::Sha256::digest(&self.demand).into()
    }

    /// Replay this demand with plans already resolved under its contract.
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn replay_resolved<B>(
        &self,
        brand: B,
        contract: (crate::contract::fingerprint::Fingerprint, &node::Chain),
        allocation: crate::contract::Allocation,
        snapshot: crate::surface::Snapshot,
        limits: &crate::cargo::Limits,
        plans: Vec<crate::surface::adapter::Plan>,
    ) -> Result<crate::Demand<crate::Sealed, B>, Error> {
        crate::classify::Account::new().exceeded(
            "cached checked profile payload bytes",
            limits.string_bytes,
            self.payload.len(),
        );
        crate::Demand::replay_resolved(
            &self.demand,
            (self.retained_items, self.retained_bytes),
            brand,
            contract,
            allocation,
            snapshot,
            limits,
            plans,
        )
        .map_err(Error::Surface)
    }

    fn replay_demand<B>(
        &self,
        brand: B,
        contract: &crate::Contract<crate::Classified>,
        snapshot: crate::surface::Snapshot,
        limits: &crate::cargo::Limits,
    ) -> Result<crate::Demand<crate::Sealed, B>, Error> {
        crate::classify::Account::new().exceeded(
            "cached checked profile payload bytes",
            limits.string_bytes,
            self.payload.len(),
        );
        crate::Demand::replay_cached(
            &self.demand,
            (self.retained_items, self.retained_bytes),
            brand,
            contract,
            snapshot,
            limits,
        )
        .map_err(Error::Surface)
    }

    fn encoded_len(&self) -> usize {
        HEADER_BYTES
            .saturating_add(self.payload.len())
            .saturating_add(self.demand.len())
    }
}

impl node::Value for Product {
    const KIND: node::Kind = node::Kind::Checked;
    const SCHEMA: u32 = 1;

    fn encode(&self, limit: usize) -> Result<Box<[u8]>, node::Error> {
        let length = self.encoded_len();
        if length > limit {
            return Err(node::Error::Limit {
                resource: "cached checked application bytes",
                limit,
                actual: length,
            });
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| node::Error::Allocation)?;
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(
            &u64::try_from(self.retained_items)
                .unwrap_or(u64::MAX)
                .to_le_bytes(),
        );
        bytes.extend_from_slice(
            &u64::try_from(self.retained_bytes)
                .unwrap_or(u64::MAX)
                .to_le_bytes(),
        );
        bytes.extend_from_slice(&self.checked);
        bytes.extend_from_slice(
            &u64::try_from(self.payload.len())
                .unwrap_or(u64::MAX)
                .to_le_bytes(),
        );
        bytes.extend_from_slice(
            &u64::try_from(self.demand.len())
                .unwrap_or(u64::MAX)
                .to_le_bytes(),
        );
        bytes.extend_from_slice(&[0; 24]);
        bytes.extend_from_slice(&self.payload);
        bytes.extend_from_slice(&self.demand);
        if bytes.len() != length {
            return Err(node::Error::State(
                "checked application encoder length drifted",
            ));
        }
        Ok(bytes.into_boxed_slice())
    }

    fn decode(bytes: &[u8], limit: usize) -> Result<Self, node::Error> {
        if bytes.len() > limit {
            return Err(node::Error::Limit {
                resource: "cached checked application bytes",
                limit,
                actual: bytes.len(),
            });
        }
        if bytes.len() < HEADER_BYTES || &bytes[..8] != MAGIC || bytes[72..96] != [0; 24] {
            return Err(node::Error::Codec(
                "cached checked application header changed",
            ));
        }
        let retained_items = usize_at(bytes, 8, "checked retained items are truncated")?;
        let retained_bytes = usize_at(bytes, 16, "checked retained bytes are truncated")?;
        let checked = bytes[24..56]
            .try_into()
            .map_err(|_| node::Error::Codec("checked fingerprint is truncated"))?;
        let payload_len = usize_at(bytes, 56, "checked payload length is truncated")?;
        let demand_len = usize_at(bytes, 64, "checked demand length is truncated")?;
        let expected = HEADER_BYTES
            .checked_add(payload_len)
            .and_then(|length| length.checked_add(demand_len))
            .ok_or(node::Error::Codec(
                "checked application length is not addressable",
            ))?;
        if expected != bytes.len() || demand_len == 0 {
            return Err(node::Error::Codec(
                "cached checked application framing changed",
            ));
        }
        let payload_end = HEADER_BYTES + payload_len;
        Ok(Self {
            retained_items,
            retained_bytes,
            checked,
            payload: bytes[HEADER_BYTES..payload_end].to_vec().into_boxed_slice(),
            demand: bytes[payload_end..].to_vec().into_boxed_slice(),
        })
    }
}

fn usize_at(bytes: &[u8], offset: usize, reason: &'static str) -> Result<usize, node::Error> {
    let encoded: [u8; 8] = bytes
        .get(offset..offset.saturating_add(8))
        .ok_or(node::Error::Codec(reason))?
        .try_into()
        .map_err(|_| node::Error::Codec(reason))?;
    usize::try_from(u64::from_le_bytes(encoded)).map_err(|_| node::Error::Codec(reason))
}

/// Checked-node construction, validation, or replay failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A required generic, facade, or profile parent was not DAG-validated.
    #[error("checked application requires current generic, facade, and profile parents")]
    ForeignEvidence,
    /// Generic and profile classifications refer to different API graphs.
    #[error("checked application parents disagree on their imported API graph")]
    DefinitionDrift,
    /// The cold transform returned demand for another contract or source snapshot.
    #[error("checked application transform returned demand for different inputs")]
    DemandDrift,
    /// A checked-node payload exceeded its existing bounded authority.
    #[error("{resource} exceeded limit {limit} with observed size {actual}")]
    Limit {
        resource: &'static str,
        limit: usize,
        actual: usize,
    },
    /// Exact checked-node bytes could not be retained.
    #[error("checked application cache allocation failed")]
    Allocation,
    /// Cached symbolic demand reconstruction failed.
    #[error(transparent)]
    Surface(#[from] crate::surface::Error),
    /// The shared content-addressed node failed validation or storage.
    #[error("checked application node failed: {0}")]
    Node(#[source] Box<node::Error>),
}

#[cfg(all(test, feature = "test-fixtures"))]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    use crate::node::Value as _;

    use super::{Cached, Product};

    struct Checked;
    struct Definition;

    impl crate::profile::Checked for Checked {
        fn profile_fingerprint(&self) -> [u8; 32] {
            [0x63; 32]
        }

        fn profile_payload(&self) -> &[u8] {
            b"checked-payload"
        }
    }

    impl crate::profile::Definition<Checked> for Definition {
        fn identity(&self) -> crate::profile::Identity {
            crate::profile::Identity::new("rawr.checked.fixture", &crate::cargo::Limits::standard())
                .expect("the checked fixture identity seals")
        }

        fn version(&self) -> crate::profile::Version {
            crate::profile::Version::new(1).expect("the checked fixture version seals")
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

    fn snapshot(limits: &crate::cargo::Limits) -> crate::surface::Snapshot {
        let mut builder = crate::surface::Snapshot::builder(limits);
        builder
            .entry("/fixture/rawr.toml")
            .expect("the checked snapshot entry is bounded");
        builder
            .file("/fixture/main.rawr", b"Pair")
            .expect("the checked snapshot source is bounded");
        builder.seal().expect("the checked snapshot seals")
    }

    fn demand(
        contract: &crate::Contract<crate::Classified>,
        source: crate::surface::Snapshot,
        limits: &crate::cargo::Limits,
    ) -> crate::Demand<crate::Sealed, ()> {
        let identity = contract
            .outcomes()
            .find(|outcome| outcome.representability().is_demandable())
            .expect("the checked fixture has a demandable outcome")
            .identity()
            .clone();
        let mut compiler = contract
            .compiler((), source, limits)
            .expect("the checked fixture compiler starts");
        compiler
            .require(
                identity,
                crate::surface::Origin::checked("/fixture/main.rawr", 0, 4, limits)
                    .expect("the checked fixture origin is bounded"),
            )
            .expect("the checked fixture demand is admitted");
        compiler.seal().expect("the checked fixture demand seals")
    }

    #[test]
    fn owned_checked_authority_preserves_canonical_bytes_and_rejects_graph_drift() {
        let limits = crate::cargo::Limits::standard();
        let graph = crate::test_fixture::graph();
        let profile = crate::profile::classify(&Definition, &graph, &limits)
            .expect("the checked authority profile classifies");
        let contract = graph
            .classify(&crate::classify::Policy::standard(), &limits)
            .expect("the checked authority fixture classifies");
        let source = snapshot(&limits);
        let authority = super::authority(
            contract.fingerprint(),
            contract.raw_fingerprint(),
            profile.identity(),
            profile.version(),
            profile.rules_digest(),
            profile.graph_fingerprint(),
            source,
            Some(
                &contract
                    .raw_graph()
                    .callback_observations(contract.raw_graph().freshness_fingerprint()),
            ),
        )
        .expect("owned checked identity prepares");
        let mut expected = super::DOMAIN.to_vec();
        expected.extend_from_slice(contract.fingerprint().as_bytes());
        expected.extend_from_slice(profile.identity().digest());
        expected.extend_from_slice(&profile.version().value().to_le_bytes());
        expected.extend_from_slice(profile.rules_digest());
        expected.extend_from_slice(source.as_bytes());
        expected.push(1);
        expected.extend_from_slice(
            &contract
                .raw_graph()
                .callback_observations(contract.raw_graph().freshness_fingerprint()),
        );
        assert_eq!(authority, expected);
        let mut foreign = *profile.graph_fingerprint().as_bytes();
        foreign[0] ^= 1;
        assert!(matches!(
            super::authority(
                contract.fingerprint(),
                contract.raw_fingerprint(),
                profile.identity(),
                profile.version(),
                profile.rules_digest(),
                crate::api::Fingerprint::from_bytes(foreign),
                source,
                None,
            ),
            Err(super::Error::DefinitionDrift)
        ));
    }

    #[test]
    fn checked_transform_preserves_typed_source_failure_and_cache_failure() {
        let limits = crate::cargo::Limits::standard();
        let contract = crate::test_fixture::graph()
            .classify(&crate::classify::Policy::standard(), &limits)
            .expect("the checked transform fixture classifies");
        let source = snapshot(&limits);
        let mut transform = super::Transform::new();
        let result =
            transform
                .capture::<(), Checked>(contract.fingerprint(), source, &limits, || Err(37_u32));
        assert!(matches!(
            transform.resolve(result),
            Err(super::Evaluation::Transform(37))
        ));

        let mut transform = super::Transform::new();
        let _ = transform
            .capture::<(), Checked>(contract.fingerprint(), source, &limits, || Err(37_u32));
        assert!(matches!(
            transform.resolve::<()>(Err(crate::node::Error::State("cleanup failure"))),
            Err(super::Evaluation::Cache(super::Error::Node(_)))
        ));
        assert!(matches!(
            super::Transform::<u32>::new().resolve::<()>(Err(crate::node::Error::Transform(
                "product capture failed".into()
            ))),
            Err(super::Evaluation::Cache(super::Error::Node(_)))
        ));
    }

    #[test]
    fn checked_product_roundtrips_and_rejects_framing_drift() {
        let limits = crate::cargo::Limits::standard();
        let contract = crate::test_fixture::graph()
            .classify(&crate::classify::Policy::standard(), &limits)
            .expect("the checked fixture classifies");
        let source = snapshot(&limits);
        let product = Product::capture(
            &demand(&contract, source, &limits),
            &Checked,
            contract.fingerprint(),
            source,
            &limits,
        )
        .expect("the checked product captures");
        let bytes = product
            .encode(limits.node_output_bytes())
            .expect("the checked product encodes");
        let decoded = Product::decode(&bytes, limits.node_output_bytes())
            .expect("the checked product decodes");
        assert_eq!(decoded.checked, [0x63; 32]);
        assert_eq!(decoded.payload.as_ref(), b"checked-payload");
        assert!(Product::decode(&bytes[..bytes.len() - 1], limits.node_output_bytes()).is_err());
        let mut reserved = bytes.into_vec();
        reserved[72] = 1;
        assert!(Product::decode(&reserved, limits.node_output_bytes()).is_err());
    }

    #[test]
    fn identical_checked_node_replays_without_a_second_transform() {
        let project = crate::store::TestProject::new("rawr-checked-dag");
        let root = project.path();
        let cargo_home = root.join("cargo-home");
        fs::create_dir(&cargo_home).expect("the checked DAG Cargo home creates");
        let limits = crate::cargo::Limits::standard();
        let context = crate::context::Context::capture(
            root,
            crate::cargo::config::Environment::from_pairs([(
                "CARGO_HOME",
                cargo_home
                    .to_str()
                    .expect("the checked DAG Cargo home is Unicode"),
            )])
            .expect("the checked DAG environment seals"),
            "aarch64-apple-darwin",
            &limits,
        )
        .expect("the checked DAG context seals");
        let parent =
            crate::node::Chain::fixture(context).expect("the checked DAG fixture parent validates");
        let graph = crate::test_fixture::graph().with_node_chain_for_test(parent);
        let profile = crate::profile::classify(&Definition, &graph, &limits)
            .expect("the checked profile classifies");
        let contract = graph
            .classify(&crate::classify::Policy::standard(), &limits)
            .expect("the checked generic contract classifies");
        let facade = crate::surface::facade::Plan::encode_cached(&contract, &limits)
            .expect("the checked facade seals");
        let source = snapshot(&limits);
        let executions = AtomicUsize::new(0);

        let evaluate = || {
            executions.fetch_add(1, Ordering::Relaxed);
            Ok::<_, ()>((demand(&contract, source, &limits), Checked))
        };
        let cold_started = Instant::now();
        let cold: Cached = super::evaluate(&contract, &profile, &facade, source, &limits, evaluate)
            .expect("the cold checked node publishes");
        let cold_demand = cold
            .replay_demand((), &contract, source, &limits)
            .expect("the cold checked demand replays");
        let cold_elapsed = cold_started.elapsed();
        assert_eq!(cold.checked_fingerprint(), [0x63; 32]);
        assert_eq!(cold.checked_payload(), b"checked-payload");

        let warm_started = Instant::now();
        let warm: Cached = super::evaluate(&contract, &profile, &facade, source, &limits, || {
            executions.fetch_add(1, Ordering::Relaxed);
            Ok::<_, ()>((demand(&contract, source, &limits), Checked))
        })
        .expect("the warm checked node validates");
        let warm_demand = warm
            .replay_demand((), &contract, source, &limits)
            .expect("the warm checked demand replays");
        let warm_elapsed = warm_started.elapsed();
        assert_eq!(executions.load(Ordering::Relaxed), 1);
        assert_eq!(cold_demand.canonical_bytes(), warm_demand.canonical_bytes());
        assert_eq!(cold_demand.fingerprint(), warm_demand.fingerprint());
        assert!(warm.node_chain().parent().kind() == crate::node::Kind::Checked);
        eprintln!(
            "CHECKED_DAG cold_us={} warm_us={} saved_us={} cold_transform=1 warm_transform=0",
            cold_elapsed.as_micros(),
            warm_elapsed.as_micros(),
            cold_elapsed.saturating_sub(warm_elapsed).as_micros(),
        );
    }
}
