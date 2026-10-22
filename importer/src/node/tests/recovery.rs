use super::*;
use std::os::unix::fs::PermissionsExt as _;

const COMPLETE: &[u8] = b"stable owner completion";

fn stable(
    attempt: crate::store::bundle::Attempt,
) -> Result<super::super::Bundled<BundleValue>, Error> {
    fs::write(attempt.generated().join("input"), b"generated")
        .map_err(|error| Error::Transform(error.to_string().into()))?;
    let value = BundleValue::capture(attempt.reference());
    let pending = attempt.seal(Box::from(BundleValue::SEAL))?;
    super::super::Bundled::validate(value, pending, |value, pending| {
        value.validate_pending(pending)?;
        Ok(Box::from(COMPLETE))
    })
}

fn validate(value: &BundleValue, store: &crate::store::Store) -> Result<Box<[u8]>, Error> {
    value.validate_published(store)?;
    Ok(Box::from(COMPLETE))
}

#[test]
fn same_action_equal_size_frozen_corruption_retires_once_then_reuses() {
    let root = temporary("same-action-corruption");
    let context = prepare_fixture(&root);
    let store = context.open_store().unwrap();
    let recipe = context
        .discover::<BundleValue>(b"same-action", 4096)
        .unwrap();
    let current = store.clone();
    let first = Evaluator::new(&store, &Counts::default())
        .discover_bundled(&recipe, move |value| validate(value, &current), stable)
        .unwrap();
    let old_root = store.root().join(first.value().reference.relative());
    let seal = old_root.join("seal");
    let frozen = fs::metadata(&seal).unwrap().permissions();
    fs::set_permissions(&seal, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&seal, vec![0x57; BundleValue::SEAL.len()]).unwrap();
    fs::set_permissions(&seal, frozen).unwrap();
    let fresh = fixture_context(&root).open_store().unwrap();
    let current = fresh.clone();
    let counts = Counts::default();
    let repaired = Evaluator::new(&fresh, &counts)
        .discover_bundled(&recipe, move |value| validate(value, &current), stable)
        .unwrap();
    assert_eq!(first.receipt().action(), repaired.receipt().action());
    assert_ne!(first.content().digest(), repaired.content().digest());
    assert!(!old_root.exists());
    assert_eq!(counts.miss.load(Ordering::Relaxed), 1);
    assert_eq!(
        crate::store::bundle::count_bundles(store.root()).unwrap(),
        1
    );
    let warm = fixture_context(&root).open_store().unwrap();
    let current = warm.clone();
    let hit = Evaluator::new(&warm, &Counts::default())
        .discover_bundled(
            &recipe,
            move |value| validate(value, &current),
            |_| panic!("warm repaired owner must not launch a child"),
        )
        .unwrap();
    assert_eq!(hit.content().digest(), repaired.content().digest());
    store.retire_fixture().unwrap();
    fs::remove_dir_all(root).unwrap();
}

