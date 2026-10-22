use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;
use crate::node::{Evaluator, Receipt, Validated};
use crate::store::bundle::{BundleCommitment, BundleId, BundleRef};
use crate::store::Store;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Root(PathBuf);
impl Root {
    fn prepare() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rawr-retention-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn open(&self) -> Store {
        Store::open(&self.0, [7; 32]).unwrap()
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Owned;
impl Value for Owned {
    const KIND: Kind = Kind::Api;
    const SCHEMA: u32 = 1;
    fn encode(&self, _: usize) -> Result<Box<[u8]>, Error> {
        Ok(Box::from(&b"owned"[..]))
    }
    fn decode(bytes: &[u8], _: usize) -> Result<Self, Error> {
        if bytes == b"owned" {
            Ok(Self)
        } else {
            Err(Error::Codec("fixture bytes changed"))
        }
    }
}

struct NeverCurrent;
impl Current for NeverCurrent {
    fn revalidate(&self, _: &super::super::GraphBudget, _: Scope) -> Result<Entry, Error> {
        panic!("retention must never invoke semantic validators")
    }
}

fn current(store: &Store, inputs: &[u8]) -> Validated<Owned> {
    let recipe = Direct::capture([1; 32], inputs, 64, &[], 0).unwrap();
    let current = Evaluator::new(store, &crate::metrics::Current)
        .direct(&recipe, || Ok(Owned))
        .unwrap();
    let mut fragment = current.receipt().read_set().clone();
    for witness in &mut fragment.witnesses {
        witness.current = Arc::new(NeverCurrent);
    }
    let receipt = Receipt::new(
        current.receipt().action(),
        current.receipt().recipe_digest(),
        current.content(),
        fragment,
    );
    Validated::new(Arc::new(Owned), current.content(), receipt)
}

fn commitment() -> BundleCommitment {
    BundleCommitment::new(
        BundleRef::new(
            super::super::ExecutionKey::from_bytes([4; 32]),
            BundleId::from_bytes([5; 32]),
        ),
        0,
        [6; 32],
    )
}

fn bundled(current: &Validated<Owned>, ancestor: bool) -> Validated<Owned> {
    let mut fragment = current.receipt().read_set().clone();
    let mut entry = fragment.entries[0];
    entry.bundled = true;
    if ancestor {
        entry.action = ActionKey::from_bytes([9; 32]);
    }
    let mut entries = fragment.entries.into_vec();
    if ancestor {
        entries.push(entry);
    } else {
        entries[0] = entry;
    }
    entries.sort_unstable();
    fragment.digest = digest(&entries);
    fragment.entries = entries.into_boxed_slice();
    let receipt = Receipt::new(
        current.receipt().action(),
        current.receipt().recipe_digest(),
        current.content(),
        fragment,
    );
    Validated::new(Arc::new(Owned), current.content(), receipt)
}

#[test]
fn retention_preserves_exact_closure_without_semantic_calls() {
    let root = Root::prepare();
    let store = root.open();
    let current = current(&store, b"one");
    let retention = Retention::from_validated(&current, None).unwrap();
    assert!(retention.entries() == current.receipt().read_set().entries.as_ref());
    assert_eq!(retention.digest(), current.receipt().read_set().digest());
    retention
        .require_parents(&[Parent::validated(&current)])
        .unwrap();
    retention.validate_storage(&store).unwrap();
}

#[test]
fn retention_rejects_foreign_graph_and_unrelated_parent() {
    let root = Root::prepare();
    let store = root.open();
    let first = current(&store, b"one");
    let retention = Retention::from_validated(&first, None).unwrap();
    let foreign = root.open();
    assert!(matches!(
        retention.validate_storage(&foreign),
        Err(Error::State("retention belongs to another graph"))
    ));
    let other = current(&store, b"two");
    assert!(retention
        .require_parents(&[Parent::validated(&other)])
        .is_err());
    assert!(retention.require_parents(&[] as &[Parent]).is_err());
    assert!(retention
        .require_parents(&[Parent::validated(&first), Parent::validated(&other)])
        .is_err());
}

#[test]
fn retention_requires_exact_root_bundle_coverage() {
    let root = Root::prepare();
    let store = root.open();
    let current = current(&store, b"one");
    assert!(matches!(
        Retention::from_validated(&current, Some(commitment())),
        Err(Error::State("retention has an extra bundle commitment"))
    ));
    assert!(matches!(
        Retention::from_validated(&bundled(&current, false), None),
        Err(Error::State("retention bundled root has no commitment"))
    ));
    assert!(matches!(
        Retention::from_validated(&bundled(&current, true), Some(commitment())),
        Err(Error::State("retention bundled ancestor has no commitment"))
    ));
}

#[test]
fn retention_refuses_foreign_bundle_storage() {
    let root = Root::prepare();
    let store = root.open();
    let current = current(&store, b"one");
    let retention =
        Retention::from_validated(&bundled(&current, false), Some(commitment())).unwrap();
    assert!(retention.validate_storage(&store).is_err());
}

#[test]
fn retention_refuses_changed_stored_output() {
    let root = Root::prepare();
    let store = root.open();
    let current = current(&store, b"one");
    let retention = Retention::from_validated(&current, None).unwrap();
    let payload = find_payload(store.root());
    fs::write(payload, b"other").unwrap();
    assert!(retention.validate_storage(&store).is_err());
}

#[test]
fn retention_copies_spend_the_original_graph_account() {
    let root = Root::prepare();
    let store = root.open();
    let current = current(&store, b"one");
    let graph = store.graph_budget();
    let spent = graph.usage.lock().unwrap().decoded;
    let target = super::super::MAX_DECODED_BYTES_PER_GRAPH;
    graph.decode((target - spent) as usize).unwrap();
    assert!(!graph.exceedances().is_exceeded());
    // The copy still spends the original account past its target and the
    // account records exactly the copied entry array as the exceedance.
    let retained = Retention::from_validated(&current, None)
        .expect("a retention copy past the accounting target is still owned");
    assert!(retained.graph.shares_with(graph));
    let bytes = (current.receipt().read_set().len() * std::mem::size_of::<Entry>()) as u64;
    let decoded = graph
        .exceedances()
        .axis(super::super::DECODED_RESOURCE)
        .copied()
        .expect("the copy charges the decoded-byte account");
    assert_eq!(decoded.target, target);
    assert_eq!(decoded.first_requested, target + bytes);
    assert_eq!(decoded.high_water, target + bytes);
    assert_eq!(graph.usage.lock().unwrap().decoded, decoded.high_water);
}

fn find_payload(root: &std::path::Path) -> PathBuf {
    fn visit(root: &std::path::Path) -> Option<PathBuf> {
        for entry in fs::read_dir(root).unwrap() {
            let entry = entry.unwrap();
            if entry.file_name() == "payload" {
                return Some(entry.path());
            }
            if entry.file_type().unwrap().is_dir()
                && let Some(path) = visit(&entry.path())
            {
                return Some(path);
            }
        }
        None
    }
    visit(root).expect("published payload exists")
}

// Real discoverable provenance: the current recipe depends on bounded file
// observations, while the owned semantic output remains unchanged.
fn observed_parent(store: &Store, source: PathBuf, checks: Arc<AtomicU64>) -> Validated<Owned> {
    let recipe = Discoverable::capture([1; 32], b"observed-parent", 64, &[], 0).unwrap();
    Evaluator::new(store, &crate::metrics::Current)
        .discover_budgeted(
            &recipe,
            move |_: &Owned, graph| {
                checks.fetch_add(1, Ordering::Relaxed);
                let bytes = crate::store::read_file_budgeted(&source, 64, graph)?;
                graph.hash(bytes.len())?;
                Ok(bytes)
            },
            || Ok(Owned),
        )
        .unwrap()
}

#[test]
fn staged_direct_mutation_during_pure_miss_rejects_current_promotion() {
    let root = Root::prepare();
    let store = root.open();
    let inputs = Root::prepare();
    let source = inputs.0.join("observed-input");
    fs::write(&source, b"before").unwrap();
    let checks = Arc::new(AtomicU64::new(0));
    let parent = observed_parent(&store, source.clone(), Arc::clone(&checks));
    let retention = Retention::from_validated(&parent, None).unwrap();
    let recipe = Direct::capture(
        [1; 32],
        b"staged-miss",
        64,
        &[Parent::validated(&parent)],
        1,
    )
    .unwrap();
    let before = checks.load(Ordering::Relaxed);
    let executed = AtomicU64::new(0);
    let result =
        Evaluator::new(&store, &crate::metrics::Current).staged_direct(&recipe, &retention, || {
            executed.fetch_add(1, Ordering::Relaxed);
            assert_eq!(
                checks.load(Ordering::Relaxed),
                before,
                "retention bookkeeping must not reinterpret the parent"
            );
            // Deterministically inject an external write during the pure stage.
            fs::write(&source, b"after!").unwrap();
            Ok(Owned)
        });
    assert!(
        matches!(result, Err(Error::StaleReadSet { action }) if action == parent.receipt().action())
    );
    assert_eq!(executed.load(Ordering::Relaxed), 1);
    assert!(checks.load(Ordering::Relaxed) > before);
    // Historical publication may survive, but has not become a current value.
    assert!(store
        .lookup_staged(&recipe, store.graph_budget())
        .unwrap()
        .is_some());
    store.retire_fixture().unwrap();
}

#[test]
fn staged_direct_warm_hit_skips_execute_and_transitive_drift_rejects() {
    let root = Root::prepare();
    let store = root.open();
    let inputs = Root::prepare();
    let source = inputs.0.join("observed-input");
    fs::write(&source, b"before").unwrap();
    let checks = Arc::new(AtomicU64::new(0));
    let ancestor = observed_parent(&store, source.clone(), Arc::clone(&checks));
    let middle_recipe = Direct::capture(
        [1; 32],
        b"owned-middle",
        64,
        &[Parent::validated(&ancestor)],
        1,
    )
    .unwrap();
    let evaluator = Evaluator::new(&store, &crate::metrics::Current);
    let middle = evaluator.direct(&middle_recipe, || Ok(Owned)).unwrap();
    let retention = Retention::from_validated(&middle, None).unwrap();
    assert_eq!(retention.entries().len(), 2);
    let recipe = Direct::capture(
        [1; 32],
        b"staged-child",
        64,
        &[Parent::validated(&middle)],
        1,
    )
    .unwrap();
    let executed = AtomicU64::new(0);
    let first = evaluator
        .staged_direct(&recipe, &retention, || {
            executed.fetch_add(1, Ordering::Relaxed);
            Ok(Owned)
        })
        .unwrap();
    let before_hit = checks.load(Ordering::Relaxed);
    let hit = evaluator
        .staged_direct(&recipe, &retention, || {
            panic!("unchanged staged hit must not execute")
        })
        .unwrap();
    assert_eq!(hit.content().digest(), first.content().digest());
    assert_eq!(hit.receipt().read_set().len(), 3);
    assert!(
        checks.load(Ordering::Relaxed) > before_hit,
        "a hit still checks the complete current ancestor union"
    );
    fs::write(&source, b"after!").unwrap();
    let before_rejection = checks.load(Ordering::Relaxed);
    let result = evaluator.staged_direct(&recipe, &retention, || {
        panic!("stale hit must reject promotion, not execute a fallback")
    });
    assert!(
        matches!(result, Err(Error::StaleReadSet { action }) if action == ancestor.receipt().action())
    );
    assert!(checks.load(Ordering::Relaxed) > before_rejection);
    assert_eq!(executed.load(Ordering::Relaxed), 1);
    store.retire_fixture().unwrap();
}

fn owned_current(store: &Store, inputs: &[u8], parents: &[Parent]) -> Validated<Owned> {
    let recipe = Direct::capture([1; 32], inputs, 64, parents, parents.len()).unwrap();
    Evaluator::new(store, &crate::metrics::Current)
        .direct(&recipe, || Ok(Owned))
        .unwrap()
}

#[test]
fn retention_successor_preserves_exact_ancestors_and_charges_original_account() {
    let root = Root::prepare();
    let store = root.open();
    let parent = owned_current(&store, b"successor-parent", &[]);
    let retention = Retention::from_validated(&parent, None).unwrap();
    let child = owned_current(&store, b"successor-child", &[Parent::validated(&parent)]);
    let before = store.graph_budget().usage.lock().unwrap().decoded;
    let successor = retention.extend_current(&child).unwrap();
    let expected = child.receipt().read_set().len() * std::mem::size_of::<Entry>();
    assert_eq!(
        store.graph_budget().usage.lock().unwrap().decoded - before,
        expected as u64
    );
    assert!(successor
        .entries()
        .iter()
        .filter(|entry| entry.action != child.receipt().action())
        .eq(retention.entries().iter()));
    assert!(successor.entries() == child.receipt().read_set().entries.as_ref());
    assert_eq!(successor.digest(), child.receipt().read_set().digest());
    assert!(successor.graph.shares_with(store.graph_budget()));
    assert!(successor.context == retention.context);
    successor.validate_storage(&store).unwrap();
    successor
        .require_parents(&[Parent::validated(&child)])
        .unwrap();
    assert!(
        retention.extend_current(&parent).is_err(),
        "an existing root is not a successor"
    );
    // One byte short of the successor's exact charge, the copy still
    // succeeds and the original account records a high water of exactly one
    // byte past the target, proving the successor charged exactly `expected`.
    let target = super::super::MAX_DECODED_BYTES_PER_GRAPH;
    let used = store.graph_budget().usage.lock().unwrap().decoded;
    store
        .graph_budget()
        .decode((target - used) as usize - expected + 1)
        .unwrap();
    assert!(!store.graph_budget().exceedances().is_exceeded());
    let again = retention
        .extend_current(&child)
        .expect("a successor past the accounting target is still owned");
    assert!(again.entries() == successor.entries());
    let decoded = store
        .graph_budget()
        .exceedances()
        .axis(super::super::DECODED_RESOURCE)
        .copied()
        .expect("the successor charges the decoded-byte account");
    assert_eq!(decoded.target, target);
    assert_eq!(decoded.high_water, target + 1);
    assert_eq!(
        store.graph_budget().usage.lock().unwrap().decoded,
        target + 1
    );
    store.retire_fixture().unwrap();
}

#[test]
fn retention_successor_rejects_foreign_graph_and_unrelated_ancestor_closure() {
    let root = Root::prepare();
    let store = root.open();
    let parent = owned_current(&store, b"successor-parent", &[]);
    let retention = Retention::from_validated(&parent, None).unwrap();
    let other = owned_current(&store, b"unrelated-parent", &[]);
    let unrelated = owned_current(&store, b"unrelated-child", &[Parent::validated(&other)]);
    assert!(matches!(
        retention.extend_current(&unrelated),
        Err(Error::State(
            "retention successor changed its ancestor closure"
        ))
    ));
    let foreign_store = root.open();
    let foreign_parent = owned_current(&foreign_store, b"successor-parent", &[]);
    let foreign_child = owned_current(
        &foreign_store,
        b"foreign-child",
        &[Parent::validated(&foreign_parent)],
    );
    assert!(matches!(
        retention.extend_current(&foreign_child),
        Err(Error::State("node read-set belongs to another graph"))
    ));
    store.retire_fixture().unwrap();
}

#[cfg(feature = "test-fixtures")]
#[test]
fn retention_successor_preserves_registered_ancestor_bundle_commitment() {
    let root = Root::prepare();
    let store = root.open();
    let (parent, commitment) = super::super::tests::registered_retention_parent(&store);
    let retention = Retention::from_validated(&parent, Some(commitment)).unwrap();
    retention.validate_storage(&store).unwrap();
    let child = owned_current(&store, b"bundled-successor", &[Parent::validated(&parent)]);
    assert!(matches!(
        Retention::from_validated(&child, None),
        Err(Error::State("retention bundled ancestor has no commitment"))
    ));
    let successor = retention.extend_current(&child).unwrap();
    assert!(successor.bundles == retention.bundles);
    assert!(successor.bundles.len() == 1);
    successor.validate_storage(&store).unwrap();
    let grandchild = owned_current(&store, b"bundled-grandchild", &[Parent::validated(&child)]);
    let extended = successor.extend_current(&grandchild).unwrap();
    assert!(extended.bundles == retention.bundles);
    assert_eq!(extended.entries().len(), retention.entries().len() + 2);
    extended.validate_storage(&store).unwrap();
    let sibling = owned_current(&store, b"bundled-sibling", &[Parent::validated(&parent)]);
    let branched = extended.extend_current(&sibling).unwrap();
    assert!(branched.bundles == retention.bundles);
    assert_eq!(branched.entries().len(), retention.entries().len() + 3);
    branched.validate_storage(&store).unwrap();
    store.retire_fixture().unwrap();
}

#[test]
fn retention_keeps_siblings_without_adding_recipe_edges() {
    let root = Root::prepare();
    let store = root.open();
    let evaluator = Evaluator::new(&store, &crate::metrics::Current);
    let seed_recipe = Direct::<Owned>::capture([1; 32], b"branch-root", 64, &[], 0).unwrap();
    let seed = evaluator.direct(&seed_recipe, || Ok(Owned)).unwrap();
    let parent = Parent::validated(&seed);
    let left_recipe =
        Direct::<Owned>::capture([1; 32], b"left", 64, std::slice::from_ref(&parent), 1).unwrap();
    let right_recipe =
        Direct::<Owned>::capture([1; 32], b"right", 64, std::slice::from_ref(&parent), 1).unwrap();
    let left = evaluator.direct(&left_recipe, || Ok(Owned)).unwrap();
    let right = evaluator.direct(&right_recipe, || Ok(Owned)).unwrap();
    let retained = Retention::from_validated(&seed, None).unwrap();
    let retained = retained.extend_current(&left).unwrap();
    retained
        .covers_parents(std::slice::from_ref(&parent))
        .unwrap();
    let retained = retained.extend_current(&right).unwrap();
    retained
        .covers_parents(&[Parent::validated(&left), Parent::validated(&right)])
        .unwrap();
    assert_eq!(retained.entries().len(), 3);
    assert_eq!(left.receipt().read_set().entries.len(), 2);
    assert_eq!(right.receipt().read_set().entries.len(), 2);
    assert!(right
        .receipt()
        .read_set()
        .entries
        .iter()
        .all(|entry| entry.action != left.receipt().action()));
    assert!(
        retained.extend_current(&right).is_err(),
        "duplicate roots are not successors"
    );
    retained.validate_storage(&store).unwrap();
}

#[test]
fn retention_branch_rejects_unowned_or_changed_ancestor() {
    let root = Root::prepare();
    let store = root.open();
    let first = current(&store, b"retained-root");
    let other = current(&store, b"unowned-root");
    let retained = Retention::from_validated(&first, None).unwrap();
    let mut fragment = other.receipt().read_set().clone();
    fragment.entries =
        vec![first.receipt().read_set().entries[0], fragment.entries[0]].into_boxed_slice();
    fragment.entries.sort_unstable();
    let action = other.receipt().action();
    let mut ancestor = first.receipt().read_set().entries[0];
    ancestor.content = Digest::from_bytes([91; 32]);
    let index = fragment
        .entries
        .iter()
        .position(|entry| entry.action == ancestor.action)
        .unwrap();
    fragment.entries[index] = ancestor;
    fragment.entries.sort_unstable();
    assert!(retained.extend_fragment(action, &fragment).is_err());
    assert_eq!(retained.entries().len(), 1);
}

// Construct closure identities directly so these bookkeeping tests cannot call
// semantic validators or accidentally acquire current authority.
fn bundled_successor(store: &Store, inputs: &[u8], parent: &Validated<Owned>) -> Validated<Owned> {
    let child = current(store, inputs);
    let mut fragment = child.receipt().read_set().clone();
    let mut entry = fragment.entries[0];
    entry.bundled = true;
    let mut entries = parent.receipt().read_set().entries.to_vec();
    entries.push(entry);
    entries.sort_unstable();
    fragment.digest = digest(&entries);
    fragment.entries = entries.into_boxed_slice();
    let receipt = Receipt::new(
        child.receipt().action(),
        child.receipt().recipe_digest(),
        child.content(),
        fragment,
    );
    Validated::new(Arc::new(Owned), child.content(), receipt)
}

#[test]
fn retention_multiple_bundles_preserve_exact_bindings_and_charge_every_owned_copy() {
    let root = Root::prepare();
    let store = root.open();
    let parent = bundled(&current(&store, b"first-bundle"), false);
    let first_commitment = commitment();
    let second_commitment = BundleCommitment::new(
        BundleRef::new(
            super::super::ExecutionKey::from_bytes([14; 32]),
            BundleId::from_bytes([15; 32]),
        ),
        12,
        [16; 32],
    );
    let retained = Retention::from_validated(&parent, Some(first_commitment)).unwrap();
    let child = bundled_successor(&store, b"second-bundle", &parent);
    let before = store.graph_budget().usage.lock().unwrap().decoded;
    let extended = retained
        .extend_bundled_current(&child, second_commitment)
        .unwrap();
    let bytes =
        2 * std::mem::size_of::<Entry>() + 2 * std::mem::size_of::<(Entry, BundleCommitment)>();
    assert_eq!(
        store.graph_budget().usage.lock().unwrap().decoded - before,
        bytes as u64
    );
    assert_eq!(extended.bundles.len(), 2);
    assert!(extended
        .bundles
        .iter()
        .any(|(entry, bound)| entry.action == parent.receipt().action()
            && *bound == first_commitment));
    assert!(extended
        .bundles
        .iter()
        .any(|(entry, bound)| entry.action == child.receipt().action()
            && *bound == second_commitment));
    assert_eq!(extended.digest(), child.receipt().read_set().digest());
    extended.require_bindings().unwrap();
    let before = store.graph_budget().usage.lock().unwrap().decoded;
    let copied = extended.try_clone().unwrap();
    assert!(copied.bundles == extended.bundles);
    assert_eq!(
        store.graph_budget().usage.lock().unwrap().decoded - before,
        bytes as u64
    );
    assert!(copied.graph.shares_with(store.graph_budget()));
    assert!(matches!(
        Retention::from_validated(&child, Some(second_commitment)),
        Err(Error::State("retention bundled ancestor has no commitment"))
    ));
    assert!(extended
        .extend_bundled_current(&child, second_commitment)
        .is_err());
    let pure = current(&store, b"pure-after-two-bundles");
    let mut fragment = pure.receipt().read_set().clone();
    fragment.entries = extended
        .entries
        .iter()
        .copied()
        .chain(fragment.entries.iter().copied())
        .collect();
    fragment.entries.sort_unstable();
    fragment.digest = digest(&fragment.entries);
    let before = store.graph_budget().usage.lock().unwrap().decoded;
    let pure_retained = extended
        .extend_fragment(pure.receipt().action(), &fragment)
        .unwrap();
    assert!(pure_retained.bundles == extended.bundles);
    assert_eq!(pure_retained.entries.len(), 3);
    assert_eq!(
        store.graph_budget().usage.lock().unwrap().decoded - before,
        (bytes + std::mem::size_of::<Entry>()) as u64
    );

    // Entries alone are not the copy's charge: both exact bindings are charged
    // to the original account as well. One byte short of that exact
    // charge the copy still succeeds, and the account records a high water of
    // exactly one byte past the target.
    let target = super::super::MAX_DECODED_BYTES_PER_GRAPH;
    let spent = store.graph_budget().usage.lock().unwrap().decoded;
    store
        .graph_budget()
        .decode((target - spent) as usize - bytes + 1)
        .unwrap();
    assert!(!store.graph_budget().exceedances().is_exceeded());
    let copied = extended
        .try_clone()
        .expect("a copy past the accounting target is still owned");
    assert!(copied.bundles == extended.bundles);
    let decoded = store
        .graph_budget()
        .exceedances()
        .axis(super::super::DECODED_RESOURCE)
        .copied()
        .expect("the copy charges the decoded-byte account");
    assert_eq!(decoded.target, target);
    assert_eq!(decoded.high_water, target + 1);
    assert_eq!(
        store.graph_budget().usage.lock().unwrap().decoded,
        target + 1
    );
}

#[test]
fn retention_bundled_extension_rejects_missing_extra_and_foreign_closure() {
    let root = Root::prepare();
    let store = root.open();
    let parent = bundled(&current(&store, b"retained-bundle"), false);
    let retained = Retention::from_validated(&parent, Some(commitment())).unwrap();
    let child = bundled_successor(&store, b"new-bundle", &parent);
    assert!(matches!(
        retained.extend_current(&child),
        Err(Error::State("retention bundled root has no commitment"))
    ));
    let pure = current(&store, b"pure-output");
    assert!(matches!(
        retained.extend_bundled_current(&pure, commitment()),
        Err(Error::State("retention has an extra bundle commitment"))
    ));
    let other = bundled(&current(&store, b"unowned-bundle"), false);
    let unrelated = bundled_successor(&store, b"unowned-child", &other);
    assert!(matches!(
        retained.extend_bundled_current(&unrelated, commitment()),
        Err(Error::State(
            "retention successor changed its ancestor closure"
        ))
    ));
    let foreign_store = root.open();
    let foreign = bundled_successor(&foreign_store, b"foreign-bundle", &parent);
    assert!(matches!(
        retained.extend_bundled_current(&foreign, commitment()),
        Err(Error::State("node read-set belongs to another graph"))
    ));

    let mut foreign_context = child.receipt().read_set().clone();
    foreign_context.context = Some(Digest::from_bytes([89; 32]));
    assert!(matches!(
        retained.extend_with_commitment(
            child.receipt().action(),
            &foreign_context,
            Some(commitment())
        ),
        Err(Error::State("node read-set belongs to another context"))
    ));

    let mut changed = bundled_successor(&store, b"changed-bundle", &parent);
    // Reconstruct a same-action ancestor with foreign content identity.
    let mut fragment = changed.receipt().read_set().clone();
    let ancestor = fragment
        .entries
        .iter_mut()
        .find(|entry| entry.action == parent.receipt().action())
        .unwrap();
    ancestor.content = Digest::from_bytes([77; 32]);
    fragment.entries.sort_unstable();
    fragment.digest = digest(&fragment.entries);
    let receipt = Receipt::new(
        changed.receipt().action(),
        changed.receipt().recipe_digest(),
        changed.content(),
        fragment,
    );
    changed = Validated::new(Arc::new(Owned), changed.content(), receipt);
    assert!(matches!(
        retained.extend_bundled_current(&changed, commitment()),
        Err(Error::State(
            "retention successor changed its ancestor closure"
        ))
    ));
}

#[test]
fn retention_storage_rejects_missing_extra_or_foreign_entry_bindings() {
    let root = Root::prepare();
    let store = root.open();
    let parent = bundled(&current(&store, b"bound-parent"), false);
    let retained = Retention::from_validated(&parent, Some(commitment())).unwrap();
    let child = bundled_successor(&store, b"bound-child", &parent);
    let retained = retained
        .extend_bundled_current(&child, commitment())
        .unwrap();
    let mut missing = retained.try_clone().unwrap();
    missing.bundles = missing.bundles[..1].into();
    assert!(matches!(
        missing.validate_storage(&store),
        Err(Error::State(
            "retention bundle bindings differ from its closure"
        ))
    ));
    let mut extra = retained.try_clone().unwrap();
    extra.bundles = extra
        .bundles
        .iter()
        .copied()
        .chain(extra.bundles.last().copied())
        .collect();
    assert!(matches!(
        extra.validate_storage(&store),
        Err(Error::State("retention has an extra bundle commitment"))
    ));
    let mut foreign = retained.try_clone().unwrap();
    foreign.bundles[0].0.content = Digest::from_bytes([88; 32]);
    assert!(matches!(
        foreign.validate_storage(&store),
        Err(Error::State(
            "retention bundle bindings differ from its closure"
        ))
    ));
}

#[test]
fn retention_union_preserves_siblings_and_charges_original_arrays() {
    let root = Root::prepare();
    let store = root.open();
    let seed = owned_current(&store, b"union-seed", &[]);
    let left = owned_current(&store, b"union-left", &[Parent::validated(&seed)]);
    let right = owned_current(&store, b"union-right", &[Parent::validated(&seed)]);
    let left_retained = Retention::from_validated(&left, None).unwrap();
    let right_retained = Retention::from_validated(&right, None).unwrap();
    let before = store.graph_budget().usage.lock().unwrap().decoded;
    let union = left_retained.union(&right_retained).unwrap();
    assert_eq!(union.entries.len(), 3);
    assert!(union.entries.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(
        store.graph_budget().usage.lock().unwrap().decoded - before,
        (4 * std::mem::size_of::<Entry>()) as u64
    );
    assert!(union.graph.shares_with(store.graph_budget()));
    assert!(union.entries == right_retained.union(&left_retained).unwrap().entries);
    union.validate_storage(&store).unwrap();
    let proof = Fragment::seal(
        &store,
        [
            left.receipt().read_set().clone(),
            right.receipt().read_set().clone(),
        ],
    )
    .unwrap();
    union.require_proof(&proof).unwrap();
    assert!(left_retained.require_proof(&proof).is_err());
    let subset = Fragment::seal(&store, [left.receipt().read_set().clone()]).unwrap();
    assert!(union.require_proof(&subset).is_err());
}

#[test]
fn retention_union_preserves_multiple_bundle_bindings_and_rejects_conflicts() {
    let root = Root::prepare();
    let store = root.open();
    let first = bundled(&current(&store, b"union-first-bundle"), false);
    let second = bundled(&current(&store, b"union-second-bundle"), false);
    let left = Retention::from_validated(&first, Some(commitment())).unwrap();
    let right = Retention::from_validated(&second, Some(commitment())).unwrap();
    let both = left.union(&right).unwrap();
    let before = store.graph_budget().usage.lock().unwrap().decoded;
    let union = both.union(&left).unwrap();
    assert_eq!(union.entries.len(), 2);
    assert!(union.bundles == both.bundles);
    union.require_bindings().unwrap();
    assert_eq!(
        store.graph_budget().usage.lock().unwrap().decoded - before,
        (3 * (std::mem::size_of::<Entry>() + std::mem::size_of::<(Entry, BundleCommitment)>()))
            as u64
    );
    let mut conflicting = left.try_clone().unwrap();
    conflicting.bundles[0].1 = BundleCommitment::new(
        BundleRef::new(
            super::super::ExecutionKey::from_bytes([24; 32]),
            BundleId::from_bytes([25; 32]),
        ),
        0,
        [26; 32],
    );
    assert!(matches!(
        left.union(&conflicting),
        Err(Error::State(
            "retention action has contradictory bundle commitments"
        ))
    ));
    let mut missing = both.try_clone().unwrap();
    missing.bundles = missing.bundles[..1].into();
    assert!(left.union(&missing).is_err());
    let mut extra = left.try_clone().unwrap();
    extra.bundles = both.bundles.clone();
    assert!(extra.union(&right).is_err());
}

#[test]
fn retention_union_rejects_changed_action_and_foreign_lineage() {
    let root = Root::prepare();
    let store = root.open();
    let value = current(&store, b"union-identity");
    let retained = Retention::from_validated(&value, None).unwrap();
    let mut changed = retained.try_clone().unwrap();
    changed.entries[0].content = Digest::from_bytes([90; 32]);
    changed.digest = digest(&changed.entries);
    assert!(matches!(
        retained.union(&changed),
        Err(Error::State(
            "retention action has contradictory identities"
        ))
    ));
    // Entry ordering starts with kind, so same-action conflicts must also be
    // found when their exact-entry sort positions differ.
    changed.entries[0].kind = Kind::Resolution;
    assert!(retained.union(&changed).is_err());
    let mut foreign = retained.try_clone().unwrap();
    foreign.context = Digest::from_bytes([91; 32]);
    assert!(matches!(
        retained.union(&foreign),
        Err(Error::State("retention belongs to another context"))
    ));
    let other_store = root.open();
    foreign.context = retained.context;
    foreign.graph = other_store.graph_budget().clone();
    assert!(matches!(
        retained.union(&foreign),
        Err(Error::State("retention belongs to another graph"))
    ));
    let mut proof = ReadSetProof {
        fragment: value.receipt().read_set().clone(),
        snapshot: None,
    };
    retained.require_proof(&proof).unwrap();
    proof.fragment.digest = Digest::from_bytes([92; 32]);
    assert!(retained.require_proof(&proof).is_err());
    proof.fragment.digest = retained.digest;
    proof.fragment.context = Some(Digest::from_bytes([93; 32]));
    assert!(retained.require_proof(&proof).is_err());
    proof.fragment.context = Some(retained.context);
    proof.fragment.graph = other_store.graph_budget().clone();
    assert!(retained.require_proof(&proof).is_err());
}

#[test]
fn retention_union_requires_budget_for_entries_and_commitments_before_allocating() {
    let root = Root::prepare();
    let store = root.open();
    let value = bundled(&current(&store, b"union-account"), false);
    let retained = Retention::from_validated(&value, Some(commitment())).unwrap();
    let bytes =
        2 * (std::mem::size_of::<Entry>() + std::mem::size_of::<(Entry, BundleCommitment)>());
    // One byte short of the union's exact entry-and-commitment charge,
    // the union still allocates and the original account records a high water
    // of exactly one byte past the target: both arrays were charged in full.
    let target = super::super::MAX_DECODED_BYTES_PER_GRAPH;
    let spent = store.graph_budget().usage.lock().unwrap().decoded;
    store
        .graph_budget()
        .decode((target - spent) as usize - bytes + 1)
        .unwrap();
    assert!(!store.graph_budget().exceedances().is_exceeded());
    let union = retained
        .union(&retained)
        .expect("a union past the accounting target is still owned");
    assert!(union.entries() == retained.entries());
    assert!(union.bundles == retained.bundles);
    let decoded = store
        .graph_budget()
        .exceedances()
        .axis(super::super::DECODED_RESOURCE)
        .copied()
        .expect("the union charges the decoded-byte account");
    assert_eq!(decoded.target, target);
    assert_eq!(decoded.first_requested, target + 1);
    assert_eq!(decoded.high_water, target + 1);
    assert_eq!(
        store.graph_budget().usage.lock().unwrap().decoded,
        target + 1
    );
}

#[test]
fn prepared_retention_requires_the_exact_sealed_union() {
    let root = Root::prepare();
    let environment =
        crate::cargo::config::Environment::from_pairs([("CARGO_HOME", root.0.to_str().unwrap())])
            .unwrap();
    let context = crate::context::Context::capture(
        &root.0,
        environment,
        "retention-test-target",
        &crate::cargo::Limits::standard(),
    )
    .unwrap();
    let store = context.open_store().unwrap();
    let left = owned_current(&store, b"prepared-left", &[]);
    let right = owned_current(&store, b"prepared-right", &[]);
    let left_retained = Retention::from_validated(&left, None).unwrap();
    let right_retained = Retention::from_validated(&right, None).unwrap();
    let union = left_retained.union(&right_retained).unwrap();
    let fragments = [
        left.receipt().read_set().clone(),
        right.receipt().read_set().clone(),
    ];
    let prepared = Prepared::seal_union(context.clone(), fragments.clone()).unwrap();
    assert!(prepared.retention.is_none());
    assert!(prepared.with_retention(left_retained).is_err());
    let prepared = Prepared::seal_union(context, fragments)
        .unwrap()
        .with_retention(union)
        .unwrap();
    prepared
        .retention
        .as_ref()
        .unwrap()
        .require_proof(&prepared.proof)
        .unwrap();
}

#[test]
fn retention_union_caps_unique_entries_without_rejecting_overlapping_inputs() {
    let root = Root::prepare();
    let store = root.open();
    let value = current(&store, b"union-limit");
    let mut retained = Retention::from_validated(&value, None).unwrap();
    let template = retained.entries[0];
    retained.entries = (0..MAX_ENTRIES)
        .map(|index| {
            let mut bytes = [0; 32];
            bytes[..8].copy_from_slice(&(index as u64).to_be_bytes());
            Entry {
                action: ActionKey::from_bytes(bytes),
                ..template
            }
        })
        .collect();
    retained.entries.sort_unstable();
    retained.digest = digest(&retained.entries);
    let union = retained.union(&retained).unwrap();
    assert_eq!(union.entries.len(), MAX_ENTRIES);
    assert!(union.entries == retained.entries);
    let mut extra = Retention::from_validated(&value, None).unwrap();
    extra.entries[0].action = ActionKey::from_bytes([255; 32]);
    extra.digest = digest(&extra.entries);
    assert!(matches!(retained.union(&extra), Err(Error::Limit {
        resource: "node read-set entries", limit: MAX_ENTRIES, actual,
    }) if actual == MAX_ENTRIES + 1));
}

#[test]
fn scopes_survive_union_dedup_and_complete_every_admission_boundary() {
    struct Counted {
        entry: Entry,
        scopes: Arc<std::sync::Mutex<Vec<Scope>>>,
        reject_complete: Arc<std::sync::atomic::AtomicBool>,
    }
    impl Current for Counted {
        fn revalidate(&self, _: &super::super::GraphBudget, scope: Scope) -> Result<Entry, Error> {
            self.scopes.lock().unwrap().push(scope);
            if scope == Scope::Complete && self.reject_complete.load(Ordering::SeqCst) {
                return Err(Error::StaleReadSet {
                    action: self.entry.action,
                });
            }
            Ok(self.entry)
        }
    }
    let root = Root::prepare();
    let environment =
        crate::cargo::config::Environment::from_pairs([("CARGO_HOME", root.0.to_str().unwrap())])
            .unwrap();
    let context = crate::context::Context::capture(
        &root.0,
        environment,
        "scope-test-target",
        &crate::cargo::Limits::standard(),
    )
    .unwrap();
    let store = context.open_store().unwrap();
    let value = owned_current(&store, b"scoped", &[]);
    let scopes = Arc::new(std::sync::Mutex::new(Vec::new()));
    let reject_complete = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut fragment = value.receipt().read_set().clone();
    let entry = fragment.entries[0];
    fragment.witnesses[0].current = Arc::new(Counted {
        entry,
        scopes: scopes.clone(),
        reject_complete: reject_complete.clone(),
    });
    let proof = Fragment::seal(&store, [fragment.clone(), fragment.clone()]).unwrap();
    assert_eq!(proof.len(), 1);
    assert_eq!(*scopes.lock().unwrap(), [Scope::Consumer]);
    scopes.lock().unwrap().clear();
    let prepared = Prepared::seal_union(context, [fragment.clone(), fragment]).unwrap();
    assert_eq!(*scopes.lock().unwrap(), [Scope::Complete]);
    scopes.lock().unwrap().clear();
    prepared.revalidate().unwrap();
    prepared.pin_prepared().unwrap();
    assert!(scopes
        .lock()
        .unwrap()
        .iter()
        .all(|scope| *scope == Scope::Consumer));
    scopes.lock().unwrap().clear();
    crate::supervisor::bootstrap::admit_current(Some(&prepared), || Ok(())).unwrap();
    assert_eq!(*scopes.lock().unwrap(), [Scope::Complete]);
    for retained in [false, true] {
        let prepared = if retained {
            prepared.proof.fragment.clone()
        } else {
            proof.fragment.clone()
        };
        let retention = Retention::from_validated(&value, None).unwrap();
        reject_complete.store(true, Ordering::SeqCst);
        scopes.lock().unwrap().clear();
        let proof = ReadSetProof {
            fragment: prepared,
            snapshot: None,
        };
        let called = std::cell::Cell::new(false);
        let operation = || {
            called.set(true);
            Ok::<(), std::convert::Infallible>(())
        };
        let result = if retained {
            store.with_live_last_good_retained(&proof, &retention, operation)
        } else {
            store.with_live_last_good(&proof, operation)
        };
        assert!(matches!(result, Err(Error::StaleReadSet { .. })));
        assert!(!called.get());
        assert_eq!(scopes.lock().unwrap().last(), Some(&Scope::Complete));
    }
    assert!(crate::supervisor::bootstrap::admit_current(Some(&prepared), || Ok(())).is_err());
}

#[test]
fn ordinary_discovered_callbacks_run_unchanged_for_both_scopes() {
    let root = Root::prepare();
    let store = root.open();
    let calls = Arc::new(AtomicU64::new(0));
    let observed = calls.clone();
    let recipe = Discoverable::capture([7; 32], b"ordinary-scope", 64, &[], 0).unwrap();
    let value = Evaluator::new(&store, &crate::metrics::Current)
        .discover_budgeted(
            &recipe,
            move |_: &Owned, _| {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(Box::from(&b"inputs"[..]))
            },
            || Ok(Owned),
        )
        .unwrap();
    let proof = Fragment::seal(&store, [value.receipt().read_set().clone()]).unwrap();
    calls.store(0, Ordering::SeqCst);
    proof.revalidate(&store).unwrap();
    proof.revalidate_complete(&store).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
