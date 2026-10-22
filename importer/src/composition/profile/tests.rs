use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crate::composition::tests::{demand, graph, snapshot, Root};
use crate::profile;

#[test]
fn owned_runner_overlay_uses_retained_composition_on_cold_and_warm_requests() {
    let root = Root::new();
    let limits = crate::cargo::Limits::standard();
    let witnesses = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(Calls::default());
    let definition = Definition(Arc::clone(&calls));
    let adapter = profile::trusted::Adapter::trusted(&definition, identity(), version(), false);
    let source = snapshot(&limits);
    let harness = crate::input::proof::Proof::fixture_runner_harness_manifest(&root.0)
        .expect("harness manifest");
    std::fs::write(
        harness.with_file_name("Cargo.lock"),
        b"version = 4\n\n[[package]]\nname = \"rawr-resolution-harness\"\nversion = \"0.0.0\"\n",
    )
    .expect("harness lock");
    let mut owner = crate::supervisor::Owner::new();
    let mut previous = None;
    for warm in [false, true] {
        let prefix = crate::composition::evaluate_retained(
            graph(&root.0, &witnesses, &limits),
            &adapter,
            &crate::classify::Policy::standard(),
            source,
            &limits,
            |contract, _, _| Ok::<_, ()>((demand(contract, source, &limits), Checked)),
        )
        .expect("checked prefix");
        witnesses.store(0, Ordering::Relaxed);
        let generated =
            super::prepare_retained(&adapter, &prefix.profile, &prefix.checked, &limits)
                .expect("historical generated profile");
        assert_eq!(
            witnesses.load(Ordering::Relaxed),
            0,
            "owned generation adds no current promotion"
        );
        let symbolic = prefix
            .checked
            .replay_demand((), &prefix.contract, source, &limits)
            .expect("symbolic demand")
            .with_trusted_runner(false, &prefix.facade, &prefix.contract)
            .expect("owned Runner inputs");
        let request = symbolic
            .request_retained(
                crate::input::proof::Proof::fixture_runner_harness(&root.0).expect("harness"),
                generated,
                owner.revision().expect("revision"),
            )
            .expect("owned request");
        witnesses.store(0, Ordering::Relaxed);
        let manifest = request.generate(&limits).expect("retained overlay");
        let overlay = manifest.overlay.as_ref().expect("overlay receipt");
        assert!(overlay.retention().is_some());
        assert_eq!(witnesses.load(Ordering::Relaxed), if warm { 0 } else { 1 });
        let action = overlay.action();
        if let Some(previous) = previous {
            assert_eq!(action, previous, "warm request reuses the exact overlay");
        }
        previous = Some(action);
    }
}

struct Checked;
impl profile::Checked for Checked {
    fn profile_fingerprint(&self) -> [u8; 32] {
        [0x63; 32]
    }
    fn profile_payload(&self) -> &[u8] {
        &[0x63; 32]
    }
}

struct CachedChecked<'a>(&'a crate::checked::Cached);
impl profile::Checked for CachedChecked<'_> {
    fn profile_fingerprint(&self) -> [u8; 32] {
        self.0.checked_fingerprint()
    }
    fn profile_payload(&self) -> &[u8] {
        self.0.checked_payload()
    }
    fn node_chain(&self) -> Option<&crate::node::Chain> {
        Some(self.0.node_chain())
    }
}

#[derive(Default)]
struct Calls {
    demand: AtomicUsize,
    generated: AtomicUsize,
}