/// The hashed-byte account is an accounting target. Validating the
/// commitment on an account already at its target records the allocation
/// hash as the first exceeding charge and keeps going, so the oversized seal
/// is classified as candidate-owned corruption (positive classification) and
/// the bundle is repaired and then reused.
#[test]
fn oversized_frozen_seal_records_hash_exceedance_then_classifies_corruption_and_repairs() {
    let root = temporary("oversized-seal-corruption");
    let context = prepare_fixture(&root);
    let store = context.open_store().unwrap();
    let recipe = context
        .discover::<BundleValue>(b"oversized-seal", 4096)
        .unwrap();
    let current = store.clone();
    let first = Evaluator::new(&store, &Counts::default())
        .discover_bundled(&recipe, move |value| validate(value, &current), stable)
        .unwrap();
    let old_root = store.root().join(first.value().reference.relative());
    let seal = old_root.join("seal");
    let frozen = fs::metadata(&seal).unwrap().permissions();
    fs::set_permissions(&seal, fs::Permissions::from_mode(0o600)).unwrap();
    let oversized = super::super::MAX_CONTENT_BYTES as u64 + 1;
    fs::OpenOptions::new()
        .write(true)
        .open(&seal)
        .unwrap()
        .set_len(oversized)
        .unwrap();
    fs::set_permissions(&seal, frozen).unwrap();

    let refused = super::super::GraphBudget::new();
    refused
        .hash(super::super::MAX_HASHED_BYTES_PER_GRAPH as usize)
        .unwrap();
    let allocation_bytes = fs::metadata(old_root.join("allocation")).unwrap().len();
    let commitment = crate::store::bundle::BundleCommitment::new(
        first.value().reference,
        first.value().seal_length,
        first.value().seal_digest,
    );
    let result = crate::store::bundle::validate_consumer_commitment(
        store.root(),
        &commitment,
        BundleValue::KIND,
        BundleValue::SCHEMA,
        first.content().digest(),
        &refused,
    );
    assert!(
        matches!(&result, Err(Error::CandidateCorrupt(corruption))
            if format!("{corruption:?}").contains("fact: Seal")),
        "the oversized seal is candidate-owned corruption, never an accounting refusal: {result:?}"
    );
    let exceeded = refused.exceedances();
    let hashed = exceeded
        .axis(super::super::HASHED_RESOURCE)
        .expect("the allocation hash crosses the hashed-byte target");
    assert_eq!(hashed.target, super::super::MAX_HASHED_BYTES_PER_GRAPH);
    assert_eq!(
        hashed.first_requested,
        super::super::MAX_HASHED_BYTES_PER_GRAPH + allocation_bytes
    );
    assert!(hashed.high_water >= hashed.first_requested);
    assert_eq!(fs::metadata(&seal).unwrap().len(), oversized);

    let fresh = fixture_context(&root).open_store().unwrap();
    let current = fresh.clone();
    let counts = Counts::default();
    let repaired = Evaluator::new(&fresh, &counts)
        .discover_bundled(&recipe, move |value| validate(value, &current), stable)
        .unwrap();
    assert_eq!(first.receipt().action(), repaired.receipt().action());
    assert!(!old_root.exists());
    assert_eq!(counts.miss.load(Ordering::Relaxed), 1);
    let warm = fixture_context(&root).open_store().unwrap();
    let current = warm.clone();
    Evaluator::new(&warm, &Counts::default())
        .discover_bundled(
            &recipe,
            move |value| validate(value, &current),
            |_| panic!("repaired oversized seal must reuse its replacement"),
        )
        .unwrap();
    store.retire_fixture().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unknown_operational_refusal_and_external_drift_never_nominate_retirement() {
    for case in 0..7 {
        let root = temporary("nonnominating-rejection");
        let context = prepare_fixture(&root);
        let store = context.open_store().unwrap();
        let recipe = context
            .discover::<BundleValue>(b"nonnominating", 4096)
            .unwrap();
        let current = store.clone();
        let first = Evaluator::new(&store, &Counts::default())
            .discover_bundled(&recipe, move |value| validate(value, &current), stable)
            .unwrap();
        let old_root = store.root().join(first.value().reference.relative());
        let before = snapshot(&old_root);
        let fresh = fixture_context(&root).open_store().unwrap();
        let source = root.join("external");
        let result = Evaluator::new(&fresh, &Counts::default()).discover_bundled(
            &recipe,
            move |_| match case {
                0 => Err(Error::Limit {
                    resource: "fixture refusal",
                    limit: 0,
                    actual: 1,
                }),
                1 => Err(Error::Allocation),
                2 => Err(Error::State("fixture poisoned state")),
                3 => Err(Error::Io {
                    operation: "fixture permission failure",
                    path: source.clone(),
                    source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
                }),
                4 => Err(Error::Codec(
                    "generic format failure lacks candidate provenance",
                )),
                5 => Err(Error::Importer(Box::new(crate::Error::InputAccountRefused))),
                _ => Ok(Box::from(b"changed external completion".as_slice())),
            },
            |_| Err(Error::State("fixture stops replacement execution")),
        );
        assert!(result.is_err());
        assert_eq!(snapshot(&old_root), before, "case {case}");
        assert_eq!(
            crate::store::bundle::count_bundles(store.root()).unwrap(),
            1
        );
        store.retire_fixture().unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}

fn inventory_owner(
    attempt: crate::store::bundle::Attempt,
    graph: &super::super::GraphBudget,
) -> Result<super::super::Bundled<BundleValue>, Error> {
    use crate::runner::artifact::retained;
    fs::write(attempt.generated().join("input"), b"generated")
        .map_err(|error| Error::Transform(error.to_string().into()))?;
    let inventory = retained::Stored::capture(
        &attempt.root(),
        attempt.reference(),
        retained::Mode::Pending,
        graph,
    )?;
    let value = BundleValue {
        reference: attempt.reference(),
        seal_length: inventory.bytes().len(),
        seal_digest: inventory.digest(),
    };
    let pending = attempt.seal(inventory.into_bytes())?;
    super::super::Bundled::validate(value, pending, |value, pending| {
        value.validate_pending(pending)?;
        Ok(Box::from(COMPLETE))
    })
}

fn validate_inventory(
    value: &BundleValue,
    store: &crate::store::Store,
) -> Result<Box<[u8]>, Error> {
    use crate::runner::artifact::retained;
    value.validate_published(store)?;
    let root = store.root().join(value.reference.relative());
    let bytes = crate::store::read_file_budgeted(
        &root.join("seal"),
        super::super::MAX_CONTENT_BYTES,
        store.graph_budget(),
    )?;
    let inventory = retained::Stored::decode(&bytes, &root, value.reference, store.graph_budget())?;
    inventory.replay_published(
        &root,
        store,
        BundleValue::KIND,
        BundleValue::SCHEMA,
        super::super::content_digest::<BundleValue>(&value.encode(4096)?),
        store.graph_budget(),
    )?;
    Ok(Box::from(COMPLETE))
}

#[test]
fn same_size_frozen_owned_member_digest_corruption_repairs_same_action() {
    let root = temporary("same-action-member-corruption");
    let context = prepare_fixture(&root);
    let store = context.open_store().unwrap();
    let recipe = context
        .discover::<BundleValue>(b"owned-member", 4096)
        .unwrap();
    let current = store.clone();
    let graph = store.graph_budget().clone();
    let first = Evaluator::new(&store, &Counts::default())
        .discover_bundled(
            &recipe,
            move |value| validate_inventory(value, &current),
            move |attempt| inventory_owner(attempt, &graph),
        )
        .unwrap();
    let old_root = store.root().join(first.value().reference.relative());
    let member = old_root.join("generated/input");
    let frozen = fs::metadata(&member).unwrap().permissions();
    fs::set_permissions(&member, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&member, b"corrupted").unwrap();
    fs::set_permissions(&member, frozen).unwrap();
    let fresh = fixture_context(&root).open_store().unwrap();
    let current = fresh.clone();
    let graph = fresh.graph_budget().clone();
    let counts = Counts::default();
    let repaired = Evaluator::new(&fresh, &counts)
        .discover_bundled(
            &recipe,
            move |value| validate_inventory(value, &current),
            move |attempt| inventory_owner(attempt, &graph),
        )
        .unwrap();
    assert_eq!(first.receipt().action(), repaired.receipt().action());
    assert_ne!(first.content().digest(), repaired.content().digest());
    assert!(!old_root.exists());
    assert_eq!(counts.miss.load(Ordering::Relaxed), 1);
    let warm = fixture_context(&root).open_store().unwrap();
    let current = warm.clone();
    let reused = Evaluator::new(&warm, &Counts::default())
        .discover_bundled(
            &recipe,
            move |value| validate_inventory(value, &current),
            |_| panic!("unchanged repaired member cannot launch a child"),
        )
        .unwrap();
    assert_eq!(reused.content().digest(), repaired.content().digest());
    assert_eq!(
        crate::store::bundle::count_bundles(store.root()).unwrap(),
        1
    );
    store.retire_fixture().unwrap();
    fs::remove_dir_all(root).unwrap();
}
