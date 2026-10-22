use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::node;

use super::{evaluate, evaluate_retained, Product};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    root: PathBuf,
    chain: node::Chain,
    ancestor: node::ActionKey,
    checks: Arc<AtomicUsize>,
    revision: Arc<AtomicUsize>,
}

impl Fixture {
    fn capture() -> Self {
        let root = std::env::temp_dir().join(format!(
            "rawr-retained-overlay-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&root).expect("the retained overlay fixture root creates");
        let root = fs::canonicalize(root).expect("the fixture root canonicalizes");
        let project = root.join("project");
        let cargo_home = root.join("cargo-home");
        fs::create_dir(&project).expect("the fixture project creates");
        fs::create_dir(&cargo_home).expect("the fixture Cargo home creates");
        let environment = crate::cargo::config::Environment::from_pairs([(
            "CARGO_HOME",
            cargo_home.to_str().expect("the Cargo home path is Unicode"),
        )])
        .expect("the fixture environment seals");
        let limits = crate::cargo::Limits::standard();
        let context = crate::context::Context::capture(
            &project,
            environment,
            "retained-overlay-test-target",
            &limits,
        )
        .expect("the fixture context seals");
        let store = context.open_store().expect("the fixture store opens");
        let evaluator = node::Evaluator::new(&store, &crate::metrics::Current);
        let checks = Arc::new(AtomicUsize::new(0));
        let revision = Arc::new(AtomicUsize::new(0));
        let observed_checks = Arc::clone(&checks);
        let observed_revision = Arc::clone(&revision);
        let recipe = context
            .discover::<Parent>(b"mutable-overlay-ancestor", 64)
            .expect("the observed ancestor recipe seals");
        let ancestor = evaluator
            .discover(
                &recipe,
                move |_| {
                    observed_checks.fetch_add(1, Ordering::Relaxed);
                    Ok(Box::from(
                        observed_revision.load(Ordering::Relaxed).to_le_bytes(),
                    ))
                },
                || Ok(Parent),
            )
            .expect("the observed ancestor authenticates");
        let recipe = context
            .direct_with_parents::<Parent>(
                b"owned-overlay-parent",
                64,
                &[node::Parent::validated(&ancestor)],
            )
            .expect("the intermediate parent recipe seals");
        let parent = evaluator
            .direct(&recipe, || Ok(Parent))
            .expect("the intermediate parent authenticates");
        let retained = node::read_set::Retention::from_validated(&parent, None)
            .expect("the exact parent closure retains without a bundle");
        let chain = node::Chain::validated_retained(context, &parent, retained)
            .expect("the parent chain retains its closure");
        assert_eq!(chain.current_parent().read_set().len(), 2);
        Self {
            root,
            chain,
            ancestor: ancestor.receipt().action(),
            checks,
            revision,
        }
    }

    fn checks(&self) -> usize {
        self.checks.load(Ordering::Relaxed)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct Parent;

impl node::Value for Parent {
    const KIND: node::Kind = node::Kind::ProfileProof;
    const SCHEMA: u32 = u32::MAX;

    fn encode(&self, _: usize) -> Result<Box<[u8]>, node::Error> {
        Ok(Box::from(&b"owned-parent"[..]))
    }

    fn decode(bytes: &[u8], _: usize) -> Result<Self, node::Error> {
        if bytes == b"owned-parent" {
            Ok(Self)
        } else {
            Err(node::Error::Codec("the overlay fixture parent changed"))
        }
    }
}

fn product() -> Product {
    Product {
        aggregate: [1; 32],
        entries: [2; 32],
        lock: Box::from(&b"lock"[..]),
        manifest: Box::from(&b"manifest"[..]),
        source: Box::from(&b"source"[..]),
        source_map: Box::from(&b"map"[..]),
    }
}

#[test]
fn retained_overlay_miss_checks_ancestors_once_and_hit_skips_rendering() {
    let fixture = Fixture::capture();
    let executions = AtomicUsize::new(0);
    let before = fixture.checks();
    let (cold, cold_overlay) = evaluate_retained(&fixture.chain, b"overlay", || {
        executions.fetch_add(1, Ordering::Relaxed);
        assert_eq!(fixture.checks() - before, 1, "check before consumption");
        Ok(product())
    })
    .expect("the retained overlay miss is historical");
    assert_eq!(
        fixture.checks() - before,
        1,
        "post-consumption validation belongs to the runner union"
    );
    assert_eq!(cold_overlay.fragment().len(), 3);

    let before = fixture.checks();
    let (warm, _) = evaluate_retained(&fixture.chain, b"overlay", || {
        executions.fetch_add(1, Ordering::Relaxed);
        Ok(product())
    })
    .expect("the unchanged overlay is a usable hit");
    assert_eq!(executions.load(Ordering::Relaxed), 1);
    assert_eq!(
        fixture.checks(),
        before,
        "the established hit does not replay ancestors"
    );
    assert_eq!(digest(&warm), digest(&cold));
    assert_eq!(warm.value().source, cold.value().source);
}

#[test]
fn retained_overlay_removes_two_ancestor_walks_from_the_conservative_miss() {
    let fixture = Fixture::capture();
    let before = fixture.checks();
    let conservative = evaluate(&fixture.chain, b"conservative-overlay", || Ok(product()))
        .expect("the conservative overlay publishes");
    let conservative_checks = fixture.checks() - before;
    let before = fixture.checks();
    let (retained, _) = evaluate_retained(&fixture.chain, b"retained-overlay", || Ok(product()))
        .expect("the retained overlay remains historical");
    let retained_checks = fixture.checks() - before;
    assert_eq!(conservative_checks, 3);
    assert_eq!(retained_checks, 1);
    assert_eq!(digest(&retained), conservative.content().digest());
}

#[test]
fn retained_overlay_rejects_ancestor_drift_before_consuming_inputs() {
    let fixture = Fixture::capture();
    fixture.revision.store(1, Ordering::Relaxed);
    let result = evaluate_retained(&fixture.chain, b"stale-before-overlay", || {
        panic!("a stale transitive ancestor must reject before rendering")
    });
    assert!(matches!(
        result,
        Err(node::Error::StaleReadSet { action }) if action == fixture.ancestor
    ));
}

#[test]
fn retained_overlay_rejects_mutation_during_render_and_later_current_use() {
    let fixture = Fixture::capture();
    let executions = AtomicUsize::new(0);
    let result = evaluate_retained(&fixture.chain, b"stale-during-overlay", || {
        executions.fetch_add(1, Ordering::Relaxed);
        fixture.revision.store(1, Ordering::Relaxed);
        Ok(product())
    });
    let (_, overlay) = result.expect("changed inputs cannot grant current output authority");
    assert!(matches!(
        crate::runner::overlay::prepare_union(&fixture.chain, Some(&overlay)),
        Err(node::Error::StaleReadSet { action }) if action == fixture.ancestor
    ));
    assert_eq!(executions.load(Ordering::Relaxed), 1);

    // Historical publication is allowed. The established lookup does not
    // replay mutable witnesses, so its receipt must still fail the next
    // consumer/final-union freshness boundary.
    let (_, historical) = evaluate_retained(&fixture.chain, b"stale-during-overlay", || {
        panic!("the historical overlay remains stored")
    })
    .expect("the historical product authenticates under the same recipe");
    let store = fixture
        .chain
        .context()
        .open_store()
        .expect("the store reopens");
    assert!(matches!(
        historical.fragment().revalidate(&store),
        Err(node::Error::StaleReadSet { action }) if action == fixture.ancestor
    ));
}

#[test]
fn historical_overlay_union_rejects_foreign_context_and_retains_exact_closure() {
    let fixture = Fixture::capture();
    let foreign = Fixture::capture();
    let (product, overlay) =
        evaluate_retained(&fixture.chain, b"retained-union", || Ok(product())).unwrap();
    drop(product);
    assert!(crate::runner::overlay::prepare_union(&foreign.chain, Some(&overlay)).is_err());
    let prepared = crate::runner::overlay::prepare_union(&fixture.chain, Some(&overlay)).unwrap();
    assert!(prepared.proof().contains(overlay.action()));
    assert!(prepared.proof().contains(fixture.ancestor));
    fixture.revision.store(1, Ordering::Relaxed);
    assert!(prepared.revalidate().is_err());
}

#[test]
fn historical_overlay_union_preserves_original_decode_budget() {
    let fixture = Fixture::capture();
    let (_, overlay) =
        evaluate_retained(&fixture.chain, b"bounded-union", || Ok(product())).unwrap();
    let budget = fixture.chain.context().graph_budget();
    let (decoded, _, _) = budget.census_usage().unwrap();
    budget
        .decode((node::MAX_DECODED_BYTES_PER_GRAPH - decoded) as usize)
        .unwrap();
    assert!(!budget.exceedances().is_exceeded());
    // The union still charges the original graph's decode account; at
    // the exact target it succeeds and records the decoded axis with a high
    // water equal to the account's cumulative total.
    crate::runner::overlay::prepare_union(&fixture.chain, Some(&overlay)).unwrap();
    let (decoded, _, _) = budget.census_usage().unwrap();
    assert!(decoded > node::MAX_DECODED_BYTES_PER_GRAPH);
    let exceeded = budget.exceedances();
    let axis = exceeded.axis(node::DECODED_RESOURCE).unwrap();
    assert_eq!(axis.target, node::MAX_DECODED_BYTES_PER_GRAPH);
    assert!(axis.first_requested > node::MAX_DECODED_BYTES_PER_GRAPH);
    assert_eq!(axis.high_water, decoded);
    assert!(axis.site.is_some());
}

fn digest(value: &node::staging::Owned<Product>) -> node::Digest {
    match value {
        node::staging::Owned::Current(value) => value.content().digest(),
        node::staging::Owned::Staged(value) => value.content().digest(),
    }
}