struct Definition(Arc<Calls>);
impl<C: profile::Checked> profile::Definition<C> for Definition {
    fn identity(&self) -> profile::Identity {
        identity()
    }
    fn version(&self) -> profile::Version {
        version()
    }
    fn classify(
        &self,
        _: &crate::api::Graph<crate::api::Extracted>,
        _: &mut profile::contract::Builder<'_>,
    ) -> Result<(), profile::Error> {
        Ok(())
    }
    fn demand(
        &self,
        _: &profile::Contract,
        _: &C,
        _: &mut profile::demand::Builder<'_>,
    ) -> Result<(), profile::Error> {
        self.0.demand.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    fn generate(
        &self,
        _: &profile::Demand,
        _: &mut profile::demand::Generator<'_>,
    ) -> Result<(), profile::Error> {
        self.0.generated.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}
impl profile::trusted::Operations for &Definition {
    fn classify<P: crate::api::Phase>(
        &self,
        _: &crate::api::Graph<P>,
        _: &mut profile::contract::Builder<'_, P>,
    ) -> Result<(), profile::Error> {
        Ok(())
    }
    fn demand(
        &self,
        _: profile::trusted::Contract<'_>,
        _: profile::trusted::Checked<'_>,
        _: &mut profile::demand::Builder<'_>,
    ) -> Result<(), profile::Error> {
        self.0.demand.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    fn generate(
        &self,
        _: profile::trusted::Demand<'_>,
        _: &mut profile::demand::Generator<'_>,
    ) -> Result<(), profile::Error> {
        self.0.generated.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}
fn identity() -> profile::Identity {
    profile::Identity::new(
        "rawr.composition.downstream",
        &crate::cargo::Limits::standard(),
    )
    .expect("identity")
}
fn version() -> profile::Version {
    profile::Version::new(1).expect("version")
}

#[test]
fn downstream_cold_mixed_and_warm_reuse_exact_owned_outputs() {
    // The fourth state uses admitted selective eviction in the separate test.
    // These are the three dependency-consistent initial seed states.
    for (seed_demand, seed_generated) in [(false, false), (true, false), (true, true)] {
        let root = Root::new();
        let limits = crate::cargo::Limits::standard();
        let witnesses = Arc::new(AtomicUsize::new(0));
        let calls = Arc::new(Calls::default());
        let definition = Definition(Arc::clone(&calls));
        let adapter = profile::trusted::Adapter::trusted(&definition, identity(), version(), false);
        let policy = crate::classify::Policy::standard();
        let source = snapshot(&limits);
        let prefix = crate::composition::evaluate(
            graph(&root.0, &witnesses, &limits),
            &adapter,
            &policy,
            source,
            &limits,
            |contract, _, _| Ok::<_, ()>((demand(contract, source, &limits), Checked)),
        )
        .expect("checked prefix");
        if seed_demand {
            let seeded = profile::demand(
                &definition,
                &prefix.profile,
                &CachedChecked(&prefix.checked),
                &limits,
            )
            .expect("seed admitted demand");
            if seed_generated {
                profile::Proof::generate::<CachedChecked<'_>, _>(&definition, &seeded, &limits)
                    .expect("seed admitted generated proof");
            }
        }
        witnesses.store(0, Ordering::Relaxed);
        let (first_demand, first_generated) =
            super::evaluate(&adapter, &prefix.profile, &prefix.checked, &limits)
                .expect("connected downstream");
        assert_eq!(calls.demand.load(Ordering::Relaxed), 1);
        assert_eq!(calls.generated.load(Ordering::Relaxed), 1);
        if seed_generated {
            assert_eq!(
                witnesses.load(Ordering::Relaxed),
                0,
                "all hits add no mutable union"
            );
        }
        let demand_chain = first_demand.node_chain().expect("demand chain");
        let generated_chain = first_generated.node_chain().expect("generated chain");
        assert!(
            demand_chain.retention().is_some(),
            "demand carries retained authority"
        );
        assert!(
            generated_chain.retention().is_some(),
            "generated carries retained authority"
        );
        assert!(generated_chain
            .parent()
            .read_set()
            .contains(demand_chain.parent().action()));
        assert!(generated_chain
            .parent()
            .read_set()
            .contains(prefix.checked.node_chain().parent().action()));
        witnesses.store(0, Ordering::Relaxed);
        let (warm_demand, warm_generated) =
            super::evaluate(&adapter, &prefix.profile, &prefix.checked, &limits)
                .expect("warm downstream");
        assert_eq!(witnesses.load(Ordering::Relaxed), 0);
        assert_eq!(calls.demand.load(Ordering::Relaxed), 1);
        assert_eq!(calls.generated.load(Ordering::Relaxed), 1);
        assert_eq!(first_demand.digest(), warm_demand.digest());
        assert_eq!(
            first_demand.checked_payload(),
            warm_demand.checked_payload()
        );
        assert_eq!(first_generated.fingerprint(), warm_generated.fingerprint());
        assert_eq!(
            first_generated.generated_source(),
            warm_generated.generated_source()
        );
        assert_eq!(
            demand_chain.parent().action(),
            warm_demand
                .node_chain()
                .expect("warm demand chain")
                .parent()
                .action()
        );
        assert_eq!(
            generated_chain.parent().action(),
            warm_generated
                .node_chain()
                .expect("warm generated chain")
                .parent()
                .action()
        );

        let mut edited = crate::surface::Snapshot::builder(&limits);
        edited.entry("/fixture/rawr.toml").expect("entry");
        edited
            .file("/fixture/main.rawr", b"Pair // edited")
            .expect("edited source");
        let edited = edited.seal().expect("edited snapshot");
        let prefix = crate::composition::evaluate(
            graph(&root.0, &witnesses, &limits),
            &adapter,
            &policy,
            edited,
            &limits,
            |contract, _, _| Ok::<_, ()>((demand(contract, edited, &limits), Checked)),
        )
        .expect("edited checked prefix");
        let (edited_demand, edited_generated) =
            super::evaluate(&adapter, &prefix.profile, &prefix.checked, &limits)
                .expect("edited downstream");
        assert_eq!(
            calls.demand.load(Ordering::Relaxed),
            2,
            "changed checked content computes demand once"
        );
        let generated_calls = calls.generated.load(Ordering::Relaxed);
        assert!((1..=2).contains(&generated_calls));
        assert_eq!(first_demand.digest(), edited_demand.digest());
        assert_eq!(
            first_generated.fingerprint(),
            edited_generated.fingerprint()
        );
        witnesses.store(0, Ordering::Relaxed);
        let (edited_warm_demand, edited_warm_generated) =
            super::evaluate(&adapter, &prefix.profile, &prefix.checked, &limits)
                .expect("edited warm downstream");
        assert_eq!(calls.demand.load(Ordering::Relaxed), 2);
        assert_eq!(calls.generated.load(Ordering::Relaxed), generated_calls);
        assert_eq!(witnesses.load(Ordering::Relaxed), 0);
        assert_eq!(edited_demand.digest(), edited_warm_demand.digest());
        assert_eq!(
            edited_generated.fingerprint(),
            edited_warm_generated.fingerprint()
        );
    }
}

#[test]
fn downstream_promotion_rejects_mutated_external_ancestor() {
    let root = Root::new();
    let limits = crate::cargo::Limits::standard();
    let witnesses = Arc::new(AtomicUsize::new(0));
    let changed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let calls = Arc::new(Calls::default());
    let definition = Definition(Arc::clone(&calls));
    let adapter = profile::trusted::Adapter::trusted(&definition, identity(), version(), false);
    let policy = crate::classify::Policy::standard();
    let source = snapshot(&limits);
    let prefix = crate::composition::evaluate(
        crate::composition::tests::graph_with_change(
            &root.0,
            &witnesses,
            Arc::clone(&changed),
            &limits,
        ),
        &adapter,
        &policy,
        source,
        &limits,
        |contract, _, _| Ok::<_, ()>((demand(contract, source, &limits), Checked)),
    )
    .expect("checked prefix before external mutation");
    changed.store(true, Ordering::Relaxed);
    witnesses.store(0, Ordering::Relaxed);
    assert!(
        super::evaluate(&adapter, &prefix.profile, &prefix.checked, &limits).is_err(),
        "staged owned computation cannot grant current authority after external drift"
    );
    assert!(
        witnesses.load(Ordering::Relaxed) > 0,
        "promotion must invoke mutable ancestor witness"
    );
}

#[test]
fn downstream_rejects_foreign_profile_with_matching_semantics_before_callbacks() {
    let first_root = Root::new();
    let second_root = Root::new();
    let limits = crate::cargo::Limits::standard();
    let witnesses = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(Calls::default());
    let definition = Definition(Arc::clone(&calls));
    let adapter = profile::trusted::Adapter::trusted(&definition, identity(), version(), false);
    let policy = crate::classify::Policy::standard();
    let source = snapshot(&limits);
    let build_prefix = |root: &Root| {
        crate::composition::evaluate(
            graph(&root.0, &witnesses, &limits),
            &adapter,
            &policy,
            source,
            &limits,
            |contract, _, _| Ok::<_, ()>((demand(contract, source, &limits), Checked)),
        )
        .expect("independent checked prefix")
    };
    let first = build_prefix(&first_root);
    let second = build_prefix(&second_root);
    // Semantic equality and the same audited adapter do not make one context's
    // profile a proven ancestor of another context's checked result.
    assert_eq!(first.profile.identity(), second.profile.identity());
    assert_eq!(first.profile.version(), second.profile.version());
    assert_eq!(first.profile.rules_digest(), second.profile.rules_digest());
    assert_eq!(first.contract.fingerprint(), second.contract.fingerprint());
    assert_eq!(
        first.checked.checked_fingerprint(),
        second.checked.checked_fingerprint()
    );
    assert_ne!(
        first
            .profile
            .node_chain()
            .expect("first profile chain")
            .parent()
            .action(),
        second
            .profile
            .node_chain()
            .expect("second profile chain")
            .parent()
            .action(),
    );
    for warm in [false, true] {
        if warm {
            super::evaluate(&adapter, &first.profile, &first.checked, &limits)
                .expect("seed valid downstream hits");
        }
        let demand_calls = calls.demand.load(Ordering::Relaxed);
        let generated_calls = calls.generated.load(Ordering::Relaxed);
        witnesses.store(0, Ordering::Relaxed);
        assert!(
            super::evaluate(&adapter, &second.profile, &first.checked, &limits).is_err(),
            "foreign profile must not reuse or construct outputs from checked A"
        );
        assert_eq!(calls.demand.load(Ordering::Relaxed), demand_calls);
        assert_eq!(calls.generated.load(Ordering::Relaxed), generated_calls);
        assert_eq!(
            witnesses.load(Ordering::Relaxed),
            0,
            "foreign lineage is rejected before mutable validation"
        );
    }
}

#[test]
fn downstream_rebuilds_evicted_demand_and_reuses_surviving_generated_output() {
    use crate::node::staging::{Outcome, Recipe};
    use crate::profile::demand::cached;

    let root = Root::new();
    let limits = crate::cargo::Limits::standard();
    let witnesses = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(Calls::default());
    let definition = Definition(Arc::clone(&calls));
    let adapter = profile::trusted::Adapter::trusted(&definition, identity(), version(), false);
    let policy = crate::classify::Policy::standard();
    let source = snapshot(&limits);
    let prefix = crate::composition::evaluate(
        graph(&root.0, &witnesses, &limits),
        &adapter,
        &policy,
        source,
        &limits,
        |contract, _, _| Ok::<_, ()>((demand(contract, source, &limits), Checked)),
    )
    .expect("checked prefix");
    let prepared = prefix
        .checked
        .node_chain()
        .prepare_read_set()
        .expect("prefix read set");
    prepared
        .pin_prepared()
        .expect("protect only prefix allocations");
    let seeded_demand = profile::demand(
        &definition,
        &prefix.profile,
        &CachedChecked(&prefix.checked),
        &limits,
    )
    .expect("seed admitted demand");
    let seeded_generated =
        profile::Proof::generate::<CachedChecked<'_>, _>(&definition, &seeded_demand, &limits)
            .expect("seed newer admitted generated output");
    let store = prefix
        .checked
        .node_chain()
        .context()
        .open_store()
        .expect("store");
    let generated_action = seeded_generated
        .node_chain()
        .expect("generated chain")
        .parent()
        .action();
    let generated_envelope = store
        .root()
        .join("actions")
        .join("profile-proof")
        .join(crate::node::STORAGE_DIRECTORY)
        .join(generated_action.directory())
        .join("envelope");
    let generated_bytes =
        std::fs::read(&generated_envelope).expect("generated envelope before eviction");
    assert!(store
        .fixture_evict_one(false)
        .expect("journaled oldest unpinned unit eviction"));
    let recipe = Recipe::<cached::Product>::from_chain(
        prefix.checked.node_chain(),
        &cached::demand_authority(
            identity(),
            version(),
            prefix.profile.rules_digest(),
            &prefix.checked.checked_fingerprint(),
            prefix.checked.checked_payload(),
            &limits,
            Some(&prefix.profile.graph_fingerprint()),
        )
        .expect("demand authority"),
        limits.node_output_bytes(),
    )
    .expect("exact demand recipe");
    assert!(
        matches!(
            recipe
                .lookup_owned(&crate::node::Evaluator::new(
                    &store,
                    &crate::metrics::Current
                ))
                .expect("missing exact demand lookup"),
            Outcome::Miss(_)
        ),
        "demand action was evicted"
    );
    assert_eq!(
        std::fs::read(&generated_envelope).expect("generated survives eviction"),
        generated_bytes
    );
    let (rebuilt_demand, reused_generated) =
        super::evaluate(&adapter, &prefix.profile, &prefix.checked, &limits)
            .expect("recover missing demand with surviving generated output");
    assert_eq!(calls.demand.load(Ordering::Relaxed), 2);
    assert_eq!(
        calls.generated.load(Ordering::Relaxed),
        1,
        "generated callback must not repeat"
    );
    assert_eq!(seeded_demand.digest(), rebuilt_demand.digest());
    assert_eq!(
        seeded_generated.fingerprint(),
        reused_generated.fingerprint()
    );
    assert_eq!(
        seeded_generated.generated_source(),
        reused_generated.generated_source()
    );
    assert_eq!(
        generated_action,
        reused_generated
            .node_chain()
            .expect("reused generated chain")
            .parent()
            .action()
    );
    assert!(reused_generated
        .node_chain()
        .expect("reused generated chain")
        .retention()
        .is_some());
    drop(prepared);
}

#[test]
fn historical_checked_continuation_saves_one_cold_replay_and_reuses_warm_outputs() {
    let limits = crate::cargo::Limits::standard();
    let source = snapshot(&limits);
    let mut ordinary_checks = 0;
    for retained in [false, true] {
        let root = Root::new();
        let witnesses = Arc::new(AtomicUsize::new(0));
        let calls = Arc::new(Calls::default());
        let definition = Definition(Arc::clone(&calls));
        let adapter = profile::trusted::Adapter::trusted(&definition, identity(), version(), false);
        let api = graph(&root.0, &witnesses, &limits);
        witnesses.store(0, Ordering::Relaxed);
        if !retained {
            let prefix = crate::composition::evaluate(
                api,
                &adapter,
                &crate::classify::Policy::standard(),
                source,
                &limits,
                |contract, _, _| Ok::<_, ()>((demand(contract, source, &limits), Checked)),
            )
            .unwrap();
            super::evaluate(&adapter, &prefix.profile, &prefix.checked, &limits).unwrap();
            ordinary_checks = witnesses.load(Ordering::Relaxed);
            continue;
        }
        let prefix = crate::composition::evaluate_retained(
            api,
            &adapter,
            &crate::classify::Policy::standard(),
            source,
            &limits,
            |contract, _, _| {
                assert_eq!(witnesses.load(Ordering::Relaxed), 1, "fresh before source");
                Ok::<_, ()>((demand(contract, source, &limits), Checked))
            },
        )
        .unwrap();
        assert_eq!(
            witnesses.load(Ordering::Relaxed),
            1,
            "checked bytes remain historical"
        );
        let (selected, generated) =
            super::evaluate_retained(&adapter, &prefix.profile, &prefix.checked, &limits).unwrap();
        assert_eq!(witnesses.load(Ordering::Relaxed) + 1, ordinary_checks);
        let warm_api = graph(&root.0, &witnesses, &limits);
        witnesses.store(0, Ordering::Relaxed);
        let warm = crate::composition::evaluate_retained(
            warm_api,
            &adapter,
            &crate::classify::Policy::standard(),
            source,
            &limits,
            |_, _, _| -> Result<(crate::Demand<crate::Sealed, ()>, Checked), ()> {
                panic!("warm checked output skips source")
            },
        )
        .unwrap();
        let (warm_selected, warm_generated) =
            super::evaluate_retained(&adapter, &warm.profile, &warm.checked, &limits).unwrap();
        assert_eq!(witnesses.load(Ordering::Relaxed), 0);
        assert_eq!(selected.digest(), warm_selected.digest());
        assert_eq!(generated.fingerprint(), warm_generated.fingerprint());
        assert_eq!(calls.demand.load(Ordering::Relaxed), 1);
        assert_eq!(calls.generated.load(Ordering::Relaxed), 1);
    }
}

#[test]
fn historical_checked_mutation_rejects_at_profile_promotion() {
    let root = Root::new();
    let limits = crate::cargo::Limits::standard();
    let witnesses = Arc::new(AtomicUsize::new(0));
    let changed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let definition = Definition(Arc::new(Calls::default()));
    let adapter = profile::trusted::Adapter::trusted(&definition, identity(), version(), false);
    let source = snapshot(&limits);
    let api = crate::composition::tests::graph_with_change(
        &root.0,
        &witnesses,
        Arc::clone(&changed),
        &limits,
    );
    let prefix = crate::composition::evaluate_retained(
        api,
        &adapter,
        &crate::classify::Policy::standard(),
        source,
        &limits,
        |contract, _, _| {
            changed.store(true, Ordering::Relaxed);
            Ok::<_, ()>((demand(contract, source, &limits), Checked))
        },
    )
    .expect("owned output does not claim current ancestry");
    assert!(super::evaluate_retained(&adapter, &prefix.profile, &prefix.checked, &limits).is_err());
}

#[test]
fn historical_checked_rejects_foreign_profile_and_records_original_budget_exceedance() {
    let limits = crate::cargo::Limits::standard();
    let source = snapshot(&limits);
    let witnesses = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(Calls::default());
    let definition = Definition(Arc::clone(&calls));
    let adapter = profile::trusted::Adapter::trusted(&definition, identity(), version(), false);
    let first = Root::new();
    let second = Root::new();
    let build = |root: &Root| {
        crate::composition::evaluate_retained(
            graph(&root.0, &witnesses, &limits),
            &adapter,
            &crate::classify::Policy::standard(),
            source,
            &limits,
            |contract, _, _| Ok::<_, ()>((demand(contract, source, &limits), Checked)),
        )
        .unwrap()
    };
    let first = build(&first);
    let second = build(&second);
    assert!(super::evaluate_retained(&adapter, &second.profile, &first.checked, &limits).is_err());
    assert_eq!(calls.demand.load(Ordering::Relaxed), 0);
    // The historical evaluation keeps charging the checked context's
    // original cumulative graph account. Exhausting that account's decoded
    // target never refuses the evaluation: it runs to completion, computing
    // the trusted demand exactly once, and every charge it makes is recorded
    // past the target on that same account.
    let budget = first.checked.context.graph_budget();
    let (decoded, _, _) = budget.census_usage().unwrap();
    budget
        .decode((crate::node::MAX_DECODED_BYTES_PER_GRAPH - decoded) as usize)
        .unwrap();
    assert!(!budget.exceedances().is_exceeded());
    super::evaluate_retained(&adapter, &first.profile, &first.checked, &limits)
        .expect("an accounting axis never refuses the historical evaluation");
    let (decoded_after, _, _) = budget.census_usage().unwrap();
    assert!(decoded_after > crate::node::MAX_DECODED_BYTES_PER_GRAPH);
    assert_eq!(
        budget
            .exceedances()
            .axis(crate::node::DECODED_RESOURCE)
            .map(|axis| axis.high_water),
        Some(decoded_after)
    );
    assert_eq!(calls.demand.load(Ordering::Relaxed), 1);
}

#[test]
fn historical_checked_still_rejects_changed_inputs_before_source() {
    let root = Root::new();
    let limits = crate::cargo::Limits::standard();
    let witnesses = Arc::new(AtomicUsize::new(0));
    let changed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let definition = Definition(Arc::new(Calls::default()));
    let adapter = profile::trusted::Adapter::trusted(&definition, identity(), version(), false);
    let api = crate::composition::tests::graph_with_change(
        &root.0,
        &witnesses,
        Arc::clone(&changed),
        &limits,
    );
    changed.store(true, Ordering::Relaxed);
    assert!(crate::composition::evaluate_retained(
        api,
        &adapter,
        &crate::classify::Policy::standard(),
        snapshot(&limits),
        &limits,
        |_, _, _| -> Result<(crate::Demand<crate::Sealed, ()>, Checked), ()> {
            panic!("changed source inputs must reject before consumption")
        }
    )
    .is_err());
}

fn write_retained_runner_harness(root: &Root) {
    let harness = crate::input::proof::Proof::fixture_runner_harness_manifest(&root.0).unwrap();
    std::fs::write(
        harness.with_file_name("Cargo.lock"),
        b"version = 4\n\n[[package]]\nname = \"rawr-resolution-harness\"\nversion = \"0.0.0\"\n",
    )
    .unwrap();
}

#[test]
fn pair_continuation_combines_promotion_with_render_check() {
    let limits = crate::cargo::Limits::standard();
    let source = snapshot(&limits);
    let mut checks = Vec::new();
    for carry_pair in [false, true] {
        let root = Root::new();
        write_retained_runner_harness(&root);
        let witnesses = Arc::new(AtomicUsize::new(0));
        let definition = Definition(Arc::new(Calls::default()));
        let adapter = profile::trusted::Adapter::trusted(&definition, identity(), version(), false);
        let prefix = crate::composition::evaluate_retained(
            graph(&root.0, &witnesses, &limits),
            &adapter,
            &crate::classify::Policy::standard(),
            source,
            &limits,
            |contract, _, _| Ok::<_, ()>((demand(contract, source, &limits), Checked)),
        )
        .unwrap();
        let symbolic = prefix
            .checked
            .replay_demand((), &prefix.contract, source, &limits)
            .unwrap()
            .with_trusted_runner(false, &prefix.facade, &prefix.contract)
            .unwrap();
        let native = crate::input::proof::Proof::fixture_runner_harness(&root.0).unwrap();
        let mut owner = crate::supervisor::Owner::new();
        witnesses.store(0, Ordering::Relaxed);
        let request = if carry_pair {
            symbolic
                .request_retained(
                    native,
                    super::prepare_retained(&adapter, &prefix.profile, &prefix.checked, &limits)
                        .unwrap(),
                    owner.revision().unwrap(),
                )
                .unwrap()
        } else {
            symbolic
                .request(
                    native,
                    super::evaluate_retained(&adapter, &prefix.profile, &prefix.checked, &limits)
                        .unwrap()
                        .1,
                    owner.revision().unwrap(),
                )
                .unwrap()
        };
        let manifest = request.generate(&limits).unwrap();
        assert!(
            manifest.profile.node_chain().is_some(),
            "current proof before runner build"
        );
        checks.push(witnesses.load(Ordering::Relaxed));
    }
    assert_eq!(checks, [2, 1]);
}

#[test]
fn pair_request_rejects_arbitrary_runner_before_callbacks() {
    struct Arbitrary;
    impl crate::surface::Runner for Arbitrary {
        fn compile_snapshot(&self) -> bool {
            panic!("retained entry must reject arbitrary callbacks")
        }
    }
    let root = Root::new();
    write_retained_runner_harness(&root);
    let limits = crate::cargo::Limits::standard();
    let source = snapshot(&limits);
    let witnesses = Arc::new(AtomicUsize::new(0));
    let definition = Definition(Arc::new(Calls::default()));
    let adapter = profile::trusted::Adapter::trusted(&definition, identity(), version(), false);
    let prefix = crate::composition::evaluate_retained(
        graph(&root.0, &witnesses, &limits),
        &adapter,
        &crate::classify::Policy::standard(),
        source,
        &limits,
        |contract, _, _| Ok::<_, ()>((demand(contract, source, &limits), Checked)),
    )
    .unwrap();
    let pair =
        super::prepare_retained(&adapter, &prefix.profile, &prefix.checked, &limits).unwrap();
    let symbolic = prefix
        .checked
        .replay_demand(Arbitrary, &prefix.contract, source, &limits)
        .unwrap();
    let mut owner = crate::supervisor::Owner::new();
    assert!(symbolic
        .request_retained(
            crate::input::proof::Proof::fixture_runner_harness(&root.0).unwrap(),
            pair,
            owner.revision().unwrap()
        )
        .is_err());
}

#[test]
fn historical_pair_rejects_drift_at_actual_render_boundary() {
    let root = Root::new();
    write_retained_runner_harness(&root);
    let limits = crate::cargo::Limits::standard();
    let source = snapshot(&limits);
    let witnesses = Arc::new(AtomicUsize::new(0));
    let changed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let definition = Definition(Arc::new(Calls::default()));
    let adapter = profile::trusted::Adapter::trusted(&definition, identity(), version(), false);
    let prefix = crate::composition::evaluate_retained(
        crate::composition::tests::graph_with_change(
            &root.0,
            &witnesses,
            Arc::clone(&changed),
            &limits,
        ),
        &adapter,
        &crate::classify::Policy::standard(),
        source,
        &limits,
        |contract, _, _| Ok::<_, ()>((demand(contract, source, &limits), Checked)),
    )
    .unwrap();
    let pair =
        super::prepare_retained(&adapter, &prefix.profile, &prefix.checked, &limits).unwrap();
    let symbolic = prefix
        .checked
        .replay_demand((), &prefix.contract, source, &limits)
        .unwrap()
        .with_trusted_runner(false, &prefix.facade, &prefix.contract)
        .unwrap();
    let mut owner = crate::supervisor::Owner::new();
    let request = symbolic
        .request_retained(
            crate::input::proof::Proof::fixture_runner_harness(&root.0).unwrap(),
            pair,
            owner.revision().unwrap(),
        )
        .unwrap();
    changed.store(true, Ordering::Relaxed);
    assert!(request.generate(&limits).is_err());
}
