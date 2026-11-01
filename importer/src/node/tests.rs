use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

use sha2::{Digest as _, Sha256};

use super::{Error, Evaluator, Kind, Parent, Value};
use crate::cargo;
use crate::context::Context;
use crate::metrics::{self, Milestone, NodeEvent, NodeKind, ProcessKind, Sink};

#[cfg(all(feature = "test-fixtures", unix))]
mod bundle_release;
mod native;
#[cfg(all(feature = "test-fixtures", unix))]
mod recovery;

const ROOT_ENV: &str = "RAWR_DAG_SLICE_ROOT";
const REPORT_ENV: &str = "RAWR_DAG_SLICE_REPORT";
const WORKER: &str = "node::tests::fresh_process_worker";

static TEST_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Eq, PartialEq)]
struct Artifact(Box<[u8]>);

impl Value for Artifact {
    const KIND: Kind = Kind::NativeProduct;
    const SCHEMA: u32 = 1;

    fn encode(&self, limit: usize) -> Result<Box<[u8]>, Error> {
        if self.0.len() > limit {
            return Err(Error::Limit {
                resource: "fixture artifact bytes",
                limit,
                actual: self.0.len(),
            });
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(self.0.len())
            .map_err(|_| Error::Allocation)?;
        bytes.extend_from_slice(&self.0);
        Ok(bytes.into_boxed_slice())
    }

    fn decode(bytes: &[u8], limit: usize) -> Result<Self, Error> {
        if bytes.len() > limit {
            return Err(Error::Limit {
                resource: "fixture artifact bytes",
                limit,
                actual: bytes.len(),
            });
        }
        let mut retained = Vec::new();
        retained
            .try_reserve_exact(bytes.len())
            .map_err(|_| Error::Allocation)?;
        retained.extend_from_slice(bytes);
        Ok(Self(retained.into_boxed_slice()))
    }
}

#[derive(Debug, Eq, PartialEq)]
struct Derived(Box<[u8]>);

impl Value for Derived {
    const KIND: Kind = Kind::Facade;
    const SCHEMA: u32 = 1;

    fn encode(&self, limit: usize) -> Result<Box<[u8]>, Error> {
        retain_fixture_bytes(&self.0, limit, "fixture derived bytes")
    }

    fn decode(bytes: &[u8], limit: usize) -> Result<Self, Error> {
        retain_fixture_bytes(bytes, limit, "fixture derived bytes").map(Self)
    }
}

#[cfg(feature = "test-fixtures")]
pub(super) struct BundleValue {
    reference: crate::store::bundle::BundleRef,
    seal_length: usize,
    seal_digest: [u8; 32],
}

#[cfg(feature = "test-fixtures")]
struct NativeBundleValue(BundleValue);

#[cfg(feature = "test-fixtures")]
impl BundleValue {
    const BYTES: usize = 104;
    const SEAL: &'static [u8] = b"rawr.node.bundled-fixture/v1";

    fn capture(reference: crate::store::bundle::BundleRef) -> Self {
        Self {
            reference,
            seal_length: Self::SEAL.len(),
            seal_digest: Sha256::digest(Self::SEAL).into(),
        }
    }

    fn validate_pending(
        &self,
        pending: &crate::store::bundle::Pending,
    ) -> Result<Box<[u8]>, Error> {
        pending.validate_unpublished(self.reference, self.seal_length, self.seal_digest)?;
        self.encode(4_096)
    }

    fn validate_published(&self, store: &crate::store::Store) -> Result<Box<[u8]>, Error> {
        let canonical = self.encode(4_096)?;
        crate::store::bundle::validate_complete(
            store.root(),
            self.reference,
            Self::KIND,
            Self::SCHEMA,
            super::content_digest::<Self>(&canonical),
            self.seal_length,
            self.seal_digest,
            store.graph_budget(),
        )?;
        self.encode(4_096)
    }
}

#[cfg(feature = "test-fixtures")]
impl Value for BundleValue {
    const KIND: Kind = Kind::Runner;
    const SCHEMA: u32 = 2;

    fn encode(&self, limit: usize) -> Result<Box<[u8]>, Error> {
        if Self::BYTES > limit {
            return Err(Error::Limit {
                resource: "bundled fixture value bytes",
                limit,
                actual: Self::BYTES,
            });
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(Self::BYTES)
            .map_err(|_| Error::Allocation)?;
        bytes.extend_from_slice(&self.reference.execution().as_bytes());
        bytes.extend_from_slice(&self.reference.id().as_bytes());
        bytes.extend_from_slice(&(self.seal_length as u64).to_le_bytes());
        bytes.extend_from_slice(&self.seal_digest);
        Ok(bytes.into_boxed_slice())
    }

    fn decode(bytes: &[u8], limit: usize) -> Result<Self, Error> {
        if bytes.len() != Self::BYTES || bytes.len() > limit {
            return Err(Error::Codec("bundled fixture value shape changed"));
        }
        let array = |offset: usize| {
            bytes
                .get(offset..offset + 32)
                .and_then(|bytes| bytes.try_into().ok())
                .ok_or(Error::Codec("bundled fixture value is truncated"))
        };
        Ok(Self {
            reference: crate::store::bundle::BundleRef::new(
                super::ExecutionKey::from_bytes(array(0)?),
                crate::store::bundle::BundleId::from_bytes(array(32)?),
            ),
            seal_length: usize::try_from(u64::from_le_bytes(
                bytes[64..72]
                    .try_into()
                    .map_err(|_| Error::Codec("bundled fixture seal length is truncated"))?,
            ))
            .map_err(|_| Error::Codec("bundled fixture seal length is not addressable"))?,
            seal_digest: array(72)?,
        })
    }
}

#[cfg(feature = "test-fixtures")]
impl NativeBundleValue {
    fn capture(reference: crate::store::bundle::BundleRef) -> Self {
        Self(BundleValue::capture(reference))
    }

    fn validate_pending(
        &self,
        pending: &crate::store::bundle::Pending,
    ) -> Result<Box<[u8]>, Error> {
        pending.validate_unpublished(self.0.reference, self.0.seal_length, self.0.seal_digest)?;
        self.encode(4_096)
    }

    fn validate_published(&self, store: &crate::store::Store) -> Result<Box<[u8]>, Error> {
        let canonical = self.encode(4_096)?;
        crate::store::bundle::validate_complete(
            store.root(),
            self.0.reference,
            Self::KIND,
            Self::SCHEMA,
            super::content_digest::<Self>(&canonical),
            self.0.seal_length,
            self.0.seal_digest,
            store.graph_budget(),
        )?;
        Ok(canonical)
    }
}

#[cfg(feature = "test-fixtures")]
impl Value for NativeBundleValue {
    const KIND: Kind = Kind::NativeProduct;
    const SCHEMA: u32 = 2;

    fn encode(&self, limit: usize) -> Result<Box<[u8]>, Error> {
        self.0.encode(limit)
    }

    fn decode(bytes: &[u8], limit: usize) -> Result<Self, Error> {
        BundleValue::decode(bytes, limit).map(Self)
    }
}

#[cfg(feature = "test-fixtures")]
fn bundled_fixture(
    attempt: crate::store::bundle::Attempt,
) -> Result<super::Bundled<BundleValue>, Error> {
    fs::write(attempt.generated().join("input"), b"generated")
        .map_err(|error| Error::Transform(error.to_string().into()))?;
    let value = BundleValue::capture(attempt.reference());
    let pending = attempt.seal(Box::from(BundleValue::SEAL))?;
    super::Bundled::validate(value, pending, |value, pending| {
        value.validate_pending(pending)
    })
}

/// Registered positive bundle fixture for retention successor tests.
#[cfg(feature = "test-fixtures")]
pub(super) fn registered_retention_parent(
    store: &crate::store::Store,
) -> (
    super::Validated<BundleValue>,
    crate::store::bundle::BundleCommitment,
) {
    registered_retention_parent_in(store, [1; 32], &[])
}

#[cfg(feature = "test-fixtures")]
pub(super) fn registered_retention_parent_in(
    store: &crate::store::Store,
    context: [u8; 32],
    parents: &[Parent],
) -> (
    super::Validated<BundleValue>,
    crate::store::bundle::BundleCommitment,
) {
    let recipe =
        super::Discoverable::capture(context, b"retention-bundled-parent", 4_096, parents).unwrap();
    let validation_store = store.clone();
    let value = Evaluator::new(store, &crate::metrics::Current)
        .discover_bundled(
            &recipe,
            move |value| value.validate_published(&validation_store),
            bundled_fixture,
        )
        .unwrap();
    let product = value.value();
    let commitment = crate::store::bundle::BundleCommitment::new(
        product.reference,
        product.seal_length,
        product.seal_digest,
    );
    (value, commitment)
}

#[cfg(feature = "test-fixtures")]
fn native_bundled_fixture(
    attempt: crate::store::bundle::Attempt,
) -> Result<super::Bundled<NativeBundleValue>, Error> {
    fs::write(attempt.generated().join("input"), b"native-generated")
        .map_err(|error| Error::Transform(error.to_string().into()))?;
    let value = NativeBundleValue::capture(attempt.reference());
    let pending = attempt.seal(Box::from(BundleValue::SEAL))?;
    super::Bundled::validate(value, pending, |value, pending| {
        value.validate_pending(pending)
    })
}

fn retain_fixture_bytes(
    bytes: &[u8],
    limit: usize,
    resource: &'static str,
) -> Result<Box<[u8]>, Error> {
    if bytes.len() > limit {
        return Err(Error::Limit {
            resource,
            limit,
            actual: bytes.len(),
        });
    }
    let mut retained = Vec::new();
    retained
        .try_reserve_exact(bytes.len())
        .map_err(|_| Error::Allocation)?;
    retained.extend_from_slice(bytes);
    Ok(retained.into_boxed_slice())
}

#[derive(Default)]
struct Counts {
    hit: AtomicUsize,
    miss: AtomicUsize,
    published: AtomicUsize,
    rejected: AtomicUsize,
}

impl Sink for Counts {
    fn node(&self, kind: NodeKind, event: NodeEvent) {
        assert!(matches!(
            kind,
            NodeKind::NativeProduct | NodeKind::Facade | NodeKind::Runner
        ));
        match event {
            NodeEvent::ValidatedHit => &self.hit,
            NodeEvent::ExecutedMiss => &self.miss,
            NodeEvent::RejectedHit => &self.rejected,
            NodeEvent::Published => &self.published,
        }
        .fetch_add(1, Ordering::Relaxed);
    }

    fn process(&self, _kind: ProcessKind) {}

    fn milestone(&self, _milestone: Milestone, _observed_ns: Option<u64>) {}

    fn runner_owned(&self, _owned: &metrics::runner::Owned) {}

    fn scope_opened(&self) {}

    fn scope_joined(&self) {}
}

#[test]
#[ignore = "launched only by the fresh-process DAG slice regression"]
fn fresh_process_worker() {
    let root =
        PathBuf::from(std::env::var_os(ROOT_ENV).expect("the worker receives the store root"));
    let report_path =
        PathBuf::from(std::env::var_os(REPORT_ENV).expect("the worker receives its report path"));
    let context = fixture_context(&root);
    let store = context
        .open_store()
        .expect("the fresh worker opens the shared typed store");
    let counts = Counts::default();
    let transforms = AtomicUsize::new(0);
    let children = AtomicUsize::new(0);
    let recipe = context
        .direct::<Artifact>(
            b"rawr.phase9.native-product.synthetic/v1\0exact-demand",
            4_096,
        )
        .expect("the direct recipe seals");
    let evaluator = Evaluator::new(&store, &counts);
    let validated = evaluator
        .direct(&recipe, || {
            transforms.fetch_add(1, Ordering::Relaxed);
            let status = Command::new(
                std::env::current_exe()
                    .map_err(|error| Error::Transform(error.to_string().into_boxed_str()))?,
            )
            .arg("--help")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|error| Error::Transform(error.to_string().into_boxed_str()))?;
            if !status.success() {
                return Err(Error::Transform(
                    "synthetic transform child exited unsuccessfully".into(),
                ));
            }
            children.fetch_add(1, Ordering::Relaxed);
            Ok(Artifact(
                b"sealed-native-product".to_vec().into_boxed_slice(),
            ))
        })
        .expect("the direct node executes or validates");
    assert_eq!(validated.value().0.as_ref(), b"sealed-native-product");
    assert_eq!(validated.receipt().action(), recipe.action());
    assert_eq!(validated.receipt().recipe_digest(), recipe.recipe);
    assert_eq!(
        validated.receipt().content_digest(),
        validated.content().digest()
    );
    let report = format!(
        "hits={}\nmisses={}\npublished={}\nrejected={}\ntransforms={}\nchildren={}\n",
        counts.hit.load(Ordering::Relaxed),
        counts.miss.load(Ordering::Relaxed),
        counts.published.load(Ordering::Relaxed),
        counts.rejected.load(Ordering::Relaxed),
        transforms.load(Ordering::Relaxed),
        children.load(Ordering::Relaxed),
    );
    fs::write(&report_path, report.as_bytes()).expect("the worker writes its test report");
}

#[test]
fn direct_node_seals_once_and_a_fresh_process_reuses_without_transform_children() {
    let root = temporary("direct-reuse");
    let context = prepare_fixture(&root);
    let store_root = context.rawr().join("store");
    let first_report = root.with_extension("first.report");
    let second_report = root.with_extension("second.report");

    run_worker(&root, &first_report);
    let first = parse_report(&first_report);
    assert_eq!(first.get("hits"), Some(&0));
    assert_eq!(first.get("misses"), Some(&1));
    assert_eq!(first.get("published"), Some(&1));
    assert_eq!(first.get("rejected"), Some(&0));
    assert_eq!(first.get("transforms"), Some(&1));
    assert_eq!(first.get("children"), Some(&1));
    let sealed = snapshot(&store_root);
    assert!(
        sealed
            .keys()
            .any(|path| path.to_string_lossy().ends_with("/payload")),
        "the first run publishes one content-addressed payload"
    );
    assert!(
        sealed
            .keys()
            .any(|path| path.to_string_lossy().ends_with("/envelope")),
        "the first run publishes one action envelope"
    );

    run_worker(&root, &second_report);
    let second = parse_report(&second_report);
    assert_eq!(second.get("hits"), Some(&1));
    assert_eq!(second.get("misses"), Some(&0));
    assert_eq!(second.get("published"), Some(&0));
    assert_eq!(second.get("rejected"), Some(&0));
    assert_eq!(second.get("transforms"), Some(&0));
    assert_eq!(second.get("children"), Some(&0));
    assert_eq!(
        snapshot(&store_root),
        sealed,
        "a healthy hit is durably inert"
    );

    fs::remove_file(first_report).expect("the first report retires");
    fs::remove_file(second_report).expect("the second report retires");
    fs::remove_dir_all(root).expect("the typed store fixture retires");
}

#[test]
fn legacy_action_namespace_clean_misses_without_upgrade_or_mutation() {
    let root = temporary("legacy-action-namespace");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the typed store opens");
    let recipe = context
        .direct::<Artifact>(b"legacy-action-namespace", 4_096)
        .expect("the current direct recipe seals");
    let legacy = store
        .root()
        .join("actions/native-product/0001")
        .join(recipe.action().directory());
    fs::create_dir_all(&legacy).expect("the legacy action directory exists");
    fs::write(legacy.join("envelope"), b"legacy-format")
        .expect("the legacy action evidence exists");
    let legacy_before = snapshot(&legacy);
    let executions = AtomicUsize::new(0);
    let counts = Counts::default();

    let first = Evaluator::new(&store, &counts)
        .direct(&recipe, || {
            executions.fetch_add(1, Ordering::Relaxed);
            Ok(Artifact(b"current-format".to_vec().into_boxed_slice()))
        })
        .expect("the legacy namespace is one clean current-format miss");
    let current = store
        .root()
        .join("actions/native-product")
        .join(super::STORAGE_DIRECTORY)
        .join(first.receipt().action().directory());
    assert!(current.is_dir(), "the current namespace must publish");
    assert_eq!(
        snapshot(&legacy),
        legacy_before,
        "legacy evidence must not be upgraded or mutated in place"
    );

    Evaluator::new(&store, &counts)
        .direct(&recipe, || {
            executions.fetch_add(1, Ordering::Relaxed);
            Err(Error::State("the current-format hit executed again"))
        })
        .expect("the current-format action reuses beside legacy evidence");
    assert_eq!(executions.load(Ordering::Relaxed), 1);
    assert_eq!(counts.miss.load(Ordering::Relaxed), 1);
    assert_eq!(counts.published.load(Ordering::Relaxed), 1);
    assert_eq!(counts.hit.load(Ordering::Relaxed), 1);
    assert_eq!(counts.rejected.load(Ordering::Relaxed), 0);

    store
        .retire_fixture()
        .expect("the legacy namespace fixture store retires");
    fs::remove_dir_all(root).expect("the legacy namespace fixture retires");
}

#[test]
fn legacy_allocation_is_a_bounded_first_class_reclamation_candidate() {
    let root = temporary("legacy-allocation-reclamation");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the typed store opens");
    let recipe = context
        .direct::<Artifact>(b"legacy-allocation-reclamation", 4_096)
        .expect("the current direct recipe seals");
    let legacy = store
        .root()
        .join("actions/native-product/0001")
        .join(recipe.action().directory());
    fs::create_dir_all(&legacy).expect("the legacy allocation exists");
    fs::write(legacy.join("envelope"), b"legacy-format")
        .expect("the legacy allocation payload exists");

    assert!(store
        .fixture_evict_one(false)
        .expect("the legacy garbage allocation reclaims"));
    assert!(!legacy.exists());
    fs::remove_dir_all(root).expect("the legacy reclamation fixture retires");
}

#[test]
fn unrooted_non_runner_allocation_is_reclaimed_as_one_whole_unit() {
    let root = temporary("unrooted-non-runner-eviction");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the typed store opens");
    let counts = Counts::default();
    let executions = AtomicUsize::new(0);
    let recipe = context
        .direct::<Artifact>(b"unrooted-non-runner", 4_096)
        .expect("the non-runner recipe seals");
    let evaluator = Evaluator::new(&store, &counts);
    evaluator
        .direct(&recipe, || {
            executions.fetch_add(1, Ordering::Relaxed);
            Ok(Artifact(b"unrooted-output".to_vec().into_boxed_slice()))
        })
        .expect("the unrooted node publishes");

    assert!(store
        .fixture_evict_one(false)
        .expect("the unrooted non-runner unit evicts"));
    evaluator
        .direct(&recipe, || {
            executions.fetch_add(1, Ordering::Relaxed);
            Ok(Artifact(b"unrooted-output".to_vec().into_boxed_slice()))
        })
        .expect("the evicted unit executes again");
    assert_eq!(executions.load(Ordering::Relaxed), 2);
    fs::remove_dir_all(root).expect("the unrooted non-runner fixture retires");
}

// A project keeps its newest few unrooted units of a kind; older ones
// are retired whole, and a retired one executes again when asked.
#[test]
fn a_project_keeps_only_its_newest_unrooted_units_of_a_kind() {
    let root = temporary("unrooted-excess");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the typed store opens");
    let counts = Counts::default();
    let executions = AtomicUsize::new(0);
    let evaluator = Evaluator::new(&store, &counts);
    let recipes = (0..6)
        .map(|index| {
            context
                .direct::<Artifact>(format!("excess-{index}").as_bytes(), 4_096)
                .expect("the recipe seals")
        })
        .collect::<Vec<_>>();
    let evaluate = |recipe: &super::Direct<Artifact>| {
        evaluator
            .direct(recipe, || {
                executions.fetch_add(1, Ordering::Relaxed);
                Ok(Artifact(b"excess-output".to_vec().into_boxed_slice()))
            })
            .expect("the unit publishes or hits");
    };
    for recipe in &recipes {
        evaluate(recipe);
    }
    assert_eq!(executions.load(Ordering::Relaxed), 6);
    assert_eq!(
        store
            .fixture_retire_excess(Artifact::KIND, 4)
            .expect("the excess retires"),
        2
    );
    assert_eq!(store.fixture_retire_excess(Artifact::KIND, 4).unwrap(), 0);
    for recipe in &recipes[2..] {
        evaluate(recipe);
    }
    assert_eq!(executions.load(Ordering::Relaxed), 6, "the newest four hit");
    evaluate(&recipes[0]);
    assert_eq!(
        executions.load(Ordering::Relaxed),
        7,
        "the oldest executes again"
    );
    fs::remove_dir_all(root).expect("the excess fixture retires");
}

// The store inventory checks a registered allocation's placement only, as
// for bundles; its record and payload are checked by the consumer
// that reads them, not re-read on every store write.
#[test]
fn physical_inventory_checks_registered_placement_and_consumers_check_bytes() {
    fn publish(root: &Path, request: &[u8]) -> (Context, super::Validated<Artifact>) {
        let context = prepare_fixture(root);
        let store = context.open_store().expect("the physical store opens");
        let recipe = context
            .direct::<Artifact>(request, 16 * 1024)
            .expect("the physical recipe seals");
        let validated = Evaluator::new(&store, &Counts::default())
            .direct(&recipe, || {
                Ok(Artifact(b"physical-output".to_vec().into_boxed_slice()))
            })
            .expect("the physical fixture publishes");
        (context, validated)
    }

    let bytes_root = temporary("physical-byte-drift");
    let (bytes_context, bytes) = publish(&bytes_root, b"physical-bytes");
    let bytes_store = bytes_context
        .open_store()
        .expect("the byte-drift store reopens");
    let content = bytes_store
        .root()
        .join("content/native-product/00000001")
        .join(bytes.content().digest().directory())
        .join("payload");
    fs::write(&content, vec![0_u8; 8 * 1024])
        .expect("the registered payload physical bytes change");
    assert!(bytes_store.fixture_physical_inventory().is_ok());
    let fresh = fixture_context(&bytes_root);
    let recipe = fresh
        .direct::<Artifact>(b"physical-bytes", 16 * 1024)
        .expect("the physical recipe seals again");
    let consumed = Evaluator::new(&fresh.open_store().unwrap(), &Counts::default())
        .direct(&recipe, || {
            Ok(Artifact(b"physical-output".to_vec().into_boxed_slice()))
        });
    assert!(consumed.map_or(true, |value| value.value().0.as_ref() == b"physical-output"));
    fs::remove_dir_all(bytes_root).expect("the byte-drift fixture retires");

    let lost_root = temporary("physical-lost-allocation");
    let (lost_context, lost) = publish(&lost_root, b"physical-lost");
    let lost_store = lost_context.open_store().expect("the lost store reopens");
    let action = lost_store
        .root()
        .join("actions/native-product")
        .join(super::STORAGE_DIRECTORY)
        .join(lost.receipt().action().directory());
    fs::remove_dir_all(&action).expect("the registered action directory is removed");
    assert!(lost_store.fixture_physical_inventory().is_err());
    fs::remove_dir_all(lost_root).expect("the lost-allocation fixture retires");
}

// A corrupt path, record, ledger or payload is charged in full as the hit
// is validated; only an oversized payload is a physical limit.
#[test]
fn direct_hit_charges_corrupt_path_decode_or_payload_hash_work() {
    #[derive(Clone, Copy, Debug)]
    enum Blocked {
        Path,
        Envelope,
        Allocation,
        Payload,
        Ledger,
        Hash,
    }
    for blocked in [
        Blocked::Path,
        Blocked::Envelope,
        Blocked::Allocation,
        Blocked::Payload,
        Blocked::Ledger,
        Blocked::Hash,
    ] {
        let root = temporary("direct-precharge");
        let context = prepare_fixture(&root);
        let store = context.open_store().unwrap();
        let recipe = context
            .direct::<Artifact>(b"direct-precharge", 4_096)
            .unwrap();
        let value = Evaluator::new(&store, &Counts::default())
            .direct(&recipe, || {
                Ok(Artifact(b"bounded".to_vec().into_boxed_slice()))
            })
            .unwrap();
        let action = store
            .root()
            .join("actions/native-product")
            .join(super::STORAGE_DIRECTORY)
            .join(recipe.action().directory());
        let content = store
            .root()
            .join("content/native-product/00000001")
            .join(value.content().digest().directory())
            .join("payload");
        let envelope = fs::metadata(action.join("envelope")).unwrap().len() as usize;
        let allocation = fs::metadata(action.join("allocation")).unwrap().len() as usize;
        let ledger = fs::metadata(store.root().join("ledgers/installation"))
            .unwrap()
            .len() as usize;
        let fresh = fixture_context(&root).open_store().unwrap();
        match blocked {
            Blocked::Path => fs::remove_file(action.join("envelope")).unwrap(),
            Blocked::Envelope => fs::write(action.join("envelope"), vec![0; envelope]).unwrap(),
            Blocked::Allocation => {
                fs::write(action.join("allocation"), vec![0; allocation]).unwrap();
            }
            Blocked::Ledger => {
                fs::write(store.root().join("ledgers/installation"), vec![0; ledger]).unwrap();
            }
            Blocked::Payload => fs::write(&content, vec![0; 4_097]).unwrap(),
            Blocked::Hash => fs::write(&content, b"changed").unwrap(),
        }
        let counts = Counts::default();
        let executions = AtomicUsize::new(0);
        let result = Evaluator::new(&fresh, &counts).direct(&recipe, || {
            executions.fetch_add(1, Ordering::Relaxed);
            Ok(Artifact(b"bounded".to_vec().into_boxed_slice()))
        });
        if matches!(blocked, Blocked::Payload) {
            // The oversized payload is a physical limit: it refuses before
            // any transform.
            assert!(
                matches!(result, Err(Error::Limit { resource, .. }) if resource == "typed store file bytes"),
                "{blocked:?}"
            );
            assert_eq!(executions.load(Ordering::Relaxed), 0, "{blocked:?}");
        } else {
            assert!(
                !matches!(result, Err(Error::Limit { .. })),
                "{blocked:?}: graph work never refuses"
            );
        }
        store.retire_fixture().unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}

/// Give an entry's stored payload a new status-change time, so its physical
/// identity changes and its memoized digests no longer apply.
fn touch_payload(store: &crate::store::Store, entry: super::read_set::Entry) {
    let payload = store
        .root()
        .join("content")
        .join(entry.kind.directory())
        .join(format!("{:08x}", entry.schema))
        .join(entry.content.directory())
        .join("payload");
    fs::set_permissions(&payload, fs::metadata(&payload).unwrap().permissions()).unwrap();
}

#[test]
fn parent_refresh_schedule_covers_cold_child_and_warm_publication() {
    let root = temporary("parent-refresh-schedule");
    let context = prepare_fixture(&root);
    let store = context.open_store().unwrap();
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let refreshes = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&refreshes);
    let recipe = context
        .discover::<Artifact>(b"refresh-parent", 4_096)
        .unwrap();
    let parent = evaluator
        .discover(
            &recipe,
            move |value| {
                observed.fetch_add(1, Ordering::Relaxed);
                value.encode(4_096)
            },
            || Ok(Artifact(Box::from(b"parent".as_slice()))),
        )
        .unwrap();
    let before_child = refreshes.load(Ordering::Relaxed);
    let recipe = context
        .direct_with_parents::<Derived>(b"refresh-child", 4_096, &[Parent::validated(&parent)])
        .unwrap();
    let child = evaluator
        .direct(&recipe, || {
            // Parent-union sealing and both sides of InFlight pin admission
            // happen before the cold child executes.
            assert_eq!(refreshes.load(Ordering::Relaxed) - before_child, 3);
            Ok(Derived(Box::from(b"child".as_slice())))
        })
        .unwrap();
    evaluator
        .seal_read_set([child.receipt().read_set().clone()])
        .unwrap();
    assert_eq!(refreshes.load(Ordering::Relaxed) - before_child, 4);

    let before_hit = refreshes.load(Ordering::Relaxed);
    let hit = evaluator
        .direct(&recipe, || {
            panic!("an identical child must reuse its product")
        })
        .unwrap();
    assert_eq!(refreshes.load(Ordering::Relaxed), before_hit);
    // A direct hit alone does not refresh its parent; product publication must
    // still seal the complete transitive union, as the API product path does.
    evaluator
        .seal_read_set([hit.receipt().read_set().clone()])
        .unwrap();
    assert_eq!(refreshes.load(Ordering::Relaxed) - before_hit, 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[cfg(feature = "test-fixtures")]
fn retained_bundle_reservation_refreshes_parents_only_at_the_consumer() {
    let root = temporary("retained-bundle-consumer");
    let context = prepare_fixture(&root);
    let store = context.open_store().unwrap();
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let refreshes = Arc::new(AtomicUsize::new(0));
    let reject = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed = Arc::clone(&refreshes);
    let rejected = Arc::clone(&reject);
    let parent = evaluator
        .discover(
            &context
                .discover::<Artifact>(b"retained-consumer-parent", 4096)
                .unwrap(),
            move |value| {
                observed.fetch_add(1, Ordering::Relaxed);
                if rejected.load(Ordering::Relaxed) {
                    return Err(Error::State("parent changed"));
                }
                value.encode(4096)
            },
            || Ok(Artifact(Box::from(b"parent".as_slice()))),
        )
        .unwrap();
    let (bundle, commitment) =
        registered_retention_parent_in(&store, context.identity(), &[Parent::validated(&parent)]);
    let retention = super::read_set::Retention::from_validated(&bundle, Some(commitment)).unwrap();
    let independent = evaluator
        .direct(
            &context
                .direct::<Derived>(b"retained-extra-toolchain", 4096)
                .unwrap(),
            || Ok(Derived(Box::from(b"toolchain".as_slice()))),
        )
        .unwrap();
    let parents = [
        Parent::validated(&bundle),
        Parent::validated(&parent),
        Parent::validated(&independent),
    ];
    let recipe = context
        .discover_with_parents::<BundleValue>(b"retained-consumer-child", 4096, &parents)
        .unwrap();
    let validation_store = store.clone();
    let validate = move |value: &BundleValue, _: &super::GraphBudget| {
        value.validate_published(&validation_store)
    };
    assert!(evaluator
        .discover_bundled_retained_budgeted(&recipe, &retention, validate.clone(), |_| {
            panic!("missing independent parent must reject before reservation")
        })
        .is_err());
    let retention = retention
        .union(&super::read_set::Retention::from_validated(&independent, None).unwrap())
        .unwrap();
    let before = refreshes.load(Ordering::Relaxed);
    let child = evaluator
        .discover_bundled_retained_budgeted(&recipe, &retention, validate.clone(), |attempt| {
            assert_eq!(
                refreshes.load(Ordering::Relaxed),
                before,
                "ownership does not replay mutable recipes"
            );
            recipe.revalidate_parents(&store)?;
            assert_eq!(refreshes.load(Ordering::Relaxed) - before, 1);
            bundled_fixture(attempt)
        })
        .unwrap();
    assert_eq!(refreshes.load(Ordering::Relaxed) - before, 1);
    let hit = evaluator
        .discover_bundled_retained_budgeted(&recipe, &retention, validate.clone(), |_| {
            panic!("warm candidate must not enter the miss consumer")
        })
        .unwrap();
    assert_eq!(hit.content().digest(), child.content().digest());
    assert_eq!(refreshes.load(Ordering::Relaxed) - before, 1);

    let missing = context
        .discover_with_parents::<BundleValue>(b"retained-consumer-refused", 4096, &parents)
        .unwrap();
    reject.store(true, Ordering::Relaxed);
    let pins = store.fixture_pin_counts().unwrap();
    let bundles = crate::store::bundle::count_bundles(store.root()).unwrap();
    let launched = std::sync::atomic::AtomicBool::new(false);
    let before = refreshes.load(Ordering::Relaxed);
    assert!(evaluator
        .discover_bundled_retained_budgeted(&missing, &retention, validate, |attempt| {
            assert_eq!(refreshes.load(Ordering::Relaxed), before);
            missing.revalidate_parents(&store)?;
            launched.store(true, Ordering::Relaxed);
            bundled_fixture(attempt)
        })
        .is_err());
    assert!(!launched.load(Ordering::Relaxed));
    assert_eq!(refreshes.load(Ordering::Relaxed) - before, 1);
    assert_eq!(store.fixture_pin_counts().unwrap(), pins);
    assert_eq!(
        crate::store::bundle::count_bundles(store.root()).unwrap(),
        bundles
    );
    store.retire_fixture().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn lookup_outcome_returns_the_usable_hit_without_mutable_witness_replay() {
    let root = temporary("lookup-owned-hit");
    let context = prepare_fixture(&root);
    let store = context.open_store().unwrap();
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let refreshes = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&refreshes);
    let parent = evaluator
        .discover(
            &context
                .discover::<Artifact>(b"lookup-parent", 4_096)
                .unwrap(),
            move |value| {
                observed.fetch_add(1, Ordering::Relaxed);
                value.encode(4_096)
            },
            || Ok(Artifact(Box::from(b"parent".as_slice()))),
        )
        .unwrap();
    let recipe = context
        .direct_with_parents::<Derived>(b"lookup-child", 4_096, &[Parent::validated(&parent)])
        .unwrap();
    let published = evaluator
        .direct(&recipe, || Ok(Derived(Box::from(b"child".as_slice()))))
        .unwrap();
    let before_refreshes = refreshes.load(Ordering::Relaxed);
    let before_misses = counts.miss.load(Ordering::Relaxed);
    let before_hits = counts.hit.load(Ordering::Relaxed);
    let before_store = snapshot(store.root());
    let super::Lookup::Hit(hit) = evaluator.lookup(&recipe).unwrap() else {
        panic!("the outcome must contain the usable authenticated hit");
    };
    assert_eq!(hit.value(), published.value());
    assert_eq!(hit.content().digest(), published.content().digest());
    assert_eq!(hit.receipt().action(), published.receipt().action());
    assert_eq!(refreshes.load(Ordering::Relaxed), before_refreshes);
    assert_eq!(counts.miss.load(Ordering::Relaxed), before_misses);
    assert_eq!(counts.hit.load(Ordering::Relaxed), before_hits + 1);
    assert_eq!(snapshot(store.root()), before_store);
    // Consuming this receipt at a real current boundary still replays the
    // mutable ancestor: lookup has not granted new immutability authority.
    evaluator
        .seal_read_set([hit.receipt().read_set().clone()])
        .unwrap();
    assert_eq!(refreshes.load(Ordering::Relaxed), before_refreshes + 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn lookup_outcome_preserves_absence_recovery_and_operational_refusal() {
    let root = temporary("lookup-disposition");
    let context = prepare_fixture(&root);
    let store = context.open_store().unwrap();
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let recipe = context
        .direct::<Artifact>(b"lookup-disposition", 4_096)
        .unwrap();
    assert!(matches!(
        evaluator.lookup(&recipe).unwrap(),
        super::Lookup::Miss(super::Miss::Absent)
    ));
    assert_eq!(counts.rejected.load(Ordering::Relaxed), 0);
    let ledger_path = store.root().join("ledgers/installation");
    let empty_ledger = fs::read(&ledger_path).unwrap();
    evaluator
        .direct(&recipe, || Ok(Artifact(Box::from(b"payload".as_slice()))))
        .unwrap();
    let committed_ledger = fs::read(&ledger_path).unwrap();
    fs::write(&ledger_path, empty_ledger).unwrap();
    let before = snapshot(store.root());
    assert!(matches!(
        evaluator.lookup(&recipe).unwrap(),
        super::Lookup::Miss(super::Miss::RecoveryRequired)
    ));
    assert_eq!(counts.rejected.load(Ordering::Relaxed), 1);
    assert_eq!(snapshot(store.root()), before, "lookup cannot run recovery");
    fs::write(&ledger_path, committed_ledger).unwrap();

    // An operational read failure cannot become permission to execute a miss.
    let envelope = find_file(&store.root().join("actions"), "envelope");
    fs::remove_file(&envelope).unwrap();
    assert!(matches!(evaluator.lookup(&recipe), Err(Error::Io { .. })));
    assert_eq!(counts.rejected.load(Ordering::Relaxed), 2);
    assert_eq!(counts.miss.load(Ordering::Relaxed), 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn discoverable_and_final_union_validation_share_exact_work_caps_and_reuse_unchanged_payloads() {
    let root = temporary("discovered-union-budget");
    let context = prepare_fixture(&root);
    let store = context.open_store().unwrap();
    let recipe = context
        .discover::<Artifact>(b"discovered-union-budget", 4_096)
        .unwrap();
    let validate: super::read_set::Validator<Artifact> = Arc::new(|value, _| value.encode(4_096));
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let value = evaluator
        .discover(
            &recipe,
            |value| value.encode(4_096),
            || Ok(Artifact(Box::from(b"discovered".as_slice()))),
        )
        .unwrap();
    let child = context
        .direct_with_parents::<Derived>(b"union-child", 4_096, &[Parent::validated(&value)])
        .unwrap();
    let child = evaluator
        .direct(&child, || Ok(Derived(Box::from(b"child".as_slice()))))
        .unwrap();
    let proof = evaluator
        .seal_read_set([child.receipt().read_set().clone()])
        .unwrap();
    assert_eq!(proof.len(), 2);
    let parent = *proof
        .entries()
        .iter()
        .find(|entry| entry.action == value.receipt().action())
        .unwrap();
    let before = snapshot(store.root());
    let current_union = |context: &crate::context::Context| {
        let current_store = context.open_store().unwrap();
        let counts = Counts::default();
        let evaluator = Evaluator::new(&current_store, &counts);
        let parent = evaluator
            .replay_discovered_exact(&recipe, &validate, value.receipt().action())
            .unwrap()
            .unwrap();
        let child_recipe = context
            .direct_with_parents::<Derived>(b"union-child", 4_096, &[Parent::validated(&parent)])
            .unwrap();
        let child = evaluator
            .direct(&child_recipe, || panic!("the stored child must replay"))
            .unwrap();
        evaluator
            .seal_read_set([child.receipt().read_set().clone()])
            .unwrap()
    };

    // A memoized digest skips its file work, so every measured run and
    // its calibration start from a fresh payload identity.
    let calibration = fixture_context(&root).open_store().unwrap();
    touch_payload(&calibration, parent);
    {
        assert!(calibration
            .inspect_candidate::<Artifact>(
                value.receipt().action(),
                4_096,
                calibration.graph_budget()
            )
            .unwrap()
            .is_some());
    }
    let calibration_context = fixture_context(&root);
    let calibration = calibration_context.open_store().unwrap();
    let calibration_proof = current_union(&calibration_context);
    for entry in calibration_proof.entries() {
        touch_payload(&calibration, *entry);
    }
    calibration_proof.revalidate(&calibration).unwrap();
    // Unchanged payload identities reuse both digests on the next union.
    calibration_proof.revalidate(&calibration).unwrap();
    let calibration = fixture_context(&root).open_store().unwrap();
    touch_payload(&calibration, parent);
    {
        assert!(Evaluator::new(&calibration, &Counts::default())
            .lookup_discoverable(&recipe, &validate)
            .unwrap()
            .hit
            .is_some());
    }

    let fresh = fixture_context(&root).open_store().unwrap();
    touch_payload(&fresh, parent);
    let hit = Evaluator::new(&fresh, &Counts::default())
        .lookup_discoverable(&recipe, &validate)
        .unwrap();
    assert!(hit.hit.is_some());
    assert_eq!(snapshot(store.root()), before);

    // Exact replay revalidates again; a successful prior gate is not fresh authority.
    let current = fixture_context(&root);
    let fresh = current.open_store().unwrap();
    let proof = current_union(&current);
    for _ in 0..2 {
        for entry in proof.entries() {
            touch_payload(&fresh, *entry);
        }
        proof.revalidate(&fresh).unwrap();
    }
    assert_eq!(snapshot(store.root()), before);
    store.retire_fixture().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn discoverable_and_final_validation_precharge_metadata_before_corrupt_payload_hash() {
    for final_union in [false, true] {
        let root = temporary("discovered-before-decode");
        let context = prepare_fixture(&root);
        let store = context.open_store().unwrap();
        let recipe = context
            .discover::<Artifact>(b"before-decode", 4_096)
            .unwrap();
        let counts = Counts::default();
        let evaluator = Evaluator::new(&store, &counts);
        let value = evaluator
            .discover(
                &recipe,
                |value| value.encode(4_096),
                || Ok(Artifact(Box::from(b"bounded".as_slice()))),
            )
            .unwrap();
        let fresh = fixture_context(&root).open_store().unwrap();
        let proof = if final_union {
            let validate: super::read_set::Validator<Artifact> =
                Arc::new(|value, _| value.encode(4_096));
            let counts = Counts::default();
            let evaluator = Evaluator::new(&fresh, &counts);
            let value = evaluator
                .replay_discovered_exact(&recipe, &validate, value.receipt().action())
                .unwrap()
                .unwrap();
            Some(
                evaluator
                    .seal_read_set([value.receipt().read_set().clone()])
                    .unwrap(),
            )
        } else {
            None
        };
        fs::write(
            find_file(&store.root().join("content"), "payload"),
            b"changed",
        )
        .unwrap();
        let result = if final_union {
            proof.as_ref().unwrap().revalidate(&fresh)
        } else {
            fresh
                .inspect_candidate::<Artifact>(
                    value.receipt().action(),
                    4_096,
                    fresh.graph_budget(),
                )
                .map(|_| ())
        };
        // The validation charges the metadata and the payload in full. The
        // corrupt payload is rejected on its own merits, never as a limit.
        assert!(
            !matches!(result, Err(Error::Limit { .. })),
            "final_union={final_union}: graph work never refuses"
        );
        assert!(
            result.is_err(),
            "final_union={final_union}: the payload is corrupt"
        );
        store.retire_fixture().unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn typed_payload_above_metadata_cap_replays_with_its_transform_bound_per_new_identity() {
    let root = temporary("large-typed-payload");
    let context = prepare_fixture(&root);
    let store = context.open_store().unwrap();
    // Above the 32 MiB metadata figure.
    let length = 32 * 1024 * 1024 + 1;
    let recipe = context
        .direct::<Artifact>(b"large-typed-payload", length)
        .unwrap();
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let value = evaluator
        .direct(&recipe, || {
            Ok(Artifact(vec![0x73; length].into_boxed_slice()))
        })
        .unwrap();
    let proof = evaluator
        .seal_read_set([value.receipt().read_set().clone()])
        .unwrap();
    let entry = proof.entries()[0];
    // Each measured run and its calibration hash a fresh identity.
    let calibration = fixture_context(&root).open_store().unwrap();
    touch_payload(&calibration, entry);
    {
        let hit = Evaluator::new(&calibration, &Counts::default())
            .direct(&recipe, || panic!("calibration must replay"))
            .unwrap();
        assert_eq!(hit.value().0.len(), length);
    }
    let calibration = fixture_context(&root).open_store().unwrap();
    let current = calibration
        .lookup(&recipe, calibration.graph_budget())
        .unwrap()
        .unwrap();
    let calibration_proof = Evaluator::new(&calibration, &Counts::default())
        .seal_read_set([current.receipt().read_set().clone()])
        .unwrap();
    touch_payload(&calibration, entry);
    {
        assert!(calibration
            .inspect_candidate::<Artifact>(recipe.action(), length, calibration.graph_budget())
            .unwrap()
            .is_some());
        calibration_proof.revalidate(&calibration).unwrap();
    }
    let fresh = fixture_context(&root).open_store().unwrap();
    touch_payload(&fresh, entry);
    let hit = Evaluator::new(&fresh, &Counts::default())
        .direct(&recipe, || {
            panic!("a lawful large typed payload must replay")
        })
        .unwrap();
    assert_eq!(hit.value().0.len(), length);

    let fresh = fixture_context(&root).open_store().unwrap();
    let current = fresh
        .lookup(&recipe, fresh.graph_budget())
        .unwrap()
        .unwrap();
    let proof = Evaluator::new(&fresh, &Counts::default())
        .seal_read_set([current.receipt().read_set().clone()])
        .unwrap();
    touch_payload(&fresh, entry);
    let hit = fresh
        .inspect_candidate::<Artifact>(recipe.action(), length, fresh.graph_budget())
        .unwrap()
        .unwrap();
    assert_eq!(hit.value().0.len(), length);
    proof.revalidate(&fresh).unwrap();

    let payload = find_file(&store.root().join("content"), "payload");
    fs::OpenOptions::new()
        .write(true)
        .open(payload)
        .unwrap()
        .set_len(length as u64 + 1)
        .unwrap();
    let fresh = fixture_context(&root).open_store().unwrap();
    assert!(matches!(fresh.lookup(&recipe, fresh.graph_budget()),
        Err(Error::Limit { resource: "typed store file bytes", limit, actual })
            if limit == length && actual == length + 1));
    store.retire_fixture().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[cfg(all(feature = "test-fixtures", unix))]
#[test]
fn registered_bundle_commitment_retains_records_without_granting_current_admission() {
    use crate::store::bundle::{self, BundleCommitment};
    use std::os::unix::fs::PermissionsExt as _;

    let root = temporary("registered-retention-commitment");
    let context = prepare_fixture(&root);
    let store = context.open_store().unwrap();
    let recipe = context
        .discover::<BundleValue>(b"registered-retention", 4_096)
        .unwrap();
    let validation_store = store.clone();
    let value = Evaluator::new(&store, &Counts::default())
        .discover_bundled(
            &recipe,
            move |value| value.validate_published(&validation_store),
            bundled_fixture,
        )
        .unwrap();
    let proof = Evaluator::new(&store, &Counts::default())
        .seal_read_set([value.receipt().read_set().clone()])
        .unwrap();
    let entry = proof.entries()[0];
    let product = value.value();
    let commitment =
        BundleCommitment::new(product.reference, product.seal_length, product.seal_digest);
    // The entry authenticates action provenance; the commitment alone is only
    // a reference/seal description and deliberately cannot grant action authority.
    let authenticate = |entry: super::read_set::Entry, commitment: &BundleCommitment| {
        store.revalidate_read_set_entry(entry, store.graph_budget())?;
        bundle::validate_commitment(
            store.root(),
            commitment,
            entry.kind,
            entry.schema,
            entry.content,
            store.graph_budget(),
        )
    };
    authenticate(entry, &commitment).unwrap();
    product.validate_published(&store).unwrap();

    let wrong_id = BundleCommitment::new(
        bundle::BundleRef::new(
            product.reference.execution(),
            bundle::BundleId::from_bytes([0xED; 32]),
        ),
        product.seal_length,
        product.seal_digest,
    );
    assert!(authenticate(entry, &wrong_id).is_err());
    let wrong_seal = BundleCommitment::new(product.reference, product.seal_length, [0; 32]);
    assert!(authenticate(entry, &wrong_seal).is_err());
    let mut foreign = entry;
    foreign.action = super::ActionKey::from_bytes([0xEF; 32]);
    assert!(authenticate(foreign, &commitment).is_err());
    let mut foreign_content = entry;
    foreign_content.content = super::Digest::from_bytes([0xEE; 32]);
    assert!(bundle::validate_commitment(
        store.root(),
        &commitment,
        entry.kind,
        entry.schema,
        foreign_content.content,
        store.graph_budget()
    )
    .is_err());

    let member = store
        .root()
        .join(product.reference.relative())
        .join("generated/input");
    let original = fs::metadata(&member).unwrap().permissions();
    fs::set_permissions(&member, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&member, vec![0x73; 8192]).unwrap();
    fs::set_permissions(&member, original).unwrap();
    authenticate(entry, &commitment)
        .expect("registered historical controls survive nested member drift");
    assert!(
        matches!(
            product.validate_published(&store),
            Err(Error::CandidateCorrupt(_))
        ),
        "a full consumer must reject changed physical charge before admission"
    );

    let graph = store.graph_budget();
    bundle::validate_commitment(
        store.root(),
        &commitment,
        entry.kind,
        entry.schema,
        entry.content,
        graph,
    )
    .expect("registered controls validate");
    store.retire_fixture().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[cfg(feature = "test-fixtures")]
#[test]
fn bundled_discoverable_and_final_replay_charge_controls_and_every_repeated_walk() {
    let root = temporary("bundled-validation-budget");
    let context = prepare_fixture(&root);
    let store = context.open_store().unwrap();
    let recipe = context
        .discover::<BundleValue>(b"bundled-validation-budget", 4_096)
        .unwrap();
    let bundle_store = store.root().to_path_buf();
    let validate: super::read_set::Validator<BundleValue> = Arc::new(move |value, budget| {
        let canonical = value.encode(4_096)?;
        crate::store::bundle::validate_complete(
            &bundle_store,
            value.reference,
            BundleValue::KIND,
            BundleValue::SCHEMA,
            super::content_digest::<BundleValue>(&canonical),
            value.seal_length,
            value.seal_digest,
            budget,
        )?;
        Ok(canonical)
    });
    let callback = Arc::clone(&validate);
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let value = evaluator
        .discover_bundled_budgeted(
            &recipe,
            move |value, budget| callback(value, budget),
            bundled_fixture,
        )
        .unwrap();
    let proof = evaluator
        .seal_read_set([value.receipt().read_set().clone()])
        .unwrap();
    let bundle = store.root().join(value.value().reference.relative());
    let length = |path| fs::metadata(path).unwrap().len() as usize;
    let allocation = length(bundle.join("allocation"));
    let before = snapshot(store.root());
    let calibration = fixture_context(&root).open_store().unwrap();
    {
        Evaluator::new(&calibration, &Counts::default())
            .replay_discovered_exact(&recipe, &validate, value.receipt().action())
            .unwrap()
            .unwrap();
    }
    let calibration = fixture_context(&root).open_store().unwrap();
    let counts = Counts::default();
    let evaluator = Evaluator::new(&calibration, &counts);
    let current = evaluator
        .replay_discovered_exact(&recipe, &validate, value.receipt().action())
        .unwrap()
        .unwrap();
    let current_proof = evaluator
        .seal_read_set([current.receipt().read_set().clone()])
        .unwrap();
    current_proof.revalidate(&calibration).unwrap();
    let calibration = fixture_context(&root).open_store().unwrap();
    {
        assert!(Evaluator::new(&calibration, &Counts::default())
            .lookup_discoverable(&recipe, &validate)
            .unwrap()
            .hit
            .is_some());
    }

    let fresh = fixture_context(&root).open_store().unwrap();
    let counts = Counts::default();
    let evaluator = Evaluator::new(&fresh, &counts);
    let current = evaluator
        .replay_discovered_exact(&recipe, &validate, value.receipt().action())
        .unwrap()
        .unwrap();
    let replayed = evaluator
        .seal_read_set([current.receipt().read_set().clone()])
        .unwrap();
    replayed
        .revalidate(&fresh)
        .expect("the first replay revalidates");
    replayed
        .revalidate(&fresh)
        .expect("the second replay revalidates");
    assert_eq!(snapshot(store.root()), before);
    let fresh = fixture_context(&root).open_store().unwrap();
    let hit = Evaluator::new(&fresh, &Counts::default())
        .lookup_discoverable(&recipe, &validate)
        .unwrap();
    assert!(hit.hit.is_some());
    assert_eq!(snapshot(store.root()), before);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;

        let allocation_path = bundle.join("allocation");
        let frozen = fs::metadata(&allocation_path).unwrap().permissions();
        fs::set_permissions(&allocation_path, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&allocation_path, vec![0; allocation]).unwrap();
        fs::set_permissions(&allocation_path, frozen).unwrap();
        let fresh = fixture_context(&root).open_store().unwrap();
        // The malformed allocation record is charged and read; it is
        // rejected as corrupt, never as a limit.
        assert!(
            matches!(
                validate(value.value(), fresh.graph_budget()),
                Err(Error::Corrupt {
                    reason: "runner Cargo bundle allocation decoding failed",
                    ..
                })
            ),
            "bounded validation rejects the malformed allocation record on its merits"
        );
        assert!(matches!(
            proof.revalidate(&store),
            Err(Error::Corrupt {
                reason: "runner Cargo bundle allocation decoding failed",
                ..
            })
        ));
    }
    store.retire_fixture().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn existing_pin_roles_preserve_unrelated_orphans_above_recovery_cap() {
    for count in [1, 65] {
        let root = temporary("healthy-pins-ignore-orphans");
        let context = prepare_fixture(&root);
        let store = context.open_store().unwrap();
        let recipe = context
            .direct::<Artifact>(b"healthy-pins-ignore-orphans", 4_096)
            .unwrap();
        let value = Evaluator::new(&store, &Counts::default())
            .direct(&recipe, || Ok(Artifact(Box::from(b"healthy".as_slice()))))
            .unwrap();
        let retained = super::Chain::validated(context, &value)
            .prepare_read_set()
            .unwrap();
        retained
            .pin_prepared()
            .expect("a healthy new Prepared root binds");
        retained
            .pin_live_last_good()
            .expect("healthy new live roles bind");
        let temporaries = store.root().join("temporaries");
        for index in 0..count {
            let orphan = temporaries.join(format!("999999-{index}"));
            fs::create_dir(&orphan).unwrap();
            fs::write(orphan.join("payload"), b"unrelated interrupted publication").unwrap();
        }
        let before = snapshot(store.root());
        let fresh = fixture_context(&root);
        let fresh_store = fresh.open_store().unwrap();
        let hit = Evaluator::new(&fresh_store, &Counts::default())
            .direct(&recipe, || {
                panic!("the healthy cached node must remain a hit")
            })
            .unwrap();
        assert_eq!(hit.content().digest(), value.content().digest());
        let prepared = super::Chain::validated(fresh, &hit)
            .prepare_read_set()
            .unwrap();
        prepared
            .pin_prepared()
            .expect("Prepared admission ignores unrelated orphans");
        assert_eq!(snapshot(store.root()), before);
        prepared
            .pin_live_last_good()
            .expect("hot promotion ignores unrelated orphans");
        assert_eq!(snapshot(store.root()), before);
        prepared
            .with_live_last_good(|| Err::<(), _>("rejected admission"))
            .expect("rollback remains available without sweeping")
            .unwrap_err();
        assert_eq!(snapshot(store.root()), before);
        fresh_store
            .with_pin(crate::store::PinRole::InFlight, prepared.proof(), || Ok(()))
            .expect("an interrupted run's temporaries never block a new pin");
        drop(prepared);
        drop(retained);
        store.retire_fixture().unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(feature = "test-fixtures")]
#[test]
fn existing_pin_roles_leave_a_journaled_retirement_for_miss_side_recovery() {
    let root = temporary("pins-preserve-journaled-temporary");
    let context = prepare_fixture(&root);
    let store = context.open_store().unwrap();
    let runner_recipe = context
        .discover::<BundleValue>(b"retired-runner", 4_096)
        .unwrap();
    let current = store.clone();
    Evaluator::new(&store, &Counts::default())
        .discover_bundled(
            &runner_recipe,
            move |value| value.validate_published(&current),
            bundled_fixture,
        )
        .unwrap();
    let recipe = context.direct::<Artifact>(b"retained-node", 4_096).unwrap();
    let value = Evaluator::new(&store, &Counts::default())
        .direct(&recipe, || Ok(Artifact(Box::from(b"retained".as_slice()))))
        .unwrap();
    let prepared = super::Chain::validated(context, &value)
        .prepare_read_set()
        .unwrap();
    prepared.pin_prepared().unwrap();
    prepared.pin_live_last_good().unwrap();
    let charge = store.fixture_usage().unwrap();
    let staged = store.fixture_interrupt_after_staging_action().unwrap();
    assert!(staged.join("envelope").is_file());
    assert!(store.fixture_has_pending_eviction().unwrap());
    assert_eq!(
        store.fixture_usage().unwrap(),
        charge,
        "the interrupted whole-unit retirement retains its ledger charge"
    );
    let before = snapshot(store.root());
    let fresh = fixture_context(&root);
    let fresh_store = fresh.open_store().unwrap();
    let hit = Evaluator::new(&fresh_store, &Counts::default())
        .direct(&recipe, || {
            panic!("the retained graph remains a read-only hit")
        })
        .unwrap();
    let rebound = super::Chain::validated(fresh, &hit)
        .prepare_read_set()
        .unwrap();
    rebound.pin_prepared().unwrap();
    rebound.pin_live_last_good().unwrap();
    assert_eq!(snapshot(store.root()), before);
    fresh_store
        .with_pin(crate::store::PinRole::InFlight, rebound.proof(), || Ok(()))
        .expect("a journaled staged temporary never blocks a new pin");
    assert!(
        staged.exists(),
        "only miss-side recovery retires the journaled unit"
    );

    let next = fixture_context(&root)
        .direct::<Artifact>(b"miss-after-retirement", 4_096)
        .unwrap();
    Evaluator::new(&fresh_store, &Counts::default())
        .direct(&next, || {
            assert!(
                !staged.exists(),
                "bounded recovery precedes the new transform"
            );
            assert!(!fresh_store.fixture_has_pending_eviction().unwrap());
            Ok(Artifact(Box::from(b"new".as_slice())))
        })
        .unwrap();
    drop(rebound);
    drop(prepared);
    store.retire_fixture().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn miss_reservation_recovers_every_unrelated_temporary() {
    for count in [64, 65] {
        let root = temporary("miss-recovers-orphans");
        let context = prepare_fixture(&root);
        let store = context.open_store().unwrap();
        let temporaries = store.root().join("temporaries");
        for index in 0..count {
            let orphan = temporaries.join(format!("999999-{index}"));
            fs::create_dir(&orphan).unwrap();
            fs::write(orphan.join("payload"), b"dead publication").unwrap();
        }
        let recipe = context
            .direct::<Artifact>(b"miss-recovers-orphans", 4_096)
            .unwrap();
        let executions = AtomicUsize::new(0);
        let result = Evaluator::new(&store, &Counts::default()).direct(&recipe, || {
            executions.fetch_add(1, Ordering::Relaxed);
            assert_eq!(fs::read_dir(&temporaries).unwrap().count(), 0);
            Ok(Artifact(Box::from(b"published".as_slice())))
        });
        result.expect("miss-side recovery removes every dead temporary");
        assert_eq!(executions.load(Ordering::Relaxed), 1);
        assert_eq!(fs::read_dir(&temporaries).unwrap().count(), 0);
        store.retire_fixture().unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn direct_post_publication_recheck_spends_the_same_graph_budget() {
    let root = temporary("direct-published-budget");
    let context = prepare_fixture(&root);
    let store = context.open_store().unwrap();
    let recipe = context
        .direct::<Artifact>(b"direct-published-budget", 4_096)
        .unwrap();
    let counts = Counts::default();
    let result = Evaluator::new(&store, &counts).direct(&recipe, || {
        Ok(Artifact(b"bounded".to_vec().into_boxed_slice()))
    });
    // The post-publication recheck spends the same account.
    result.expect("the post-publication recheck completes");
    assert_eq!(counts.miss.load(Ordering::Relaxed), 1);
    assert_eq!(counts.published.load(Ordering::Relaxed), 1);
    store.retire_fixture().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn direct_after_lease_recheck_spends_the_same_graph_budget_without_fallback() {
    struct PausedRejection {
        counts: Counts,
        reached: std::sync::mpsc::SyncSender<()>,
        resume: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
    }

    impl Sink for PausedRejection {
        fn node(&self, kind: NodeKind, event: NodeEvent) {
            self.counts.node(kind, event);
            if event == NodeEvent::RejectedHit && self.counts.rejected.load(Ordering::Relaxed) == 1
            {
                self.reached.send(()).unwrap();
                self.resume
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
            }
        }
        fn process(&self, _: ProcessKind) {}
        fn milestone(&self, _: Milestone, _: Option<u64>) {}
        fn runner_owned(&self, _: &metrics::runner::Owned) {}
        fn scope_opened(&self) {}
        fn scope_joined(&self) {}
    }

    let root = temporary("direct-lease-budget");
    let context = prepare_fixture(&root);
    let store = context.open_store().unwrap();
    let recipe = context
        .direct::<Artifact>(b"direct-lease-budget", 4_096)
        .unwrap();
    store
        .fixture_stage_uncommitted(&recipe, b"bounded")
        .unwrap();
    let contender = fixture_context(&root).open_store().unwrap();
    let (reached, ready) = std::sync::mpsc::sync_channel(1);
    let (resume, receiver) = std::sync::mpsc::sync_channel(1);
    let paused = PausedRejection {
        counts: Counts::default(),
        reached,
        resume: std::sync::Mutex::new(receiver),
    };
    let result = std::thread::scope(|scope| {
        let worker = store
            .with_ownership(|ownership| {
                store.with_action_lease(recipe.action(), || {
                    let worker = scope.spawn(|| {
                        Evaluator::new(&contender, &paused).direct(&recipe, || {
                            panic!("a contender must reuse its winner or reject its budget")
                        })
                    });
                    ready
                        .recv_timeout(Duration::from_secs(5))
                        .expect("the first lookup rejects the uncommitted record");
                    store.with_action_reservation(ownership, &recipe, None, || {
                        Ok(crate::store::Reserved::Publish {
                            result: (),
                            recipe: recipe.clone(),
                            bytes: b"bounded".to_vec().into_boxed_slice(),
                            bundle: None,
                        })
                    })?;
                    resume.send(()).unwrap();
                    Ok(worker)
                })
            })
            .unwrap();
        worker.join().expect("the contender joins")
    });
    // The winner recheck spends the same account and reuses the winner.
    result.expect("the winner recheck reuses the winner");
    assert_eq!(paused.counts.hit.load(Ordering::Relaxed), 1);
    assert_eq!(paused.counts.rejected.load(Ordering::Relaxed), 1);
    assert_eq!(paused.counts.miss.load(Ordering::Relaxed), 0);
    store.retire_fixture().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn legacy_project_local_entry_is_ignored_and_never_upgraded() {
    let root = temporary("legacy-project-local");
    let context = prepare_fixture(&root);
    let legacy = root.join("project/target/rawr/cache/runners/legacy-owner");
    fs::create_dir_all(&legacy).expect("the project-local legacy directory exists");
    fs::write(legacy.join("receipt"), b"legacy-project-local")
        .expect("the project-local legacy receipt exists");
    let project_before = project_snapshot(&root.join("project"));
    let store = context.open_store().expect("the shared store opens");
    let recipe = context
        .direct::<Artifact>(b"legacy-project-local", 4_096)
        .expect("the shared direct recipe seals");
    let executions = AtomicUsize::new(0);

    Evaluator::new(&store, &Counts::default())
        .direct(&recipe, || {
            executions.fetch_add(1, Ordering::Relaxed);
            Ok(Artifact(b"shared-current".to_vec().into_boxed_slice()))
        })
        .expect("the project-local legacy entry is one shared-store miss");
    assert_eq!(executions.load(Ordering::Relaxed), 1);
    assert_eq!(
        project_snapshot(&root.join("project")),
        project_before,
        "shared-store evaluation must not read, relocate, or upgrade project-local legacy bytes"
    );

    store
        .retire_fixture()
        .expect("the project-local legacy fixture store retires");
    fs::remove_dir_all(root).expect("the project-local legacy fixture retires");
}

#[test]
fn deleting_the_shared_store_preserves_project_sources_and_causes_one_cold_miss() {
    let root = temporary("source-safe-store-deletion");
    let context = prepare_fixture(&root);
    fs::write(root.join("project/main.rs"), b"window source").expect("the project source exists");
    let project_before = project_snapshot(&root.join("project"));
    let recipe = context
        .direct::<Artifact>(b"source-safe-store-deletion", 4_096)
        .expect("the deletion recipe seals");
    let first_executions = AtomicUsize::new(0);
    let first_store = context.open_store().expect("the first shared store opens");
    Evaluator::new(&first_store, &Counts::default())
        .direct(&recipe, || {
            first_executions.fetch_add(1, Ordering::Relaxed);
            Ok(Artifact(b"first-store".to_vec().into_boxed_slice()))
        })
        .expect("the first shared store publishes");
    assert_eq!(first_executions.load(Ordering::Relaxed), 1);
    first_store
        .retire_fixture()
        .expect("the complete shared store deletes");
    assert_eq!(
        project_snapshot(&root.join("project")),
        project_before,
        "deleting the shared store must leave project source byte-identical"
    );

    let second_executions = AtomicUsize::new(0);
    let second_store = context
        .open_store()
        .expect("the deleted shared store recreates empty");
    Evaluator::new(&second_store, &Counts::default())
        .direct(&recipe, || {
            second_executions.fetch_add(1, Ordering::Relaxed);
            Ok(Artifact(b"second-store".to_vec().into_boxed_slice()))
        })
        .expect("the recreated store takes one clean cold miss");
    assert_eq!(second_executions.load(Ordering::Relaxed), 1);
    assert_eq!(project_snapshot(&root.join("project")), project_before);

    second_store
        .retire_fixture()
        .expect("the recreated shared store retires");
    fs::remove_dir_all(root).expect("the source-safe deletion fixture retires");
}

#[test]
fn node_publication_reserves_before_execution_and_commits_exactly_once() {
    let root = temporary("accounted-publication");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the typed store opens");
    assert_eq!(
        store.fixture_usage().expect("the empty ledger reads"),
        (0, 0, 0, 0)
    );
    assert_eq!(
        store
            .fixture_project_usage()
            .expect("the empty project ledger reads")
            .3,
        0
    );
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let recipe = context
        .direct::<Artifact>(b"accounted-publication", 4_096)
        .expect("the accounted recipe seals");
    evaluator
        .direct(&recipe, || {
            let (bytes, allocations, members, reservations) =
                store.fixture_usage().expect("the active reservation reads");
            assert_eq!((bytes, allocations, members), (0, 0, 0));
            assert_eq!(reservations, 1);
            assert_eq!(
                store
                    .fixture_project_usage()
                    .expect("the active project reservation reads")
                    .3,
                1
            );
            Ok(Artifact(b"accounted".to_vec().into_boxed_slice()))
        })
        .expect("the accounted node publishes");
    let committed = store.fixture_usage().expect("the committed ledger reads");
    assert!(committed.0 >= 16 * 1024);
    assert_eq!((committed.1, committed.2, committed.3), (2, 6, 0));
    assert_eq!(
        store
            .fixture_project_usage()
            .expect("the completed project reservation reads")
            .3,
        0
    );
    let sealed = snapshot(store.root());
    evaluator
        .direct(&recipe, || {
            Err(Error::State(
                "a healthy accounted hit must not execute its transform",
            ))
        })
        .expect("the accounted node hits");
    assert_eq!(
        store.fixture_usage().expect("the hit ledger reads"),
        committed
    );
    assert_eq!(snapshot(store.root()), sealed);
    fs::remove_dir_all(root).expect("the accounted-publication fixture retires");
}

#[test]
fn one_dead_publication_temporary_recovers_before_transform_reservation() {
    let root = temporary("dead-publication-temporary");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the typed store opens");
    let dead = store.root().join("temporaries/123-456");
    fs::create_dir(&dead).expect("the dead publication temporary exists");
    fs::write(dead.join("partial"), b"incomplete").expect("the dead publication payload exists");
    let recipe = context
        .direct::<Artifact>(b"dead-publication-temporary", 4_096)
        .expect("the direct recipe seals");
    let executions = AtomicUsize::new(0);

    Evaluator::new(&store, &Counts::default())
        .direct(&recipe, || {
            executions.fetch_add(1, Ordering::Relaxed);
            assert!(
                !dead.exists(),
                "publication recovery must finish before transform execution"
            );
            Ok(Artifact(b"recovered".to_vec().into_boxed_slice()))
        })
        .expect("the dead publication temporary recovers");
    assert_eq!(executions.load(Ordering::Relaxed), 1);
    assert!(!dead.exists());

    store
        .retire_fixture()
        .expect("the dead-publication fixture store retires");
    fs::remove_dir_all(root).expect("the dead-publication fixture retires");
}

#[test]
fn transform_failure_cancels_its_preexecution_reservation() {
    let root = temporary("cancelled-reservation");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the typed store opens");
    let counts = Counts::default();
    let recipe = context
        .direct::<Artifact>(b"cancelled-reservation", 4_096)
        .expect("the failing recipe seals");
    let result = Evaluator::new(&store, &counts).direct(&recipe, || {
        assert_eq!(
            store
                .fixture_usage()
                .expect("the active reservation reads")
                .3,
            1
        );
        Err(Error::Transform("fixture transform failure".into()))
    });
    assert!(matches!(result, Err(Error::Transform(_))));
    assert_eq!(
        store.fixture_usage().expect("the cancelled ledger reads"),
        (0, 0, 0, 0)
    );
    assert_eq!(
        store
            .fixture_project_usage()
            .expect("the cancelled project ledger reads")
            .3,
        0
    );
    assert!(
        !store.fixture_project_ledger_exists(),
        "an empty project ledger must not consume the installation-wide cap"
    );
    fs::remove_dir_all(root).expect("the cancelled-reservation fixture retires");
}

#[cfg(unix)]
#[test]
fn publication_failure_preserves_its_reservation_for_recovery() {
    use std::os::unix::fs::symlink;

    let root = temporary("preserved-reservation");
    let outside = temporary("preserved-reservation-outside");
    let context = prepare_fixture(&root);
    fs::create_dir(&outside).expect("the outside fixture exists");
    let store = context.open_store().expect("the typed store opens");
    let counts = Counts::default();
    let recipe = context
        .direct::<Artifact>(b"preserved-reservation", 4_096)
        .expect("the publication-failure recipe seals");
    let result = Evaluator::new(&store, &counts).direct(&recipe, || {
        assert_eq!(
            store
                .fixture_usage()
                .expect("the active reservation reads")
                .3,
            1
        );
        let digest = super::content_digest::<Artifact>(b"unpublished");
        symlink(
            &outside,
            store
                .root()
                .join("content/native-product/00000001")
                .join(digest.directory()),
        )
        .expect("the fixture blocks content publication");
        Ok(Artifact(b"unpublished".to_vec().into_boxed_slice()))
    });
    assert!(matches!(result, Err(Error::Shape(_))));
    assert_eq!(
        store
            .fixture_usage()
            .expect("the preserved reservation reads")
            .3,
        1
    );
    assert_eq!(
        store
            .fixture_project_usage()
            .expect("the preserved project reservation reads")
            .3,
        1
    );
    let retry_executions = AtomicUsize::new(0);
    let retried = Evaluator::new(&store, &counts).direct(&recipe, || {
        retry_executions.fetch_add(1, Ordering::Relaxed);
        Ok(Artifact(b"unpublished".to_vec().into_boxed_slice()))
    });
    assert!(
        matches!(retried, Err(Error::Shape(_))),
        "{:?}",
        retried.err()
    );
    // The retry's plan checks its own allocation's placement, not the whole
    // store: it may execute once more, then refuses before publishing.
    assert!(
        retry_executions.load(Ordering::Relaxed) <= 1,
        "the retry refuses the unchanged noncanonical target"
    );
    assert_eq!(
        store
            .fixture_usage()
            .expect("the blocked reservation reads")
            .3,
        1
    );
    fs::remove_dir_all(root).expect("the preserved-reservation fixture retires");
    fs::remove_dir_all(outside).expect("the outside fixture retires");
}

#[test]
fn retry_recovers_the_current_keys_whole_unregistered_publication() {
    let root = temporary("recovered-publication");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the typed store opens");
    let recipe = context
        .direct::<Artifact>(b"recovered-publication", 4_096)
        .expect("the recovered recipe seals");
    store
        .fixture_stage_uncommitted(&recipe, b"recovered")
        .expect("the crash fixture leaves one exact planned publication");
    assert_eq!(
        store.fixture_usage().expect("the orphan ledger reads"),
        (0, 0, 0, 1)
    );
    assert_eq!(
        store
            .fixture_project_usage()
            .expect("the orphan project reservation reads")
            .3,
        1
    );

    let counts = Counts::default();
    let executions = AtomicUsize::new(0);
    let validated = Evaluator::new(&store, &counts)
        .direct(&recipe, || {
            executions.fetch_add(1, Ordering::Relaxed);
            Ok(Artifact(b"recovered".to_vec().into_boxed_slice()))
        })
        .expect("the exact-key retry recovers and republishes");
    assert_eq!(validated.value().0.as_ref(), b"recovered");
    assert_eq!(executions.load(Ordering::Relaxed), 1);
    assert_eq!(counts.hit.load(Ordering::Relaxed), 0);
    assert_eq!(counts.miss.load(Ordering::Relaxed), 1);
    assert_eq!(counts.published.load(Ordering::Relaxed), 1);
    assert_eq!(counts.rejected.load(Ordering::Relaxed), 1);
    let (_, allocations, members, reservations) =
        store.fixture_usage().expect("the recovered ledger reads");
    assert_eq!((allocations, members, reservations), (2, 6, 0));
    assert_eq!(
        store
            .fixture_project_usage()
            .expect("the recovered project ledger reads")
            .3,
        0
    );
    fs::remove_dir_all(root).expect("the recovered-publication fixture retires");
}

#[test]
fn retry_recovers_a_partially_deleted_staged_planned_publication() {
    let root = temporary("recovered-staged-publication");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the staged store opens");
    let recipe = context
        .direct::<Artifact>(b"recovered-staged-publication", 4_096)
        .expect("the staged recipe seals");
    store
        .fixture_stage_uncommitted(&recipe, b"staged")
        .expect("the fixture leaves one planned publication");
    let temporary = store
        .fixture_interrupt_planned_action(&recipe)
        .expect("the planned action is staged atomically");
    let member = fs::read_dir(&temporary)
        .expect("the staged planned action reads")
        .next()
        .expect("the staged planned action has a member")
        .expect("the staged planned action member reads")
        .path();
    fs::remove_file(member).expect("the simulated crash interrupts staged deletion");

    let executions = AtomicUsize::new(0);
    Evaluator::new(&store, &Counts::default())
        .direct(&recipe, || {
            executions.fetch_add(1, Ordering::Relaxed);
            Ok(Artifact(b"staged".to_vec().into_boxed_slice()))
        })
        .expect("the partial staged publication recovers and republishes");
    assert_eq!(executions.load(Ordering::Relaxed), 1);
    assert!(!temporary.exists());
    assert_eq!(
        store.fixture_usage().expect("the recovered usage reads").3,
        0
    );
    store
        .retire_fixture()
        .expect("the staged-publication store retires");
    fs::remove_dir_all(root).expect("the staged-publication fixture retires");
}

#[test]
fn different_key_mutation_recovers_one_dead_foreign_installation_reservation() {
    let root = temporary("foreign-reservation");
    fs::create_dir(&root).expect("the foreign-reservation root exists");
    fs::create_dir(root.join("project-a")).expect("project A exists");
    fs::create_dir(root.join("project-b")).expect("project B exists");
    fs::create_dir(root.join("cargo-home")).expect("the shared Cargo home exists");
    let cargo_home =
        fs::canonicalize(root.join("cargo-home")).expect("the shared Cargo home canonicalizes");
    // Projects share a store only through one Cargo target directory.
    let target = cargo_home.with_file_name("target");
    let environment = cargo::config::Environment::from_pairs([
        (
            "CARGO_HOME",
            cargo_home
                .to_str()
                .expect("the shared Cargo home is Unicode"),
        ),
        (
            "CARGO_TARGET_DIR",
            target.to_str().expect("the shared target is Unicode"),
        ),
    ])
    .expect("the shared environment seals");
    let limits = cargo::Limits::standard();
    let first = Context::capture(
        &root.join("project-a"),
        environment.clone(),
        "test-target",
        &limits,
    )
    .expect("project A context seals");
    let second = Context::capture(&root.join("project-b"), environment, "test-target", &limits)
        .expect("project B context seals");
    let first_store = first.open_store().expect("project A store opens");
    let abandoned = first
        .direct::<Artifact>(b"abandoned-foreign", 4_096)
        .expect("the abandoned recipe seals");
    first_store
        .fixture_stage_uncommitted(&abandoned, b"abandoned")
        .expect("project A leaves one dead reservation");
    assert_eq!(
        first_store
            .fixture_usage()
            .expect("the reservation reads")
            .3,
        1
    );

    let second_store = second.open_store().expect("project B store opens");
    let current = second
        .direct::<Artifact>(b"current-foreign", 4_096)
        .expect("the current recipe seals");
    Evaluator::new(&second_store, &Counts::default())
        .direct(&current, || {
            Ok(Artifact(b"current".to_vec().into_boxed_slice()))
        })
        .expect("the different-key mutation recovers the dead foreign reservation");
    assert_eq!(
        second_store
            .fixture_usage()
            .expect("the clean ledger reads")
            .3,
        0
    );
    assert!(
        !first_store.fixture_project_ledger_exists(),
        "the abandoned foreign project ledger retires"
    );
    second_store
        .retire_fixture()
        .expect("the shared foreign-reservation store retires");
    fs::remove_dir_all(root).expect("the foreign-reservation fixture retires");
}

#[test]
fn same_key_fresh_process_contenders_execute_and_publish_exactly_once() {
    let root = temporary("direct-contenders");
    let context = prepare_fixture(&root);
    let store_root = context.rawr().join("store");
    let first_report = root.with_extension("contender-a.report");
    let second_report = root.with_extension("contender-b.report");
    let first = worker_command(&root, &first_report)
        .spawn()
        .expect("the first contender launches");
    let second = worker_command(&root, &second_report)
        .spawn()
        .expect("the second contender launches");
    assert_worker_success(
        first
            .wait_with_output()
            .expect("the first contender completes"),
    );
    assert_worker_success(
        second
            .wait_with_output()
            .expect("the second contender completes"),
    );
    let reports = [parse_report(&first_report), parse_report(&second_report)];
    let aggregate = |field: &str| {
        reports
            .iter()
            .map(|report| report.get(field).copied().unwrap_or_default())
            .sum::<usize>()
    };
    assert_eq!(aggregate("hits"), 1);
    assert_eq!(aggregate("misses"), 1);
    assert_eq!(aggregate("published"), 1);
    assert_eq!(aggregate("rejected"), 0);
    assert_eq!(aggregate("transforms"), 1);
    assert_eq!(aggregate("children"), 1);
    let sealed = snapshot(&store_root);
    assert_eq!(
        sealed
            .keys()
            .filter(|path| path.to_string_lossy().ends_with("/payload"))
            .count(),
        1
    );
    assert_eq!(
        sealed
            .keys()
            .filter(|path| path.to_string_lossy().ends_with("/envelope"))
            .count(),
        1
    );
    fs::remove_file(first_report).expect("the first contender report retires");
    fs::remove_file(second_report).expect("the second contender report retires");
    fs::remove_dir_all(root).expect("the contender store fixture retires");
}

#[test]
fn equal_parent_content_reuses_downstream_and_rebinds_the_current_read_set() {
    let root = temporary("semantic-parent-reuse");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the typed store opens");
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let upstream_executions = AtomicUsize::new(0);
    let downstream_executions = AtomicUsize::new(0);

    let upstream_a = context
        .direct::<Artifact>(b"upstream-request-a", 4_096)
        .expect("the first upstream recipe seals");
    let upstream_action_a = upstream_a.action();
    let validated_a = evaluator
        .direct(&upstream_a, || {
            upstream_executions.fetch_add(1, Ordering::Relaxed);
            Ok(Artifact(b"semantic-parent".to_vec().into_boxed_slice()))
        })
        .expect("the first upstream action publishes");
    let parent_a = Parent::validated(&validated_a);
    let downstream_a = context
        .direct_with_parents::<Derived>(b"downstream-request", 4_096, &[parent_a])
        .expect("the first downstream recipe seals");
    let downstream_action = downstream_a.action();
    let first = evaluator
        .direct(&downstream_a, || {
            downstream_executions.fetch_add(1, Ordering::Relaxed);
            Ok(Derived(b"derived-output".to_vec().into_boxed_slice()))
        })
        .expect("the downstream action publishes");
    assert_eq!(first.receipt().read_set().len(), 2);
    assert!(first.receipt().read_set().contains(upstream_action_a));
    assert!(first.receipt().read_set().contains(downstream_action));

    let upstream_b = context
        .direct::<Artifact>(b"upstream-request-b", 4_096)
        .expect("the changed upstream recipe seals");
    let upstream_action_b = upstream_b.action();
    assert_ne!(upstream_action_a, upstream_action_b);
    let validated_b = evaluator
        .direct(&upstream_b, || {
            upstream_executions.fetch_add(1, Ordering::Relaxed);
            Ok(Artifact(b"semantic-parent".to_vec().into_boxed_slice()))
        })
        .expect("the changed upstream action converges on the same content");
    assert_eq!(
        validated_a.content().digest(),
        validated_b.content().digest()
    );
    let parent_b = Parent::validated(&validated_b);
    let downstream_b = context
        .direct_with_parents::<Derived>(b"downstream-request", 4_096, &[parent_b])
        .expect("the rebound downstream recipe seals");
    assert_eq!(downstream_action, downstream_b.action());
    let second = evaluator
        .direct(&downstream_b, || {
            downstream_executions.fetch_add(1, Ordering::Relaxed);
            Ok(Derived(
                b"forbidden-reexecution".to_vec().into_boxed_slice(),
            ))
        })
        .expect("equal parent content reuses the downstream node");

    assert_eq!(second.value().0.as_ref(), b"derived-output");
    assert_eq!(upstream_executions.load(Ordering::Relaxed), 2);
    assert_eq!(downstream_executions.load(Ordering::Relaxed), 1);
    assert_eq!(second.receipt().read_set().len(), 2);
    assert!(second.receipt().read_set().contains(upstream_action_b));
    assert!(second.receipt().read_set().contains(downstream_action));
    assert!(!second.receipt().read_set().contains(upstream_action_a));
    let read_set = evaluator
        .seal_read_set([second.receipt().read_set().clone()])
        .expect("the rebound transitive read set seals");
    assert_eq!(read_set.len(), 2);
    assert!(read_set.contains(upstream_action_b));
    assert!(read_set.contains(downstream_action));
    assert!(!read_set.contains(upstream_action_a));
    evaluator
        .revalidate_read_set(&read_set)
        .expect("the rebound transitive read set revalidates");
    assert_eq!(counts.hit.load(Ordering::Relaxed), 1);
    assert_eq!(counts.miss.load(Ordering::Relaxed), 3);
    assert_eq!(counts.published.load(Ordering::Relaxed), 3);
    assert_eq!(counts.rejected.load(Ordering::Relaxed), 0);

    let sealed = snapshot(store.root());
    assert_eq!(
        sealed
            .keys()
            .filter(|path| {
                let path = path.to_string_lossy();
                path.contains("content/native-product") && path.ends_with("/payload")
            })
            .count(),
        1,
        "two upstream actions converge on one typed content payload"
    );
    fs::remove_dir_all(root).expect("the semantic-parent store fixture retires");
}

// The per-graph lookup and deep-validation budgets are accounting
// targets. A lookup past either still validates its candidate and records the
// exceeded axis; the per-lookup physical bound on deep validations is
// unchanged.
#[test]
fn discoverable_oversized_historical_files_miss_while_exact_reads_refuse() {
    for kind in 0..4 {
        let root = temporary("discoverable-file-extent");
        let context = prepare_fixture(&root);
        let store = context.open_store().unwrap();
        let recipe = context.discover::<Derived>(b"file-extent", 64).unwrap();
        let first = Evaluator::new(&store, &Counts::default())
            .discover(
                &recipe,
                |value| Ok(value.0.clone()),
                || Ok(Derived(Box::from(b"current".as_slice()))),
            )
            .unwrap();
        let action = store
            .root()
            .join("actions/facade")
            .join(super::STORAGE_DIRECTORY)
            .join(first.receipt().action().directory());
        let content = store
            .root()
            .join("content/facade/00000001")
            .join(first.content().digest().directory());
        let (path, length) = match kind {
            0 => (
                action.join("envelope"),
                super::codec::MAX_ENVELOPE_BYTES as u64 + 1,
            ),
            1 => {
                let path = action.join("allocation");
                let length = fs::metadata(&path).unwrap().len() + 1;
                (path, length)
            }
            2 => (content.join("payload"), 65),
            _ => {
                let path = content.join("allocation");
                let length = fs::metadata(&path).unwrap().len() + 1;
                (path, length)
            }
        };
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(length)
            .unwrap();
        let fresh = fixture_context(&root).open_store().unwrap();
        let validate: super::read_set::Validator<Derived> =
            Arc::new(|_, _| panic!("oversized historical files cannot reach semantic replay"));
        let counts = Counts::default();
        let discovery = Evaluator::new(&fresh, &counts)
            .lookup_discoverable(&recipe, &validate)
            .unwrap();
        assert!(discovery.hit.is_none());
        assert!(discovery.rejected);
        assert!(discovery.rejected_actions.iter().all(Option::is_none));
        assert_eq!(counts.miss.load(Ordering::Relaxed), 0);
        let exact = fixture_context(&root).open_store().unwrap();
        let complete = recipe.complete(b"current").unwrap();
        assert!(matches!(exact.lookup(&complete, exact.graph_budget()),
            Err(Error::Limit { resource: "typed store file bytes", actual, .. })
                if actual as u64 == length));
        assert_eq!(fs::metadata(&path).unwrap().len(), length);
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn discoverable_replay_preserves_first_genuine_failure_without_executing() {
    let root = temporary("discoverable-graph-refusal");
    let context = prepare_fixture(&root);
    let store = context.open_store().unwrap();
    let recipe = context
        .discover::<Derived>(b"graph-refusal", 4_096)
        .unwrap();
    Evaluator::new(&store, &Counts::default())
        .discover(
            &recipe,
            |value| Ok(value.0.clone()),
            || Ok(Derived(Box::from(b"current".as_slice()))),
        )
        .unwrap();
    let fresh = fixture_context(&root).open_store().unwrap();
    let before = snapshot(fresh.root());
    let calls = Arc::new(AtomicUsize::new(0));
    let observed_calls = Arc::clone(&calls);
    let counts = Counts::default();
    let result = Evaluator::new(&fresh, &counts).discover_budgeted(
        &recipe,
        move |value, _| {
            observed_calls.fetch_add(1, Ordering::Relaxed);
            if value.0.as_ref() == b"current" {
                return Err(Error::Allocation);
            }
            Ok(value.0.clone())
        },
        || panic!("a substantive replay failure cannot execute a replacement"),
    );
    assert!(matches!(result, Err(Error::Allocation)));
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(counts.miss.load(Ordering::Relaxed), 0);
    assert_eq!(snapshot(fresh.root()), before);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn discoverable_replay_preserves_nested_account_and_operational_failures() {
    let failures: [fn() -> Error; 10] = [
        || {
            Error::Importer(Box::new(crate::Error::Limit {
                resource: "fixture retained request bytes",
                limit: 7,
                actual: 11,
            }))
        },
        || {
            Error::Importer(Box::new(crate::Error::Node {
                operation: "fixture input replay",
                source: Box::new(Error::Importer(Box::new(crate::Error::Limit {
                    resource: "fixture retained input bytes",
                    limit: 13,
                    actual: 17,
                }))),
            }))
        },
        || Error::Importer(Box::new(crate::Error::InputAccountRefused)),
        || Error::Allocation,
        || {
            Error::Importer(Box::new(crate::Error::Allocation {
                operation: "fixture retained replay storage",
                source: Vec::<u8>::new().try_reserve(usize::MAX).unwrap_err(),
            }))
        },
        || Error::State("fixture account lock poisoned"),
        || Error::Io {
            operation: "fixture retained artifact read",
            path: PathBuf::from("/fixture/retained"),
            source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        },
        || {
            Error::Importer(Box::new(crate::Error::Io {
                operation: "fixture current source read",
                path: PathBuf::from("/fixture/source"),
                source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
            }))
        },
        || Error::ObservationIo {
            operation: "fixture current observation",
            source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        },
        || Error::Cleanup {
            failure: Box::new(Error::Limit {
                resource: "fixture replay allocation",
                limit: 19,
                actual: 23,
            }),
            cleanup: Box::new(Error::State("fixture cleanup failed")),
        },
    ];
    let root = temporary("discoverable-account-refusal");
    let context = prepare_fixture(&root);
    let store = context.open_store().unwrap();
    let recipe = context
        .discover::<Derived>(b"account-refusal", 4_096)
        .unwrap();
    Evaluator::new(&store, &Counts::default())
        .discover(
            &recipe,
            |value| Ok(value.0.clone()),
            || Ok(Derived(Box::from(b"current".as_slice()))),
        )
        .unwrap();
    for failure in failures {
        let fresh = fixture_context(&root).open_store().unwrap();
        let before = snapshot(fresh.root());
        let calls = Arc::new(AtomicUsize::new(0));
        let observed_calls = Arc::clone(&calls);
        let counts = Counts::default();
        let expected = format!("{:?}", failure());
        let result = Evaluator::new(&fresh, &counts).discover(
            &recipe,
            move |_| {
                if observed_calls.fetch_add(1, Ordering::Relaxed) == 0 {
                    Err(failure())
                } else {
                    Err(Error::State("fixture account already refused"))
                }
            },
            || panic!("a replay failure cannot enter miss-side execution"),
        );
        let error = match result {
            Ok(_) => panic!("a failed replay cannot admit a candidate"),
            Err(error) => error,
        };
        assert_eq!(format!("{error:?}"), expected);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(counts.miss.load(Ordering::Relaxed), 0);
        assert_eq!(snapshot(fresh.root()), before);
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn discoverable_node_rejects_stale_candidates_and_reuses_the_current_one() {
    let root = temporary("discoverable-reuse");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the typed store opens");
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let executions = AtomicUsize::new(0);
    let recipe = context
        .discover::<Derived>(b"discoverable-fixture", 4_096)
        .expect("the discoverable recipe seals");

    let current_a = b"discovered-a".as_slice();
    let first = evaluator
        .discover(
            &recipe,
            move |candidate| {
                if candidate.0.as_ref() != current_a {
                    return Err(Error::Codec("discoverable fixture is stale"));
                }
                retain_fixture_bytes(&candidate.0, 4_096, "fixture discovered inputs")
            },
            || {
                executions.fetch_add(1, Ordering::Relaxed);
                Ok(Derived(current_a.to_vec().into_boxed_slice()))
            },
        )
        .expect("the first discoverable action publishes");
    assert_eq!(first.value().0.as_ref(), current_a);

    // Each call models a distinct graph evaluation. A captured Context shares
    // one budget across all evaluators in one graph, while a new capture starts
    // the next graph with the same semantic identity and store placement.
    let second_context = fixture_context(&root);
    let second_store = second_context
        .open_store()
        .expect("the second graph store opens");
    let second_evaluator = Evaluator::new(&second_store, &counts);
    let current_b = b"discovered-b".as_slice();
    let stale_replays = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&stale_replays);
    let second = second_evaluator
        .discover(
            &recipe,
            move |candidate| {
                if candidate.0.as_ref() != current_b {
                    counted.fetch_add(1, Ordering::Relaxed);
                    return Err(Error::Codec("discoverable fixture is stale"));
                }
                retain_fixture_bytes(&candidate.0, 4_096, "fixture discovered inputs")
            },
            || {
                executions.fetch_add(1, Ordering::Relaxed);
                Ok(Derived(current_b.to_vec().into_boxed_slice()))
            },
        )
        .expect("the changed discoverable action publishes");
    assert_eq!(second.value().0.as_ref(), current_b);
    // The lease rechecks look for a concurrent publication, not at the
    // candidate already found stale.
    assert_eq!(stale_replays.load(Ordering::Relaxed), 1);
    let sealed = snapshot(store.root());

    let third_context = fixture_context(&root);
    let third_store = third_context
        .open_store()
        .expect("the third graph store opens");
    let third = Evaluator::new(&third_store, &counts)
        .discover(
            &recipe,
            move |candidate| {
                if candidate.0.as_ref() != current_b {
                    return Err(Error::Codec("discoverable fixture is stale"));
                }
                retain_fixture_bytes(&candidate.0, 4_096, "fixture discovered inputs")
            },
            || {
                executions.fetch_add(1, Ordering::Relaxed);
                Ok(Derived(
                    b"forbidden-reexecution".to_vec().into_boxed_slice(),
                ))
            },
        )
        .expect("the current discoverable candidate validates");
    assert_eq!(third.value().0.as_ref(), current_b);
    assert_eq!(executions.load(Ordering::Relaxed), 2);
    assert_eq!(counts.hit.load(Ordering::Relaxed), 1);
    assert_eq!(counts.miss.load(Ordering::Relaxed), 2);
    assert_eq!(counts.published.load(Ordering::Relaxed), 2);
    assert_eq!(snapshot(store.root()), sealed);
    fs::remove_dir_all(root).expect("the discoverable store fixture retires");
}

#[test]
fn final_read_set_revalidates_external_facts_without_mutating_the_store() {
    let root = temporary("final-read-set");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the typed store opens");
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let current = Arc::new(RwLock::new(
        b"current-discovered-fact".to_vec().into_boxed_slice(),
    ));
    let recipe = context
        .discover::<Derived>(b"final-read-set", 4_096)
        .expect("the discoverable recipe seals");
    let validate_current = Arc::clone(&current);
    let validated = evaluator
        .discover(
            &recipe,
            move |candidate| {
                let current = validate_current
                    .read()
                    .map_err(|_| Error::State("fixture freshness lock poisoned"))?;
                if candidate.0.as_ref() != current.as_ref() {
                    return Err(Error::Codec("fixture external fact changed"));
                }
                retain_fixture_bytes(&current, 4_096, "fixture discovered inputs")
            },
            || {
                Ok(Derived(
                    b"current-discovered-fact".to_vec().into_boxed_slice(),
                ))
            },
        )
        .expect("the discoverable node validates");
    let before = snapshot(store.root());
    let proof = evaluator
        .seal_read_set([validated.receipt().read_set().clone()])
        .expect("the final read set seals");
    assert_eq!(proof.len(), 1);
    assert!(proof.contains(validated.receipt().action()));
    evaluator
        .revalidate_read_set(&proof)
        .expect("exact witness replay remains available");
    assert_eq!(snapshot(store.root()), before);

    *current
        .write()
        .expect("the fixture freshness lock remains healthy") =
        b"changed-discovered-fact".to_vec().into_boxed_slice();
    assert!(matches!(
        evaluator.revalidate_read_set(&proof),
        Err(Error::StaleReadSet { action }) if action == validated.receipt().action()
    ));
    assert_eq!(snapshot(store.root()), before);
    fs::remove_dir_all(root).expect("the final-read-set fixture retires");
}

#[test]
fn final_read_set_rejects_content_drift_after_it_was_sealed() {
    let root = temporary("final-read-set-content");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the typed store opens");
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let recipe = context
        .direct::<Artifact>(b"final-read-set-content", 4_096)
        .expect("the direct recipe seals");
    let validated = evaluator
        .direct(&recipe, || {
            Ok(Artifact(
                b"final-read-set-content".to_vec().into_boxed_slice(),
            ))
        })
        .expect("the direct node publishes");
    let proof = evaluator
        .seal_read_set([validated.receipt().read_set().clone()])
        .expect("the final read set seals");
    let payload = find_file(store.root(), "payload");
    fs::write(&payload, b"tampered").expect("the fixture corrupts stored content");
    assert!(matches!(
        evaluator.revalidate_read_set(&proof),
        Err(Error::Corrupt { .. })
    ));
    fs::remove_dir_all(root).expect("the read-set-content fixture retires");
}

#[test]
fn four_pin_roles_deduplicate_one_exact_closure_and_repeat_without_mutation() {
    use crate::store::PinRole;

    let root = temporary("project-pins");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the typed store opens");
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let recipe = context
        .direct::<Artifact>(b"project-pins", 4_096)
        .expect("the pinned recipe seals");
    let validated = evaluator
        .direct(&recipe, || {
            Ok(Artifact(
                b"project-pinned-content".to_vec().into_boxed_slice(),
            ))
        })
        .expect("the pinned node publishes");
    let proof = evaluator
        .seal_read_set([validated.receipt().read_set().clone()])
        .expect("the pinned closure seals");

    store
        .with_pin(PinRole::Live, &proof, || {
            assert_eq!(
                store.fixture_pin_counts()?,
                (1, 1),
                "the immutable Live root must hold a shared OS lock"
            );
            Ok(())
        })
        .expect("the live closure pins");
    assert_eq!(
        store
            .fixture_pin_counts()
            .expect("the released pin inventory reads"),
        (1, 0),
        "dropping the guard must release liveness without erasing evidence"
    );
    let first = store
        .fixture_project_usage()
        .expect("the first project pin reads");
    assert_eq!((first.1, first.2, first.3, first.4), (5, 9, 0, 1));
    let sealed = snapshot(store.root());
    store
        .with_pin(PinRole::Live, &proof, || {
            assert_eq!(store.fixture_pin_counts()?, (1, 1));
            Ok(())
        })
        .expect("the identical live closure reuses its root");
    assert_eq!(snapshot(store.root()), sealed);

    for role in [PinRole::LastGood, PinRole::Prepared, PinRole::InFlight] {
        store
            .with_pin(role, &proof, || {
                let (_, active) = store.fixture_pin_counts()?;
                assert_eq!(active, 1, "only the scoped role lock may be live");
                Ok(())
            })
            .expect("the exact closure pins under its closed role");
    }
    assert_eq!(
        store
            .fixture_pin_counts()
            .expect("the persistent-role pin inventory reads"),
        (3, 0),
        "the scoped InFlight role is released while the other roles remain"
    );
    let all = store
        .fixture_project_usage()
        .expect("the persistent project roots read");
    assert_eq!((all.1, all.2, all.3, all.4), (7, 11, 0, 3));

    let replacement = context
        .direct::<Artifact>(b"project-pins-replacement", 4_096)
        .expect("the replacement recipe seals");
    let replacement = evaluator
        .direct(&replacement, || {
            Ok(Artifact(
                b"replacement-pinned-content".to_vec().into_boxed_slice(),
            ))
        })
        .expect("the replacement node publishes");
    let replacement = evaluator
        .seal_read_set([replacement.receipt().read_set().clone()])
        .expect("the replacement closure seals");
    store
        .with_pin(PinRole::Live, &replacement, || Ok(()))
        .expect("the live role atomically replaces its closure");
    let replaced = store
        .fixture_project_usage()
        .expect("the replaced project roots read");
    assert_eq!(
        (replaced.1, replaced.2, replaced.3, replaced.4),
        (9, 17, 0, 3)
    );

    let cleanup = context
        .direct::<Artifact>(b"project-pins-cleanup", 4_096)
        .expect("the cleanup recipe seals");
    evaluator
        .direct(&cleanup, || {
            Ok(Artifact(
                b"cleanup-trigger-content".to_vec().into_boxed_slice(),
            ))
        })
        .expect("the next miss performs bounded stale-pin recovery");
    assert_eq!(
        store
            .fixture_pin_counts()
            .expect("the recovered pin inventory reads"),
        (3, 0),
        "only the replaced stale Live record may retire; current role roots remain"
    );

    fs::remove_dir_all(root).expect("the project-pin fixture retires");
}

#[test]
fn project_pin_owner_authenticates_its_exact_referenced_closure() {
    use crate::store::PinRole;

    let root = temporary("project-pin-owner");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the typed store opens");
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let recipe = context
        .direct::<Artifact>(b"project-pin-owner", 4_096)
        .expect("the pinned recipe seals");
    let validated = evaluator
        .direct(&recipe, || {
            Ok(Artifact(b"owner-bound".to_vec().into_boxed_slice()))
        })
        .expect("the pinned node publishes");
    let proof = evaluator
        .seal_read_set([validated.receipt().read_set().clone()])
        .expect("the pinned closure seals");
    store
        .with_pin(PinRole::Prepared, &proof, || Ok(()))
        .expect("the prepared closure pins");

    let project_ledger = fs::read_dir(store.root().join("ledgers/projects"))
        .expect("the project ledger directory reads")
        .next()
        .expect("one project ledger exists")
        .expect("the project ledger entry reads")
        .path();
    let mut bytes = fs::read(&project_ledger).expect("the project ledger bytes read");
    bytes[96] ^= 1;
    fs::write(&project_ledger, bytes).expect("the fixture changes the bound pin owner");
    assert!(matches!(
        store.fixture_project_usage(),
        Err(Error::Corrupt {
            reason: "project logical ledger decoding failed",
            ..
        })
    ));

    fs::remove_dir_all(root).expect("the pin-owner fixture retires");
}

#[test]
fn prepared_chain_pins_its_final_transitive_closure_across_admission_failure() {
    let root = temporary("prepared-chain");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the typed store opens");
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let recipe = context
        .direct::<Artifact>(b"prepared-chain", 4_096)
        .expect("the final recipe seals");
    let validated = evaluator
        .direct(&recipe, || {
            Ok(Artifact(
                b"prepared-final-content".to_vec().into_boxed_slice(),
            ))
        })
        .expect("the final node validates");
    let final_action = validated.receipt().action();
    let prepared = super::Chain::validated(context, &validated)
        .prepare_read_set()
        .expect("the final transitive closure seals");
    assert_eq!(prepared.proof().len(), 1);
    assert!(prepared.proof().contains(final_action));

    let admission: Result<(), &'static str> = prepared
        .with_pin(|| Err("injected admission failure"))
        .expect("the current closure pins before admission");
    assert_eq!(admission, Err("injected admission failure"));
    let usage = store
        .fixture_project_usage()
        .expect("the prepared project pin reads");
    assert_eq!(usage.4, 1, "one Prepared root must remain pinned");
    assert_eq!(
        store
            .fixture_pin_counts()
            .expect("the failed-admission pin inventory reads"),
        (1, 1),
        "the reusable prepared candidate retains its OS lock after admission failure"
    );

    drop(prepared);
    fs::remove_dir_all(root).expect("the prepared-chain fixture retires");
}

#[test]
fn prepared_chain_repeated_live_callbacks_keep_the_publication_boundary() {
    use std::os::fd::AsRawFd as _;

    let root = temporary("prepared-repeat-promotion");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the promotion store opens");
    let prepared = super::Chain::fixture(context)
        .expect("the fixture chain validates")
        .prepare_read_set()
        .expect("the current union seals");
    prepared
        .pin_live_last_good()
        .expect("the initial roles bind");
    let before = snapshot(store.root());
    for reject in [false, true] {
        let outcome = prepared
            .with_live_last_good(|| {
                let publication = fs::File::open(store.root().join("locks/publication.lock"))
                    .expect("the fixed publication lock exists");
                assert!(matches!(
                    nix::fcntl::flock(
                        publication.as_raw_fd(),
                        nix::fcntl::FlockArg::LockExclusiveNonblock
                    ),
                    Err(nix::errno::Errno::EWOULDBLOCK)
                ));
                if reject {
                    Err("repeated admission rejected")
                } else {
                    Ok(())
                }
            })
            .expect("the existing proof binds transactionally");
        assert_eq!(outcome.is_err(), reject);
        assert_eq!(snapshot(store.root()), before);
        assert_eq!(store.fixture_pin_counts().unwrap(), (2, 2));
    }
    drop(prepared);
    store.retire_fixture().unwrap();
    fs::remove_dir_all(root).expect("the fixture project retires");
}

#[test]
fn prepared_chain_promotes_live_and_last_good_as_one_exact_closure() {
    let root = temporary("prepared-promotion");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the typed store opens");
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let recipe = context
        .direct::<Artifact>(b"prepared-promotion", 4_096)
        .expect("the promotion recipe seals");
    let validated = evaluator
        .direct(&recipe, || {
            Ok(Artifact(
                b"promotion-final-content".to_vec().into_boxed_slice(),
            ))
        })
        .expect("the promotion node validates");
    let prepared = super::Chain::validated(context, &validated)
        .prepare_read_set()
        .expect("the promotion read set seals");

    prepared
        .pin_prepared()
        .expect("the prepared role binds before admission");
    assert_eq!(
        store
            .fixture_pin_counts()
            .expect("the retained Prepared pin reads"),
        (1, 1),
        "Prepared ownership must retain its OS lock after bind returns"
    );
    prepared
        .pin_live_last_good()
        .expect("live and last-good bind atomically after admission");
    assert_eq!(
        store
            .fixture_pin_counts()
            .expect("the promoted pin inventory reads"),
        (3, 3),
        "the admitted read-set owner must retain all acquired role locks"
    );
    let usage = store
        .fixture_project_usage()
        .expect("the promoted project roots read");
    assert_eq!(usage.4, 3, "Prepared, Live, and LastGood must coexist");

    drop(prepared);
    assert_eq!(
        store
            .fixture_pin_counts()
            .expect("the retired pin inventory reads"),
        (3, 0),
        "dropping the admitted read-set owner must release every role lock"
    );
    fs::remove_dir_all(root).expect("the prepared-promotion fixture retires");
}

#[test]
fn miss_execution_releases_its_exact_in_flight_parent_closure_on_completion() {
    let root = temporary("in-flight-parent");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the typed store opens");
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let parent_recipe = context
        .direct::<Artifact>(b"in-flight-parent", 4_096)
        .expect("the parent recipe seals");
    let parent = evaluator
        .direct(&parent_recipe, || {
            Ok(Artifact(b"parent".to_vec().into_boxed_slice()))
        })
        .expect("the parent publishes");
    let child_recipe = context
        .direct_with_parents::<Artifact>(b"in-flight-child", 4_096, &[Parent::validated(&parent)])
        .expect("the child recipe seals");

    evaluator
        .direct(&child_recipe, || {
            assert_eq!(
                store.fixture_pin_counts()?,
                (1, 1),
                "the miss executor must run under one live InFlight closure lock"
            );
            Ok(Artifact(b"child".to_vec().into_boxed_slice()))
        })
        .expect("the child publishes under its in-flight parent pin");
    assert_eq!(
        store
            .fixture_pin_counts()
            .expect("the completed miss pin inventory reads"),
        (0, 0),
        "miss completion must retire the exact InFlight logical and OS root"
    );
    assert_eq!(
        store
            .fixture_project_usage()
            .expect("the completed miss project ledger reads")
            .4,
        0,
        "completed execution must not retain stale InFlight authority"
    );

    fs::remove_dir_all(root).expect("the in-flight fixture retires");
}

#[test]
fn in_flight_pin_admission_recovers_a_crash_staged_temporary_first() {
    let root = temporary("in-flight-staged-recovery");
    let context = prepare_fixture(&root);
    let store = context
        .open_store()
        .expect("the staged-recovery store opens");
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let parent_recipe = context
        .direct::<Artifact>(b"in-flight-staged-parent", 4_096)
        .expect("the staged parent recipe seals");
    let parent = evaluator
        .direct(&parent_recipe, || {
            Ok(Artifact(b"staged-parent".to_vec().into_boxed_slice()))
        })
        .expect("the staged parent publishes");
    let child_recipe = context
        .direct_with_parents::<Artifact>(
            b"in-flight-staged-child",
            4_096,
            &[Parent::validated(&parent)],
        )
        .expect("the staged child recipe seals");
    store
        .with_action_lease(child_recipe.action(), || Ok(()))
        .expect("the child keyed lease exists before the crash fixture");
    let staged = store.root().join("temporaries/321-654");
    fs::create_dir(&staged).expect("the staged temporary exists");
    fs::write(staged.join("partial"), b"staged").expect("the staged member exists");

    evaluator
        .direct(&child_recipe, || {
            assert!(
                !staged.exists(),
                "InFlight pin admission must recover temporaries before inventory"
            );
            Ok(Artifact(b"staged-child".to_vec().into_boxed_slice()))
        })
        .expect("the child miss recovers before InFlight pin admission");
    assert!(!staged.exists());
    fs::remove_dir_all(root).expect("the staged InFlight fixture retires");
}

#[test]
fn failed_miss_execution_releases_in_flight_without_masking_the_failure() {
    let root = temporary("in-flight-parent-failure");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the typed store opens");
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let parent_recipe = context
        .direct::<Artifact>(b"in-flight-failure-parent", 4_096)
        .expect("the failure parent recipe seals");
    let parent = evaluator
        .direct(&parent_recipe, || {
            Ok(Artifact(b"parent".to_vec().into_boxed_slice()))
        })
        .expect("the failure parent publishes");
    let child_recipe = context
        .direct_with_parents::<Artifact>(
            b"in-flight-failure-child",
            4_096,
            &[Parent::validated(&parent)],
        )
        .expect("the failing child recipe seals");

    let failure = match evaluator.direct(&child_recipe, || {
        assert_eq!(
            store.fixture_pin_counts()?,
            (1, 1),
            "the failing executor runs under its InFlight closure"
        );
        Err(Error::State("injected executor failure"))
    }) {
        Ok(_) => panic!("the injected executor failure unexpectedly succeeded"),
        Err(failure) => failure,
    };
    assert!(matches!(failure, Error::State("injected executor failure")));
    assert_eq!(
        store
            .fixture_pin_counts()
            .expect("the failed miss pin inventory reads"),
        (0, 0),
        "failure cleanup retires the exact InFlight logical and OS root"
    );
    assert_eq!(
        store
            .fixture_project_usage()
            .expect("the failed miss project ledger reads")
            .4,
        0,
        "failed execution must not retain transient InFlight authority"
    );

    fs::remove_dir_all(root).expect("the failed in-flight fixture retires");
}

#[test]
fn poisoned_candidate_index_clean_misses_without_denying_the_valid_action() {
    let root = temporary("discoverable-poison");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the typed store opens");
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let executions = AtomicUsize::new(0);
    let recipe = context
        .discover::<Derived>(b"discoverable-poison", 4_096)
        .expect("the discoverable recipe seals");
    let validate = |candidate: &Derived| {
        retain_fixture_bytes(&candidate.0, 4_096, "fixture discovered inputs")
    };
    evaluator
        .discover(&recipe, validate, || {
            executions.fetch_add(1, Ordering::Relaxed);
            Ok(Derived(b"valid-action".to_vec().into_boxed_slice()))
        })
        .expect("the discoverable action publishes");
    let row = find_file(store.root(), "row");
    fs::write(&row, b"poisoned-index").expect("the candidate hint is corrupted");

    let second_context = fixture_context(&root);
    let second_store = second_context
        .open_store()
        .expect("the second poisoned-index graph store opens");
    let replayed = Evaluator::new(&second_store, &counts)
        .discover(&recipe, validate, || {
            executions.fetch_add(1, Ordering::Relaxed);
            Ok(Derived(b"valid-action".to_vec().into_boxed_slice()))
        })
        .expect("the poisoned hint cannot deny the revalidated action");
    assert_eq!(replayed.value().0.as_ref(), b"valid-action");
    assert_eq!(executions.load(Ordering::Relaxed), 2);
    assert_eq!(counts.miss.load(Ordering::Relaxed), 2);
    assert_eq!(counts.published.load(Ordering::Relaxed), 1);
    assert_eq!(counts.rejected.load(Ordering::Relaxed), 1);

    let third_context = fixture_context(&root);
    let third_store = third_context
        .open_store()
        .expect("the third poisoned-index graph store opens");
    let replayed = Evaluator::new(&third_store, &counts)
        .discover(&recipe, validate, || {
            executions.fetch_add(1, Ordering::Relaxed);
            Ok(Derived(b"must-not-execute".to_vec().into_boxed_slice()))
        })
        .expect("the repaired hint reuses the already-sealed action");
    assert_eq!(replayed.value().0.as_ref(), b"valid-action");
    assert_eq!(
        executions.load(Ordering::Relaxed),
        2,
        "one clean miss repairs the advisory bucket for the next process"
    );
    fs::remove_dir_all(root).expect("the poisoned-index fixture retires");
}

#[test]
fn same_discoverable_key_contenders_execute_and_publish_exactly_once() {
    let root = temporary("discoverable-contenders");
    let first_context = prepare_fixture(&root);
    let second_context = fixture_context(&root);
    let first_store = first_context
        .open_store()
        .expect("the first contender store opens");
    let second_store = second_context
        .open_store()
        .expect("the second contender store opens");
    let counts = Counts::default();
    let executions = AtomicUsize::new(0);

    std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            evaluate_discoverable_contender(&first_context, &first_store, &counts, &executions)
        });
        let second = scope.spawn(|| {
            evaluate_discoverable_contender(&second_context, &second_store, &counts, &executions)
        });
        assert_eq!(
            first
                .join()
                .expect("the first contender joins")
                .value()
                .0
                .as_ref(),
            b"discovered-once"
        );
        assert_eq!(
            second
                .join()
                .expect("the second contender joins")
                .value()
                .0
                .as_ref(),
            b"discovered-once"
        );
    });

    assert_eq!(executions.load(Ordering::Relaxed), 1);
    assert_eq!(counts.hit.load(Ordering::Relaxed), 1);
    assert_eq!(counts.miss.load(Ordering::Relaxed), 1);
    assert_eq!(counts.published.load(Ordering::Relaxed), 1);
    assert_eq!(counts.rejected.load(Ordering::Relaxed), 0);
    first_store
        .retire_fixture()
        .expect("the contender fixture store retires");
    fs::remove_dir_all(root).expect("the discoverable contender fixture retires");
}

#[cfg(feature = "test-fixtures")]
#[test]
fn same_bundled_key_contenders_allocate_execute_and_publish_once() {
    let root = temporary("bundled-contenders");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the bundled store opens");
    let counts = Counts::default();
    let executions = AtomicUsize::new(0);
    let results = RwLock::new(Vec::new());
    let graph_contexts = [fixture_context(&root), fixture_context(&root)];

    std::thread::scope(|scope| {
        for context in graph_contexts {
            let results = &results;
            let counts = &counts;
            let executions = &executions;
            scope.spawn(move || {
                let recipe = context
                    .discover::<BundleValue>(b"bundled-contender", 4_096)
                    .expect("the bundled contender recipe seals");
                let store = context.open_store().expect("the contender store opens");
                let current = store.clone();
                let validated = Evaluator::new(&store, counts)
                    .discover_bundled(
                        &recipe,
                        move |candidate| candidate.validate_published(&current),
                        |attempt| {
                            executions.fetch_add(1, Ordering::Relaxed);
                            std::thread::sleep(Duration::from_millis(100));
                            bundled_fixture(attempt)
                        },
                    )
                    .expect("the bundled contender executes or reuses");
                results
                    .write()
                    .expect("the bundled result lock writes")
                    .push(validated.content().digest());
            });
        }
    });

    let results = results.into_inner().expect("the bundled results unwrap");
    assert_eq!(results.len(), 2);
    assert_eq!(results[0], results[1]);
    assert_eq!(executions.load(Ordering::Relaxed), 1);
    assert_eq!(counts.hit.load(Ordering::Relaxed), 1);
    assert_eq!(counts.miss.load(Ordering::Relaxed), 1);
    assert_eq!(counts.published.load(Ordering::Relaxed), 1);
    assert_eq!(
        crate::store::bundle::count_bundles(store.root())
            .expect("the bundled contender allocation count reads"),
        1,
    );

    store
        .retire_fixture()
        .expect("the bundled contender store retires");
    fs::remove_dir_all(root).expect("the bundled contender fixture retires");
}

#[cfg(feature = "test-fixtures")]
#[test]
fn native_product_bundle_is_part_of_the_exact_project_pin_closure() {
    let root = temporary("native-bundle-pin-closure");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the bundled store opens");
    let counts = Counts::default();
    let recipe = context
        .discover::<NativeBundleValue>(b"native-bundle-pin-closure", 4_096)
        .expect("the native bundle recipe seals");
    let validation_context = store.clone();
    let product = Evaluator::new(&store, &counts)
        .discover_bundled(
            &recipe,
            move |candidate| candidate.validate_published(&validation_context),
            native_bundled_fixture,
        )
        .expect("the native bundle publishes");
    let proof = Evaluator::new(&store, &counts)
        .seal_read_set([product.receipt().read_set().clone()])
        .expect("the exact native bundle closure seals");

    store
        .with_pin(crate::store::PinRole::Prepared, &proof, || {
            let usage = store.fixture_project_usage()?;
            assert_eq!(
                (usage.1, usage.4),
                (6, 1),
                "action, content, and native bundle must join one Prepared root"
            );
            Ok(())
        })
        .expect("the exact native bundle closure pins");

    store
        .retire_fixture()
        .expect("the native bundle fixture store retires");
    fs::remove_dir_all(root).expect("the native bundle fixture retires");
}

#[cfg(feature = "test-fixtures")]
#[test]
fn runner_eviction_uses_seal_order_and_removes_one_whole_unpinned_unit() {
    let root = temporary("runner-eviction-order");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the eviction store opens");
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let evaluate = |semantic: &'static [u8]| {
        let current = store.clone();
        let recipe = context
            .discover::<BundleValue>(semantic, 4_096)
            .expect("the runner recipe seals");
        let validated = evaluator
            .discover_bundled(
                &recipe,
                move |candidate| candidate.validate_published(&current),
                bundled_fixture,
            )
            .expect("the synthetic runner publishes");
        (recipe.lookup(), validated)
    };
    let (first_lookup, first) = evaluate(b"runner-eviction-first");
    let (second_lookup, second) = evaluate(b"runner-eviction-second");
    let first_action = store
        .root()
        .join("actions/runner")
        .join(super::STORAGE_DIRECTORY)
        .join(first.receipt().action().directory());
    let second_action = store
        .root()
        .join("actions/runner")
        .join(super::STORAGE_DIRECTORY)
        .join(second.receipt().action().directory());
    let first_content = store
        .root()
        .join("content/runner/00000002")
        .join(first.content().digest().directory());
    let second_content = store
        .root()
        .join("content/runner/00000002")
        .join(second.content().digest().directory());
    let first_index = store
        .root()
        .join("indexes/runner")
        .join(first_lookup.directory())
        .join(first.receipt().action().directory());
    let second_index = store
        .root()
        .join("indexes/runner")
        .join(second_lookup.directory())
        .join(second.receipt().action().directory());
    assert!(first_action.is_dir() && second_action.is_dir());
    assert!(first_content.is_dir() && second_content.is_dir());
    assert!(first_index.is_dir() && second_index.is_dir());
    assert_eq!(
        crate::store::bundle::count_bundles(store.root()).expect("the two runner bundles count"),
        2
    );

    assert!(store
        .fixture_evict_one(false)
        .expect("one unpinned runner evicts"));
    assert!(
        !first_action.exists(),
        "the oldest runner action must retire"
    );
    assert!(
        second_action.is_dir(),
        "the newer runner action must remain"
    );
    assert!(
        !first_content.exists(),
        "the newly unreachable runner content must retire"
    );
    assert!(
        second_content.is_dir(),
        "the retained runner content must remain"
    );
    assert!(!first_index.exists(), "the retired index row must retire");
    assert!(second_index.is_dir(), "the retained index row must remain");
    assert_eq!(
        crate::store::bundle::count_bundles(store.root())
            .expect("the retained runner bundle counts"),
        1
    );

    store
        .retire_fixture()
        .expect("the runner-eviction store retires");
    fs::remove_dir_all(root).expect("the runner-eviction fixture retires");
}

#[cfg(feature = "test-fixtures")]
#[test]
fn runner_eviction_skips_an_active_os_pinned_root() {
    let root = temporary("runner-eviction-pin");
    let context = prepare_fixture(&root);
    let store = context
        .open_store()
        .expect("the pinned eviction store opens");
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let first_recipe = context
        .discover::<BundleValue>(b"runner-eviction-pinned", 4_096)
        .expect("the pinned runner recipe seals");
    let current = store.clone();
    let first = evaluator
        .discover_bundled(
            &first_recipe,
            move |candidate| candidate.validate_published(&current),
            bundled_fixture,
        )
        .expect("the pinned runner publishes");
    let proof = evaluator
        .seal_read_set([first.receipt().read_set().clone()])
        .expect("the pinned runner closure seals");
    let _guard = store
        .acquire_pin(crate::store::PinRole::Live, &proof)
        .expect("the runner Live root pins");
    let second_recipe = context
        .discover::<BundleValue>(b"runner-eviction-unpinned", 4_096)
        .expect("the unpinned runner recipe seals");
    let current = store.clone();
    let second = evaluator
        .discover_bundled(
            &second_recipe,
            move |candidate| candidate.validate_published(&current),
            bundled_fixture,
        )
        .expect("the unpinned runner publishes");
    let first_action = store
        .root()
        .join("actions/runner")
        .join(super::STORAGE_DIRECTORY)
        .join(first.receipt().action().directory());
    let second_action = store
        .root()
        .join("actions/runner")
        .join(super::STORAGE_DIRECTORY)
        .join(second.receipt().action().directory());

    assert!(store
        .fixture_evict_one(false)
        .expect("one unpinned runner evicts beside the live root"));
    assert!(first_action.is_dir(), "the active Live root must remain");
    assert!(!second_action.exists(), "the unpinned runner may retire");

    drop(_guard);
    store
        .retire_fixture()
        .expect("the pinned runner-eviction store retires");
    fs::remove_dir_all(root).expect("the pinned runner-eviction fixture retires");
}

#[cfg(feature = "test-fixtures")]
#[test]
fn runner_eviction_acquires_the_bundle_exclusively_before_publication() {
    let root = temporary("runner-eviction-bundle-lease");
    let context = prepare_fixture(&root);
    let store = context
        .open_store()
        .expect("the bundle-lease eviction store opens");
    let counts = Counts::default();
    let recipe = context
        .discover::<BundleValue>(b"runner-eviction-bundle-lease", 4_096)
        .expect("the bundle-lease runner recipe seals");
    let current = store.clone();
    let runner = Evaluator::new(&store, &counts)
        .discover_bundled(
            &recipe,
            move |candidate| candidate.validate_published(&current),
            bundled_fixture,
        )
        .expect("the bundle-lease runner publishes");
    let action = store
        .root()
        .join("actions/runner")
        .join(super::STORAGE_DIRECTORY)
        .join(runner.receipt().action().directory());
    let guard = crate::store::bundle::pin(store.root(), runner.value().reference)
        .expect("the runner bundle holds its shared process lease");

    assert!(!store
        .fixture_evict_one(false)
        .expect("the active bundle is skipped without waiting under publication"));
    assert!(action.is_dir(), "the active bundle action must remain");

    drop(guard);
    assert!(store
        .fixture_evict_one(false)
        .expect("the released bundle becomes evictable"));
    assert!(!action.exists(), "the released whole unit must retire");

    store
        .retire_fixture()
        .expect("the bundle-lease eviction store retires");
    fs::remove_dir_all(root).expect("the bundle-lease fixture retires");
}

#[cfg(feature = "test-fixtures")]
#[test]
fn project_local_eviction_retires_its_stale_logical_root_and_whole_bundle() {
    let root = temporary("runner-eviction-project-root");
    let context = prepare_fixture(&root);
    let store = context
        .open_store()
        .expect("the local eviction store opens");
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let recipe = context
        .discover::<BundleValue>(b"runner-eviction-project-root", 4_096)
        .expect("the retained runner recipe seals");
    let current = store.clone();
    let runner = evaluator
        .discover_bundled(
            &recipe,
            move |candidate| candidate.validate_published(&current),
            bundled_fixture,
        )
        .expect("the retained runner publishes");
    let proof = evaluator
        .seal_read_set([runner.receipt().read_set().clone()])
        .expect("the retained runner closure seals");
    store
        .with_pin(crate::store::PinRole::Live, &proof, || Ok(()))
        .expect("the project Live root binds and then becomes stale");
    assert_eq!(
        store.fixture_pin_counts().expect("the stale pin reads"),
        (1, 0)
    );
    assert_eq!(
        store
            .fixture_project_usage()
            .expect("the retained project root reads")
            .4,
        1
    );
    let action = store
        .root()
        .join("actions/runner")
        .join(super::STORAGE_DIRECTORY)
        .join(runner.receipt().action().directory());

    assert!(store
        .fixture_evict_one(true)
        .expect("the requesting project's stale root evicts"));
    assert!(!action.exists());
    assert_eq!(store.fixture_pin_counts().expect("the pin retires"), (0, 0));
    assert_eq!(
        store
            .fixture_project_usage()
            .expect("the project root retirement reads")
            .4,
        0
    );
    assert_eq!(
        crate::store::bundle::count_bundles(store.root()).expect("the whole runner bundle retires"),
        0
    );

    store
        .retire_fixture()
        .expect("the local runner-eviction store retires");
    fs::remove_dir_all(root).expect("the local runner-eviction fixture retires");
}

#[cfg(feature = "test-fixtures")]
#[test]
fn stale_runner_retirement_reclaims_its_newly_unreachable_native_bundle_closure() {
    let root = temporary("runner-eviction-native-closure");
    let context = prepare_fixture(&root);
    let store = context
        .open_store()
        .expect("the closure-eviction store opens");
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let native_recipe = context
        .discover::<NativeBundleValue>(b"runner-eviction-native-parent", 4_096)
        .expect("the native parent recipe seals");
    let validation_context = store.clone();
    let native = evaluator
        .discover_bundled(
            &native_recipe,
            move |candidate| candidate.validate_published(&validation_context),
            native_bundled_fixture,
        )
        .expect("the native parent bundle publishes");
    let runner_recipe = context
        .discover_with_parents::<BundleValue>(
            b"runner-eviction-native-child",
            4_096,
            &[Parent::validated(&native)],
        )
        .expect("the runner child recipe seals");
    let validation_context = store.clone();
    let runner = evaluator
        .discover_bundled(
            &runner_recipe,
            move |candidate| candidate.validate_published(&validation_context),
            bundled_fixture,
        )
        .expect("the runner child bundle publishes");
    let proof = evaluator
        .seal_read_set([runner.receipt().read_set().clone()])
        .expect("the runner and native closure seals");
    store
        .with_pin(crate::store::PinRole::Live, &proof, || Ok(()))
        .expect("the full Live closure binds and becomes stale");
    let action = |kind: &str, action: super::ActionKey| {
        store
            .root()
            .join("actions")
            .join(kind)
            .join(super::STORAGE_DIRECTORY)
            .join(action.directory())
    };
    let native_action = action("native-product", native.receipt().action());
    let runner_action = action("runner", runner.receipt().action());
    assert!(native_action.is_dir() && runner_action.is_dir());
    assert_eq!(
        crate::store::bundle::count_bundles(store.root())
            .expect("the native and runner bundles count"),
        2
    );

    assert!(store
        .fixture_evict_one(true)
        .expect("the released rooted closure evicts in one bounded pass"));
    assert!(!native_action.exists());
    assert!(!runner_action.exists());
    assert_eq!(
        crate::store::bundle::count_bundles(store.root())
            .expect("the released closure has no retained bundles"),
        0
    );
    assert_eq!(
        store
            .fixture_project_usage()
            .expect("the retired project closure reads")
            .4,
        0
    );

    store
        .retire_fixture()
        .expect("the closure-eviction store retires");
    fs::remove_dir_all(root).expect("the closure-eviction fixture retires");
}

#[cfg(feature = "test-fixtures")]
#[test]
fn released_runner_preserves_a_native_bundle_reachable_from_another_root() {
    let root = temporary("runner-eviction-shared-native");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the shared-native store opens");
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let native_recipe = context
        .discover::<NativeBundleValue>(b"runner-eviction-shared-native", 4_096)
        .expect("the shared native recipe seals");
    let validation_context = store.clone();
    let native = evaluator
        .discover_bundled(
            &native_recipe,
            move |candidate| candidate.validate_published(&validation_context),
            native_bundled_fixture,
        )
        .expect("the shared native bundle publishes");
    let evaluate_runner = |semantic: &'static [u8]| {
        let recipe = context
            .discover_with_parents::<BundleValue>(semantic, 4_096, &[Parent::validated(&native)])
            .expect("the shared-native runner recipe seals");
        let validation_context = store.clone();
        evaluator
            .discover_bundled(
                &recipe,
                move |candidate| candidate.validate_published(&validation_context),
                bundled_fixture,
            )
            .expect("the shared-native runner publishes")
    };
    let first = evaluate_runner(b"runner-eviction-shared-native-first");
    let second = evaluate_runner(b"runner-eviction-shared-native-second");
    let first_proof = evaluator
        .seal_read_set([first.receipt().read_set().clone()])
        .expect("the first rooted closure seals");
    let second_proof = evaluator
        .seal_read_set([second.receipt().read_set().clone()])
        .expect("the second rooted closure seals");
    store
        .with_pin(crate::store::PinRole::LastGood, &first_proof, || Ok(()))
        .expect("the first closure becomes stale LastGood");
    store
        .with_pin(crate::store::PinRole::Live, &second_proof, || Ok(()))
        .expect("the second closure becomes stale Live");
    let action = |kind: &str, action: super::ActionKey| {
        store
            .root()
            .join("actions")
            .join(kind)
            .join(super::STORAGE_DIRECTORY)
            .join(action.directory())
    };
    let native_action = action("native-product", native.receipt().action());
    let first_action = action("runner", first.receipt().action());
    let second_action = action("runner", second.receipt().action());

    assert!(store
        .fixture_evict_one(true)
        .expect("the older rooted runner closure retires"));
    assert!(!first_action.exists());
    assert!(native_action.is_dir());
    assert!(second_action.is_dir());
    assert_eq!(
        crate::store::bundle::count_bundles(store.root())
            .expect("the shared native and remaining runner bundles count"),
        2
    );
    assert_eq!(
        store
            .fixture_project_usage()
            .expect("the remaining rooted closure reads")
            .4,
        1
    );

    store
        .retire_fixture()
        .expect("the shared-native store retires");
    fs::remove_dir_all(root).expect("the shared-native fixture retires");
}

#[cfg(feature = "test-fixtures")]
#[test]
fn active_upstream_bundle_lease_blocks_the_entire_released_closure() {
    let root = temporary("runner-eviction-upstream-lease");
    let context = prepare_fixture(&root);
    let store = context
        .open_store()
        .expect("the upstream-lease store opens");
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let native_recipe = context
        .discover::<NativeBundleValue>(b"runner-eviction-upstream-lease", 4_096)
        .expect("the leased native recipe seals");
    let validation_context = store.clone();
    let native = evaluator
        .discover_bundled(
            &native_recipe,
            move |candidate| candidate.validate_published(&validation_context),
            native_bundled_fixture,
        )
        .expect("the leased native bundle publishes");
    let runner_recipe = context
        .discover_with_parents::<BundleValue>(
            b"runner-eviction-upstream-lease-child",
            4_096,
            &[Parent::validated(&native)],
        )
        .expect("the leased runner recipe seals");
    let validation_context = store.clone();
    let runner = evaluator
        .discover_bundled(
            &runner_recipe,
            move |candidate| candidate.validate_published(&validation_context),
            bundled_fixture,
        )
        .expect("the leased runner bundle publishes");
    let proof = evaluator
        .seal_read_set([runner.receipt().read_set().clone()])
        .expect("the leased closure seals");
    store
        .with_pin(crate::store::PinRole::Live, &proof, || Ok(()))
        .expect("the leased closure becomes stale Live");
    let native_guard = crate::store::bundle::pin(store.root(), native.value().0.reference)
        .expect("the native bundle holds a shared lease");
    let runner_action = store
        .root()
        .join("actions/runner")
        .join(super::STORAGE_DIRECTORY)
        .join(runner.receipt().action().directory());

    assert!(!store
        .fixture_evict_one(true)
        .expect("the active upstream lease skips the complete closure"));
    assert!(runner_action.is_dir());
    assert_eq!(
        store
            .fixture_project_usage()
            .expect("the blocked logical root remains")
            .4,
        1
    );
    drop(native_guard);
    assert!(store
        .fixture_evict_one(true)
        .expect("the released upstream lease admits closure retirement"));
    assert!(!runner_action.exists());

    store
        .retire_fixture()
        .expect("the upstream-lease store retires");
    fs::remove_dir_all(root).expect("the upstream-lease fixture retires");
}

#[cfg(feature = "test-fixtures")]
#[test]
fn one_stale_pin_root_reclaims_its_complete_multi_runner_closure_in_one_pass() {
    let root = temporary("runner-eviction-multi-root");
    let context = prepare_fixture(&root);
    let store = context
        .open_store()
        .expect("the multi-root eviction store opens");
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let evaluate = |semantic: &'static [u8]| {
        let current = store.clone();
        let recipe = context
            .discover::<BundleValue>(semantic, 4_096)
            .expect("the multi-root runner recipe seals");
        evaluator
            .discover_bundled(
                &recipe,
                move |candidate| candidate.validate_published(&current),
                bundled_fixture,
            )
            .expect("the multi-root runner publishes")
    };
    let first = evaluate(b"runner-eviction-multi-first");
    let second = evaluate(b"runner-eviction-multi-second");
    let proof = evaluator
        .seal_read_set([
            first.receipt().read_set().clone(),
            second.receipt().read_set().clone(),
        ])
        .expect("the two-runner closure seals");
    store
        .with_pin(crate::store::PinRole::Live, &proof, || Ok(()))
        .expect("the two-runner Live root binds and becomes stale");
    let first_action = store
        .root()
        .join("actions/runner")
        .join(super::STORAGE_DIRECTORY)
        .join(first.receipt().action().directory());
    let second_action = store
        .root()
        .join("actions/runner")
        .join(super::STORAGE_DIRECTORY)
        .join(second.receipt().action().directory());

    assert_eq!(
        store
            .fixture_project_usage()
            .expect("the multi-runner project root reads")
            .4,
        1
    );
    assert!(store
        .fixture_evict_one(true)
        .expect("the stale multi-runner root releases its complete closure"));
    assert!(!first_action.exists());
    assert!(!second_action.exists());
    assert_eq!(
        store
            .fixture_project_usage()
            .expect("the stale strong root retires")
            .4,
        0
    );
    assert_eq!(
        crate::store::bundle::count_bundles(store.root())
            .expect("the complete multi-runner closure retires in one pass"),
        0
    );

    store
        .retire_fixture()
        .expect("the multi-root eviction store retires");
    fs::remove_dir_all(root).expect("the multi-root eviction fixture retires");
}

#[cfg(feature = "test-fixtures")]
#[test]
fn interrupted_runner_eviction_recovers_exactly_one_journaled_unit() {
    let root = temporary("runner-eviction-interrupted");
    let context = prepare_fixture(&root);
    let store = context
        .open_store()
        .expect("the interrupted eviction store opens");
    let counts = Counts::default();
    let recipe = context
        .discover::<BundleValue>(b"runner-eviction-interrupted", 4_096)
        .expect("the interrupted runner recipe seals");
    let current = store.clone();
    let runner = Evaluator::new(&store, &counts)
        .discover_bundled(
            &recipe,
            move |candidate| candidate.validate_published(&current),
            bundled_fixture,
        )
        .expect("the interrupted runner publishes");
    let action = store
        .root()
        .join("actions/runner")
        .join(super::STORAGE_DIRECTORY)
        .join(runner.receipt().action().directory());
    let before = store.fixture_usage().expect("the sealed usage reads");
    assert!(action.is_dir());
    assert_eq!(
        crate::store::bundle::count_bundles(store.root()).expect("the sealed runner bundle counts"),
        1
    );

    store
        .fixture_interrupt_oldest_eviction()
        .expect("the fixture journals the oldest whole runner unit");
    assert!(
        action.is_dir(),
        "the interruption fixture must stop before physical retirement"
    );
    assert_eq!(
        store
            .fixture_usage()
            .expect("the journaled usage remains charged"),
        before
    );
    assert_eq!(
        crate::store::bundle::count_bundles(store.root())
            .expect("the journaled runner bundle remains"),
        1
    );

    assert!(store
        .fixture_evict_one(false)
        .expect("the interrupted whole runner unit recovers"));
    assert!(!action.exists());
    assert_eq!(
        crate::store::bundle::count_bundles(store.root())
            .expect("the recovered runner bundle retires"),
        0
    );
    let after = store.fixture_usage().expect("the recovered usage reads");
    assert!(after.0 < before.0);
    assert!(after.1 < before.1);
    assert!(!store
        .fixture_evict_one(false)
        .expect("recovery does not select a second unit"));

    store
        .retire_fixture()
        .expect("the interrupted runner-eviction store retires");
    fs::remove_dir_all(root).expect("the interrupted runner-eviction fixture retires");
}

#[cfg(feature = "test-fixtures")]
#[test]
fn ordinary_mutation_recovers_a_pending_eviction_before_reserving() {
    let root = temporary("runner-eviction-before-reserve");
    let context = prepare_fixture(&root);
    let store = context
        .open_store()
        .expect("the pending-eviction store opens");
    let runner_recipe = context
        .discover::<BundleValue>(b"runner-eviction-before-reserve", 4_096)
        .expect("the runner recipe seals");
    let current = store.clone();
    Evaluator::new(&store, &Counts::default())
        .discover_bundled(
            &runner_recipe,
            move |candidate| candidate.validate_published(&current),
            bundled_fixture,
        )
        .expect("the runner publishes");
    store
        .fixture_interrupt_oldest_eviction()
        .expect("the runner eviction journal persists");

    let direct = context
        .direct::<Artifact>(b"mutation-after-interruption", 4_096)
        .expect("the direct recipe seals");
    Evaluator::new(&store, &Counts::default())
        .direct(&direct, || {
            Ok(Artifact(b"after-interruption".to_vec().into_boxed_slice()))
        })
        .expect("ordinary mutation recovers the journal before reserving");
    assert_eq!(
        crate::store::bundle::count_bundles(store.root())
            .expect("the recovered bundle count reads"),
        0
    );
    assert!(!store
        .fixture_has_pending_eviction()
        .expect("the recovered eviction journal reads empty"));
    store
        .retire_fixture()
        .expect("the pending-eviction store retires");
    fs::remove_dir_all(root).expect("the pending-eviction fixture retires");
}

#[cfg(feature = "test-fixtures")]
#[test]
fn interrupted_bundle_root_thaw_is_one_journaled_recoverable_transition() {
    let root = temporary("runner-eviction-root-thaw");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the root-thaw store opens");
    let recipe = context
        .discover::<BundleValue>(b"runner-eviction-root-thaw", 4_096)
        .expect("the root-thaw runner recipe seals");
    let current = store.clone();
    Evaluator::new(&store, &Counts::default())
        .discover_bundled(
            &recipe,
            move |candidate| candidate.validate_published(&current),
            bundled_fixture,
        )
        .expect("the root-thaw runner publishes");

    let bundle = store
        .fixture_interrupt_after_thawing_bundle_root()
        .expect("the journaled bundle root becomes renameable");
    assert!(bundle.is_dir());
    assert!(store
        .fixture_evict_one(false)
        .expect("the exact journaled root-thaw transition recovers"));
    assert!(!bundle.exists());
    assert_eq!(
        crate::store::bundle::count_bundles(store.root())
            .expect("the root-thaw bundle count reads"),
        0
    );
    assert!(!store
        .fixture_evict_one(false)
        .expect("root-thaw recovery leaves no second candidate"));
    store.retire_fixture().expect("the root-thaw store retires");
    fs::remove_dir_all(root).expect("the root-thaw fixture retires");
}

#[cfg(feature = "test-fixtures")]
#[test]
fn interrupted_eviction_recovers_a_partially_removed_staged_action() {
    let root = temporary("runner-eviction-staged-crash");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the staged-crash store opens");
    let recipe = context
        .discover::<BundleValue>(b"runner-eviction-staged-crash", 4_096)
        .expect("the staged-crash runner recipe seals");
    let current = store.clone();
    Evaluator::new(&store, &Counts::default())
        .discover_bundled(
            &recipe,
            move |candidate| candidate.validate_published(&current),
            bundled_fixture,
        )
        .expect("the staged-crash runner publishes");

    let temporary = store
        .fixture_interrupt_after_staging_action()
        .expect("the action is atomically staged before the simulated crash");
    let member = fs::read_dir(&temporary)
        .expect("the staged action reads")
        .next()
        .expect("the staged action has one member")
        .expect("the staged action member reads")
        .path();
    fs::remove_file(member).expect("the simulated crash interrupts recursive deletion");

    assert!(store
        .fixture_evict_one(false)
        .expect("the staged partial deletion recovers and finishes its journal"));
    assert!(!temporary.exists());
    assert_eq!(
        crate::store::bundle::count_bundles(store.root())
            .expect("the staged-crash bundle count reads"),
        0
    );
    assert!(!store
        .fixture_evict_one(false)
        .expect("the recovered staged crash leaves no second candidate"));
    store
        .retire_fixture()
        .expect("the staged-crash store retires");
    fs::remove_dir_all(root).expect("the staged-crash fixture retires");
}

#[cfg(feature = "test-fixtures")]
#[test]
fn interrupted_released_closure_recovers_native_and_runner_units_once() {
    let root = temporary("runner-eviction-interrupted-closure");
    let context = prepare_fixture(&root);
    let store = context
        .open_store()
        .expect("the interrupted-closure store opens");
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let native_recipe = context
        .discover::<NativeBundleValue>(b"runner-eviction-interrupted-native", 4_096)
        .expect("the interrupted native recipe seals");
    let validation_context = store.clone();
    let native = evaluator
        .discover_bundled(
            &native_recipe,
            move |candidate| candidate.validate_published(&validation_context),
            native_bundled_fixture,
        )
        .expect("the interrupted native bundle publishes");
    let runner_recipe = context
        .discover_with_parents::<BundleValue>(
            b"runner-eviction-interrupted-child",
            4_096,
            &[Parent::validated(&native)],
        )
        .expect("the interrupted runner recipe seals");
    let validation_context = store.clone();
    let runner = evaluator
        .discover_bundled(
            &runner_recipe,
            move |candidate| candidate.validate_published(&validation_context),
            bundled_fixture,
        )
        .expect("the interrupted runner bundle publishes");
    let proof = evaluator
        .seal_read_set([runner.receipt().read_set().clone()])
        .expect("the interrupted closure seals");
    store
        .with_pin(crate::store::PinRole::Live, &proof, || Ok(()))
        .expect("the interrupted closure becomes stale Live");
    let action = |kind: &str, action: super::ActionKey| {
        store
            .root()
            .join("actions")
            .join(kind)
            .join(super::STORAGE_DIRECTORY)
            .join(action.directory())
    };
    let native_action = action("native-product", native.receipt().action());
    let runner_action = action("runner", runner.receipt().action());
    let before = store
        .fixture_usage()
        .expect("the interrupted closure usage reads");

    store
        .fixture_interrupt_oldest_eviction()
        .expect("the fixture journals the complete released closure");
    assert!(native_action.is_dir());
    assert!(runner_action.is_dir());
    assert_eq!(
        crate::store::bundle::count_bundles(store.root())
            .expect("both journaled bundles remain before recovery"),
        2
    );
    assert_eq!(
        store
            .fixture_usage()
            .expect("the journaled closure remains charged"),
        before
    );
    assert_eq!(
        store
            .fixture_project_usage()
            .expect("the released project root remains retired")
            .4,
        0
    );

    assert!(store
        .fixture_evict_one(false)
        .expect("the interrupted complete closure recovers"));
    assert!(!native_action.exists());
    assert!(!runner_action.exists());
    assert_eq!(
        crate::store::bundle::count_bundles(store.root()).expect("both recovered bundles retire"),
        0
    );
    let after = store
        .fixture_usage()
        .expect("the recovered closure usage reads");
    assert!(after.0 < before.0);
    assert!(after.1 < before.1);
    assert!(!store
        .fixture_evict_one(false)
        .expect("the recovered closure is not selected twice"));

    store
        .retire_fixture()
        .expect("the interrupted-closure store retires");
    fs::remove_dir_all(root).expect("the interrupted-closure fixture retires");
}

#[cfg(all(feature = "test-fixtures", unix))]
#[test]
fn corrupt_bundled_candidate_repairs_once_and_then_reuses_the_repair() {
    use std::os::unix::fs::PermissionsExt as _;

    let root = temporary("bundled-repair");
    let context = prepare_fixture(&root);
    let store = context
        .open_store()
        .expect("the bundled repair store opens");
    let counts = Counts::default();
    let executions = AtomicUsize::new(0);
    let recipe = context
        .discover::<BundleValue>(b"bundled-repair", 4_096)
        .expect("the bundled repair recipe seals");

    let current = store.clone();
    let first = Evaluator::new(&store, &counts)
        .discover_bundled(
            &recipe,
            move |candidate| candidate.validate_published(&current),
            |attempt| {
                executions.fetch_add(1, Ordering::Relaxed);
                bundled_fixture(attempt)
            },
        )
        .expect("the first bundled candidate publishes");
    let first_content = first.content().digest();
    let first_root = store.root().join(first.value().reference.relative());
    let generated = first_root.join("generated");
    fs::set_permissions(&generated, fs::Permissions::from_mode(0o755))
        .expect("the corrupt fixture generated directory thaws");
    fs::remove_file(generated.join("input")).expect("the corrupt fixture removes one input");

    let second_context = fixture_context(&root);
    let second_store = second_context
        .open_store()
        .expect("the repair graph store opens");
    let current = second_store.clone();
    let repaired = Evaluator::new(&second_store, &counts)
        .discover_bundled(
            &recipe,
            move |candidate| candidate.validate_published(&current),
            |attempt| {
                executions.fetch_add(1, Ordering::Relaxed);
                bundled_fixture(attempt)
            },
        )
        .expect("the corrupt bundled candidate repairs");
    assert_ne!(repaired.content().digest(), first_content);

    let third_context = fixture_context(&root);
    let third_store = third_context
        .open_store()
        .expect("the reuse graph store opens");
    let current = third_store.clone();
    let reused = Evaluator::new(&third_store, &counts)
        .discover_bundled(
            &recipe,
            move |candidate| candidate.validate_published(&current),
            |_attempt| {
                Err(Error::State(
                    "a repaired bundle unexpectedly executed again",
                ))
            },
        )
        .expect("the repaired bundled candidate reuses");
    assert_eq!(reused.content().digest(), repaired.content().digest());
    assert_eq!(executions.load(Ordering::Relaxed), 2);
    assert_eq!(counts.miss.load(Ordering::Relaxed), 2);
    assert_eq!(counts.published.load(Ordering::Relaxed), 2);
    assert_eq!(counts.hit.load(Ordering::Relaxed), 1);
    assert_eq!(counts.rejected.load(Ordering::Relaxed), 1);
    assert_eq!(
        crate::store::bundle::count_bundles(store.root())
            .expect("only the repaired bundle remains after whole retirement"),
        1,
    );

    store
        .retire_fixture()
        .expect("the bundled repair store retires");
    fs::remove_dir_all(root).expect("the bundled repair fixture retires");
}

fn evaluate_discoverable_contender<'store>(
    context: &Context,
    store: &'store crate::store::Store,
    counts: &'store Counts,
    executions: &AtomicUsize,
) -> super::Validated<Derived> {
    let recipe = context
        .discover::<Derived>(b"discoverable-contender", 4_096)
        .expect("the discoverable contender recipe seals");
    Evaluator::new(store, counts)
        .discover(
            &recipe,
            |candidate| retain_fixture_bytes(&candidate.0, 4_096, "fixture discovered inputs"),
            || {
                executions.fetch_add(1, Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(100));
                Ok(Derived(b"discovered-once".to_vec().into_boxed_slice()))
            },
        )
        .expect("the discoverable contender validates")
}

#[test]
fn corrupt_content_rejects_before_the_transform_can_execute_again() {
    let root = temporary("corrupt-content");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the typed store opens");
    let counts = Counts::default();
    let executions = AtomicUsize::new(0);
    let recipe = context
        .direct::<Artifact>(b"rawr.phase9.native-product.synthetic/v1\0tamper", 4_096)
        .expect("the direct recipe seals");
    Evaluator::new(&store, &counts)
        .direct(&recipe, || {
            executions.fetch_add(1, Ordering::Relaxed);
            Ok(Artifact(b"sealed".to_vec().into_boxed_slice()))
        })
        .expect("the first transform publishes");
    let payload = find_file(store.root(), "payload");
    fs::write(&payload, b"broken").expect("the test corrupts the exact payload");
    assert!(matches!(
        Evaluator::new(&store, &counts).direct(&recipe, || {
            executions.fetch_add(1, Ordering::Relaxed);
            Ok(Artifact(b"replacement".to_vec().into_boxed_slice()))
        }),
        Err(Error::Corrupt { .. })
    ));
    assert_eq!(executions.load(Ordering::Relaxed), 1);
    assert_eq!(counts.rejected.load(Ordering::Relaxed), 1);
    fs::remove_dir_all(root).expect("the corrupt store fixture retires");
}

#[test]
fn allocation_registry_rejects_a_rewritten_immutable_record() {
    let root = temporary("corrupt-allocation-record");
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the typed store opens");
    let counts = Counts::default();
    let executions = AtomicUsize::new(0);
    let recipe = context
        .direct::<Artifact>(b"corrupt-allocation-record", 4_096)
        .expect("the direct recipe seals");
    Evaluator::new(&store, &counts)
        .direct(&recipe, || {
            executions.fetch_add(1, Ordering::Relaxed);
            Ok(Artifact(b"sealed".to_vec().into_boxed_slice()))
        })
        .expect("the first transform publishes");
    let allocation = store
        .root()
        .join("actions/native-product")
        .join(super::STORAGE_DIRECTORY)
        .join(recipe.action().directory())
        .join("allocation");
    let mut bytes = fs::read(&allocation).expect("the allocation record reads");
    let last = bytes
        .last_mut()
        .expect("the allocation record is not empty");
    *last ^= 1;
    fs::write(&allocation, bytes).expect("the immutable audit field is rewritten");

    assert!(matches!(
        Evaluator::new(&store, &counts).direct(&recipe, || {
            executions.fetch_add(1, Ordering::Relaxed);
            Ok(Artifact(b"forbidden".to_vec().into_boxed_slice()))
        }),
        Err(Error::Corrupt {
            reason: "typed store allocation registry digest changed",
            ..
        })
    ));
    assert_eq!(executions.load(Ordering::Relaxed), 1);
    assert_eq!(counts.rejected.load(Ordering::Relaxed), 1);
    fs::remove_dir_all(root).expect("the allocation-record fixture retires");
}

#[cfg(unix)]
#[test]
fn symlinked_dynamic_store_directory_rejects_before_execution() {
    use std::os::unix::fs::symlink;

    let root = temporary("symlink-root");
    let outside = temporary("symlink-outside");
    let context = prepare_fixture(&root);
    fs::create_dir(&outside).expect("the outside directory exists");
    let store = context.open_store().expect("the fixed store layout opens");
    symlink(&outside, store.root().join("actions/native-product"))
        .expect("the test installs a dynamic-directory symlink");
    let counts = Counts::default();
    let executions = AtomicUsize::new(0);
    let recipe = context
        .direct::<Artifact>(b"rawr.phase9.native-product.synthetic/v1\0symlink", 4_096)
        .expect("the direct recipe seals");
    assert!(matches!(
        Evaluator::new(&store, &counts).direct(&recipe, || {
            executions.fetch_add(1, Ordering::Relaxed);
            Ok(Artifact(b"forbidden".to_vec().into_boxed_slice()))
        }),
        Err(Error::Shape(_))
    ));
    assert_eq!(executions.load(Ordering::Relaxed), 0);
    assert_eq!(counts.rejected.load(Ordering::Relaxed), 1);
    fs::remove_dir_all(root).expect("the symlink store fixture retires");
    fs::remove_dir_all(outside).expect("the outside fixture retires");
}

fn run_worker(root: &Path, report: &Path) {
    let output = worker_command(root, report)
        .output()
        .expect("the fresh DAG worker launches");
    assert_worker_success(output);
}

fn worker_command(root: &Path, report: &Path) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("the test executable resolves"));
    command
        .arg("--ignored")
        .arg("--exact")
        .arg(WORKER)
        .arg("--nocapture")
        .env(ROOT_ENV, root)
        .env(REPORT_ENV, report)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn assert_worker_success(output: std::process::Output) {
    assert!(
        output.status.success(),
        "fresh DAG worker failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn parse_report(path: &Path) -> BTreeMap<Box<str>, usize> {
    let report = fs::read_to_string(path).expect("the fresh worker report exists");
    report
        .lines()
        .map(|line| {
            let (key, value) = line
                .split_once('=')
                .expect("report rows are key/value pairs");
            (
                key.to_owned().into_boxed_str(),
                value.parse().expect("report counts are decimal integers"),
            )
        })
        .collect()
}

#[derive(Debug, Eq, PartialEq)]
struct Snapshot {
    directory: bool,
    bytes: u64,
    digest: [u8; 32],
    modified: Option<Duration>,
}

/// The project's own files: its Cargo target directory, which holds the
/// project's store, is left out.
fn project_snapshot(root: &Path) -> BTreeMap<PathBuf, Snapshot> {
    let mut records = snapshot(root);
    records.retain(|relative, _| !relative.starts_with("target"));
    records
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Snapshot> {
    let mut records = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for child in fs::read_dir(&directory).expect("the store tree is readable") {
            let child = child.expect("the store child is readable");
            let path = child.path();
            let metadata = fs::symlink_metadata(&path).expect("the store child has metadata");
            assert!(!metadata.file_type().is_symlink());
            let relative = path
                .strip_prefix(root)
                .expect("the store child remains beneath its root")
                .to_path_buf();
            let directory = metadata.is_dir();
            let (bytes, digest) = if metadata.is_file() {
                let contents = fs::read(&path).expect("the store file is readable");
                (metadata.len(), Sha256::digest(contents).into())
            } else {
                (0, [0; 32])
            };
            let modified = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok());
            records.insert(
                relative,
                Snapshot {
                    directory,
                    bytes,
                    digest,
                    modified,
                },
            );
            if directory {
                pending.push(path);
            }
        }
    }
    records
}

fn find_file(root: &Path, name: &str) -> PathBuf {
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for child in fs::read_dir(&directory).expect("the store tree is readable") {
            let path = child.expect("the store child is readable").path();
            let metadata = fs::symlink_metadata(&path).expect("the store child has metadata");
            if metadata.is_dir() {
                pending.push(path);
            } else if path.file_name().is_some_and(|file| file == name) {
                return path;
            }
        }
    }
    panic!("the store file `{name}` was not found")
}

fn temporary(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "rawr-dag-{label}-{}-{}",
        std::process::id(),
        TEST_ID.fetch_add(1, Ordering::Relaxed)
    ))
}

fn prepare_fixture(root: &Path) -> Context {
    fs::create_dir(root).expect("the context fixture root exists");
    fs::create_dir(root.join("project")).expect("the context project exists");
    fs::create_dir(root.join("cargo-home")).expect("the context Cargo home exists");
    fixture_context(root)
}

fn fixture_context(root: &Path) -> Context {
    let cargo_home =
        fs::canonicalize(root.join("cargo-home")).expect("the context Cargo home canonicalizes");
    let environment = cargo::config::Environment::from_pairs([(
        "CARGO_HOME",
        cargo_home
            .to_str()
            .expect("the context fixture path is Unicode"),
    )])
    .expect("the context environment seals");
    Context::capture(
        &root.join("project"),
        environment,
        "test-target",
        &cargo::Limits::standard(),
    )
    .expect("the context seals")
}

#[test]
fn replay_tells_ordinary_input_changes_from_damaged_entries() {
    let root = temporary("replay-damage");
    let context = prepare_fixture(&root);
    let store = context.open_store().unwrap();
    let recipe = context
        .discover::<Artifact>(b"replay-damage", 4_096)
        .unwrap();
    let counts = Counts::default();
    let value = Evaluator::new(&store, &counts)
        .discover(
            &recipe,
            |value| value.encode(4_096),
            || Ok(Artifact(Box::from(b"bounded".as_slice()))),
        )
        .unwrap();
    let action = value.receipt().action();
    let fresh = fixture_context(&root).open_store().unwrap();
    let evaluator = Evaluator::new(&fresh, &counts);
    // An input change rejects the candidate quietly: no damage variant.
    let changed: super::read_set::ScopedValidator<Artifact> =
        Arc::new(|_, _, _| Err(Error::Codec("fixture input changed")));
    assert!(matches!(
        evaluator.replay_discovered(&recipe, &changed, action),
        Err(Error::Codec("fixture input changed"))
    ));
    // Damaged stored bytes are reported as damage.
    fs::write(
        find_file(&store.root().join("content"), "payload"),
        b"damaged",
    )
    .unwrap();
    let current: super::read_set::ScopedValidator<Artifact> =
        Arc::new(|value, _, _| value.encode(4_096));
    assert!(matches!(
        evaluator.replay_discovered(&recipe, &current, action),
        Err(Error::Corrupt { .. })
    ));
    fs::remove_dir_all(root).unwrap();
}

#[cfg(feature = "test-fixtures")]
fn store_growth_runners(
    label: &str,
    names: &[&'static [u8]],
) -> (
    PathBuf,
    crate::store::Store,
    Vec<(PathBuf, super::Validated<BundleValue>)>,
) {
    let root = temporary(label);
    let context = prepare_fixture(&root);
    let store = context.open_store().expect("the store-growth store opens");
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let runners = names
        .iter()
        .map(|name| {
            let recipe = context
                .discover::<BundleValue>(name, 4_096)
                .expect("the runner recipe seals");
            let current = store.clone();
            let runner = evaluator
                .discover_bundled(
                    &recipe,
                    move |candidate| candidate.validate_published(&current),
                    bundled_fixture,
                )
                .expect("the runner publishes");
            let action = store
                .root()
                .join("actions/runner")
                .join(super::STORAGE_DIRECTORY)
                .join(runner.receipt().action().directory());
            (action, runner)
        })
        .collect();
    (root, store, runners)
}

#[cfg(feature = "test-fixtures")]
fn store_growth_pin(
    store: &crate::store::Store,
    runner: &super::Validated<BundleValue>,
    role: crate::store::PinRole,
) {
    let counts = Counts::default();
    let proof = Evaluator::new(store, &counts)
        .seal_read_set([runner.receipt().read_set().clone()])
        .expect("the runner closure seals");
    store
        .with_pin(role, &proof, || Ok(()))
        .expect("the root binds and becomes stale");
}

#[cfg(feature = "test-fixtures")]
#[test]
fn store_growth_target_eviction_keeps_live_and_last_good() {
    let (root, store, runners) = store_growth_runners(
        "store-growth-keep",
        &[
            b"store-growth-old",
            b"store-growth-last-good",
            b"store-growth-live",
        ],
    );
    store_growth_pin(&store, &runners[1].1, crate::store::PinRole::LastGood);
    store_growth_pin(&store, &runners[2].1, crate::store::PinRole::Live);

    store
        .fixture_make_room()
        .expect("eviction toward the target never refuses");
    assert!(!runners[0].0.exists(), "the unpinned runner evicts");
    assert!(runners[1].0.is_dir() && runners[2].0.is_dir());
    assert_eq!(
        crate::store::bundle::count_bundles(store.root()).unwrap(),
        2
    );

    store.retire_fixture().expect("the store retires");
    fs::remove_dir_all(root).expect("the fixture retires");
}

#[cfg(feature = "test-fixtures")]
#[test]
fn store_growth_clean_frees_unpinned_and_keeps_running_pins() {
    let (root, store, runners) = store_growth_runners(
        "store-growth-clean",
        &[
            b"store-growth-running",
            b"store-growth-stale-live",
            b"store-growth-unpinned",
        ],
    );
    let counts = Counts::default();
    let proof = Evaluator::new(&store, &counts)
        .seal_read_set([runners[0].1.receipt().read_set().clone()])
        .expect("the running closure seals");
    let guard = store
        .acquire_pin(crate::store::PinRole::Live, &proof)
        .expect("a running process pins its runner");
    store_growth_pin(&store, &runners[1].1, crate::store::PinRole::Live);

    assert!(store.evict_all().expect("clean evicts") > 0);
    assert!(runners[0].0.is_dir(), "a running process keeps its runner");
    assert!(!runners[1].0.exists() && !runners[2].0.exists());
    assert_eq!(
        crate::store::bundle::count_bundles(store.root()).unwrap(),
        1
    );

    drop(guard);
    store.retire_fixture().expect("the store retires");
    fs::remove_dir_all(root).expect("the fixture retires");
}

/// The cache check rebuilds a stale or damaged entry, and only a failure
/// that stops the build leaves it as an error (#522).
#[test]
fn cache_check_separates_entries_to_rebuild_from_failures() {
    use super::Check;
    let check = |error: Error| Check::<()>::of(Err(error));
    assert!(matches!(Check::of(Ok(Some(7))), Ok(Check::Hit(7))));
    assert!(matches!(Check::<()>::of(Ok(None)), Ok(Check::Stale)));
    for stale in [
        crate::Error::Input {
            code: "input-changed",
            reason: "/project/src/main.rs".into(),
        },
        crate::Error::Input {
            code: "protected-build-package-owner-missing",
            reason: "crunchy 0.2.4".into(),
        },
        crate::Error::Schema {
            document: "resolution universe receipt",
            reason: "missing field `lock`".into(),
        },
    ] {
        assert!(matches!(check(stale.into()), Ok(Check::Stale)));
    }
    assert!(matches!(
        check(Error::Corrupt {
            path: "entry".into(),
            reason: "envelope decoding failed",
        }),
        Ok(Check::Damaged(None))
    ));
    assert!(matches!(check(Error::Allocation), Err(Error::Allocation)));
}

/// What a stored entry may legitimately fail with is a rawr bug in a fresh
/// build, which panics naming the invariant (#522, D664).
#[test]
#[should_panic(expected = "rawr reads Cargo unit graph as it was written: missing field `units`")]
fn fresh_build_panics_when_rawr_misreads_cargo() {
    let _ = Error::from(crate::Error::Schema {
        document: "Cargo unit graph",
        reason: "missing field `units`".into(),
    })
    .fresh();
}

#[test]
#[should_panic(
    expected = "rawr keeps track of each build input (`protected-build-package-owner-missing`)"
)]
fn fresh_build_panics_when_rawr_loses_a_build_input() {
    let _ = Error::from(crate::Error::Input {
        code: "protected-build-package-owner-missing",
        reason: "crunchy 0.2.4".into(),
    })
    .fresh();
}

/// A fresh build's failure that the program's files can cause stays an
/// error in Cargo's words.
#[test]
fn fresh_build_keeps_failures_the_program_causes() {
    let error = Error::from(crate::Error::Input {
        code: "input-changed-during-read",
        reason: "/project/src/main.rs".into(),
    })
    .fresh();
    assert_eq!(
        error.to_string(),
        "`/project/src/main.rs` changed during the build; run it again"
    );
}

/// A store whose executions were deleted rebuilds them: the lost entries
/// are forgotten, never an error on every later check (#584).
#[cfg(feature = "test-fixtures")]
#[test]
fn store_missing_its_executions_rebuilds_them() {
    let (root, store, runners) =
        store_growth_runners("store-lost-executions", &[b"store-lost-executions"]);
    drop(runners);
    fs::remove_dir_all(store.root().join("executions")).unwrap();
    drop(store);
    // The next run opens the store again.
    let context = fixture_context(&root);
    let store = context.open_store().expect("a damaged store opens empty");
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let rebuild = |name: &'static [u8]| {
        let recipe = context.discover::<BundleValue>(name, 4_096).unwrap();
        let current = store.clone();
        evaluator.discover_bundled(
            &recipe,
            move |candidate| candidate.validate_published(&current),
            bundled_fixture,
        )
    };
    rebuild(b"store-lost-executions").expect("the lost entry is rebuilt");
    rebuild(b"store-lost-executions-other").expect("an unrelated entry publishes");
    store
        .fixture_physical_inventory()
        .expect("the rebuilt store's inventory is whole");
    store.retire_fixture().expect("the store retires");
    fs::remove_dir_all(root).expect("the fixture retires");
}

/// A store missing one execution inside its executions directory rebuilds
/// it: the lost entries and the pins that hold them are forgotten, never an
/// error on every later check (#612).
#[cfg(feature = "test-fixtures")]
#[test]
fn store_missing_one_execution_rebuilds_it() {
    let (root, store, runners) = store_growth_runners(
        "store-lost-execution",
        &[b"store-lost-execution", b"store-kept-execution"],
    );
    store_growth_pin(&store, &runners[0].1, crate::store::PinRole::Live);
    store_growth_pin(&store, &runners[1].1, crate::store::PinRole::LastGood);
    drop(runners);
    let context = fixture_context(&root);
    let execution = context
        .discover::<BundleValue>(b"store-lost-execution", 4_096)
        .unwrap()
        .execution();
    let lost = store.root().join("executions").join(execution.directory());
    fs::remove_dir_all(&lost).expect("the runner's execution is removed");
    drop(store);
    // The next run opens the store again.
    let store = context
        .open_store()
        .expect("a store missing an execution opens");
    let counts = Counts::default();
    let evaluator = Evaluator::new(&store, &counts);
    let rebuild = |name: &'static [u8]| {
        let recipe = context.discover::<BundleValue>(name, 4_096).unwrap();
        let current = store.clone();
        evaluator.discover_bundled(
            &recipe,
            move |candidate| candidate.validate_published(&current),
            bundled_fixture,
        )
    };
    rebuild(b"store-lost-execution").expect("the lost entry is rebuilt");
    rebuild(b"store-kept-execution").expect("the kept entry is found");
    // Making room walks the store: the lost entry and its pin are forgotten.
    store
        .fixture_make_room()
        .expect("a store missing an execution makes room");
    rebuild(b"store-lost-execution").expect("the lost entry is found or rebuilt");
    rebuild(b"store-lost-execution-other").expect("an unrelated entry publishes");
    store
        .fixture_physical_inventory()
        .expect("the rebuilt store's inventory is whole");
    store.retire_fixture().expect("the store retires");
    fs::remove_dir_all(root).expect("the fixture retires");
}
