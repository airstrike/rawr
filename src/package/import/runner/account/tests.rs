use super::{
    baseline_bytes, lock, shared_bytes, Attempt, Axis, Cause, Native, Observation,
    ObservationState, Receipt,
};
use crate::{host, host::ImportAllocation as _, package::import::runner, Engine};
use rawr_import::{runner::manifest::Carrier, surface::identity};
use std::sync::{Arc, Barrier};

/// Versioned empty metadata fixture, never a live native admission.
pub(crate) fn carrier() -> Carrier {
    carrier_entries(&[])
}

fn carrier_entries(entries: &[([u8; 32], [u8; 32], u32)]) -> Carrier {
    Carrier::transform(&carrier_bytes(entries)).unwrap()
}

pub(crate) fn carrier_bytes(entries: &[([u8; 32], [u8; 32], u32)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RAWRGM01");
    bytes.extend_from_slice(&[3; 32]);
    bytes.extend_from_slice(&0_u64.to_le_bytes());
    bytes.extend_from_slice(&(entries.len() as u64).to_le_bytes());
    for (identity, signature, arguments) in entries {
        bytes.extend_from_slice(identity);
        bytes.extend_from_slice(signature);
        bytes.extend_from_slice(&arguments.to_le_bytes());
    }
    bytes.extend_from_slice(&20_u64.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u64.to_le_bytes());
    bytes.extend_from_slice(&0_u64.to_le_bytes());
    bytes.extend_from_slice(&0_u64.to_le_bytes());
    // Exact current manifest, wire, generator, runtime and profile versions.
    for version in rawr_import::runner::manifest::carrier_versions() {
        bytes.extend_from_slice(&version.to_le_bytes());
    }
    bytes
}

#[test]
fn native_account_seeds_decoded_metadata_and_fixed_ownership() {
    let carrier = carrier();
    let (items, bytes) = carrier.retained_usage();
    let native = Native::transform(carrier).unwrap();
    assert_eq!(
        native.usage(),
        (items + 2, bytes + baseline_bytes() + shared_bytes(),)
    );
}

#[test]
fn native_account_overlapping_receipts_add_and_dropped_attempt_releases_its_charge() {
    let native = Native::transform(carrier()).unwrap();
    let baseline = native.usage();
    let mut live = native.attempt().unwrap();
    live.retain_checked(Some(4), Some(40)).unwrap();
    let live = live.finish();
    let mut pending = native.attempt().unwrap();
    pending.retain_checked(Some(6), Some(60)).unwrap();
    let full = native.usage();
    assert_eq!(full.0, baseline.0 + live.usage().0 + pending.usage().0);
    assert_eq!(full.1, baseline.1 + live.usage().1 + pending.usage().1);
    drop(pending);
    assert_eq!(
        native.usage(),
        (baseline.0 + live.usage().0, baseline.1 + live.usage().1)
    );
    drop(live);
    assert_eq!(native.usage(), baseline);
    assert_eq!(native.snapshot().first_refusal, None);
}

#[test]
fn native_account_overflow_and_missing_counts_do_not_partially_charge() {
    let native = Native::transform(carrier()).unwrap();
    let mut attempt = native.attempt().unwrap();
    let before = native.usage();
    for (items, bytes) in [
        (None, Some(1)),
        (Some(1), None),
        (Some(usize::MAX), Some(0)),
        (Some(1), Some(usize::MAX)),
    ] {
        assert!(attempt.retain_checked(items, bytes).is_err());
        assert_eq!(native.usage(), before);
    }
}

#[test]
fn observed_native_decode_charges_only_its_fixed_slot_and_keeps_the_same_account() {
    let ordinary = Native::decode(&carrier_bytes(&[])).unwrap();
    let (native, observation) = Native::decode_observed(&carrier_bytes(&[]));
    let native = native.unwrap();
    let snapshot = observation.snapshot().unwrap();
    assert_eq!(snapshot, native.snapshot());
    assert_eq!(
        snapshot.current,
        [
            ordinary.usage().0 + 1,
            ordinary.usage().1 + std::mem::size_of::<Observation>()
        ]
    );
    assert_eq!(snapshot.peak, snapshot.current);
    assert_eq!(snapshot.first_refusal, None);
    drop(observation);
    assert_eq!(native.usage(), ordinary.usage());
    assert_eq!(native.snapshot().peak, snapshot.peak);
    let retained = native.snapshot();
    let engine = Engine::new().retain_imported_native(native).unwrap();
    assert_eq!(engine.imported_native_snapshot(), Some(retained));
    assert!(Engine::new().imported_native_snapshot().is_none());
}

#[test]
fn reconstructed_native_shares_original_account_without_a_baseline_chain() {
    let (native, initial_observation) = Native::decode_observed(&carrier_bytes(&[]));
    let native = native.unwrap();
    let baseline = Arc::downgrade(&native.inner);
    let initial = native.snapshot();
    let receipt = native.attempt().unwrap().finish();
    let live = native.snapshot();
    let engine = Engine::new().retain_imported_native(native).unwrap();
    let (replacement, observation) = engine.reconstruct_imported_native(&carrier_bytes(&[]));
    let replacement = replacement.unwrap();
    let (carrier_items, retained_bytes) = replacement.carrier().retained_usage();
    let contribution = [carrier_items + 1, retained_bytes + baseline_bytes()];
    let overlapping = replacement.snapshot();
    assert_eq!(overlapping.limits, initial.limits);
    assert_eq!(
        overlapping.current,
        [
            live.current[0] + contribution[0] + 1,
            live.current[1] + contribution[1] + std::mem::size_of::<Observation>(),
        ]
    );
    assert_eq!(initial_observation.snapshot(), observation.snapshot());
    drop(engine);
    assert!(baseline.upgrade().is_some());
    drop(receipt);
    assert!(baseline.upgrade().is_none());
    drop(initial_observation);
    assert_eq!(replacement.snapshot().current, initial.current);
    let replacement_baseline = Arc::downgrade(&replacement.inner);
    let engine = Engine::new().retain_imported_native(replacement).unwrap();
    let (next, next_observation) = engine.reconstruct_imported_native(&carrier_bytes(&[]));
    let next = next.unwrap();
    drop(engine);
    assert!(replacement_baseline.upgrade().is_none());
    drop(observation);
    assert_eq!(next.snapshot().current, initial.current);
    drop(next);
    assert_eq!(
        next_observation.snapshot().unwrap().current,
        [2, shared_bytes() + std::mem::size_of::<Observation>(),]
    );
}

#[test]
fn reconstructed_native_preserves_decode_error_and_requires_original_ownership() {
    let engine = Engine::new();
    let (replacement, observation) = engine.reconstruct_imported_native(&carrier_bytes(&[]));
    assert!(matches!(replacement, Err(runner::Error::NativeOwner)));
    assert!(observation.snapshot().is_none());

    let native = Native::transform(carrier()).unwrap();
    let before = native.snapshot();
    let engine = engine.retain_imported_native(native).unwrap();
    let (replacement, observation) = engine.reconstruct_imported_native(b"invalid manifest");
    assert!(matches!(replacement, Err(runner::Error::Manifest(_))));
    assert_eq!(observation.snapshot(), Some(before));
    assert_eq!(engine.imported_native_snapshot(), Some(before));
}

#[test]
fn observation_does_not_retain_native_owners_and_releases_only_its_slot() {
    let (native, observation) = Native::decode_observed(&carrier_bytes(&[]));
    let native = native.unwrap();
    let baseline = Arc::downgrade(&native.inner);
    let before = observation.snapshot().unwrap();
    let account = match &observation.state {
        ObservationState::Retained(allocation) => {
            assert!(allocation.baseline.is_none());
            Arc::clone(&allocation.account)
        }
        _ => panic!("successful observation must retain the original account"),
    };
    drop(native);
    assert!(baseline.upgrade().is_none());
    let after = observation.snapshot().unwrap();
    assert_eq!(after.peak, before.peak);
    assert_eq!(
        after.current,
        [2, shared_bytes() + std::mem::size_of::<Observation>()]
    );
    drop(observation);
    let state = lock(&account);
    assert_eq!((state.items, state.bytes), (1, shared_bytes()));
    assert_eq!(state.peak, before.peak);
}

#[test]
fn native_refusal_observation_preserves_original_check_order_and_error() {
    let cases = [
        (None, None, Axis::Items, Cause::MissingAmount),
        (Some(usize::MAX), None, Axis::Bytes, Cause::MissingAmount),
        (
            Some(usize::MAX),
            Some(usize::MAX),
            Axis::Items,
            Cause::OwnedOverflow,
        ),
        (Some(0), Some(usize::MAX), Axis::Bytes, Cause::OwnedOverflow),
    ];
    for (items, bytes, axis, cause) in cases {
        let native = Native::transform(carrier()).unwrap();
        let mut attempt = native.attempt().unwrap();
        let before = native.snapshot();
        assert!(matches!(
            attempt.retain_checked(items, bytes),
            Err(rawr_import::surface::Error::Allocation)
        ));
        let after = native.snapshot();
        assert_eq!(after.current, before.current);
        assert_eq!(after.peak, before.peak);
        let refusal = after.first_refusal.unwrap();
        assert_eq!((refusal.axis, refusal.cause), (axis, cause));
        assert_eq!(refusal.current, before.current);
        assert_eq!(refusal.requested, None);
    }
    for axis in [Axis::Items, Axis::Bytes] {
        let native = Native::transform(carrier()).unwrap();
        let mut attempt = native.attempt().unwrap();
        let before = native.snapshot();
        let (items, bytes) = match axis {
            Axis::Items => (usize::MAX - attempt.items, 0),
            Axis::Bytes => (0, usize::MAX - attempt.bytes),
        };
        assert!(matches!(
            attempt.retain_checked(Some(items), Some(bytes)),
            Err(rawr_import::surface::Error::Allocation)
        ));
        let after = native.snapshot();
        assert_eq!(after.current, before.current);
        let refusal = after.first_refusal.unwrap();
        assert_eq!(
            (refusal.axis, refusal.cause),
            (axis, Cause::AggregateOverflow)
        );
        assert_eq!(refusal.requested, None);
    }
}

#[test]
fn native_account_baseline_survives_native_until_last_receipt() {
    let native = Native::transform(carrier()).unwrap();
    let attempt = native.attempt().unwrap();
    let account = Arc::clone(&attempt.account);
    let full = native.usage();
    let baseline = Arc::downgrade(&native.inner);
    let receipt = attempt.finish();
    assert_eq!(
        std::mem::size_of::<Receipt>(),
        std::mem::size_of::<Attempt>()
    );
    drop(native);
    {
        let state = lock(&account);
        assert_eq!((state.items, state.bytes), full);
    }
    assert!(baseline.upgrade().is_some());
    drop(receipt);
    assert!(baseline.upgrade().is_none());
    let state = lock(&account);
    assert_eq!((state.items, state.bytes), (1, shared_bytes()));
}

#[test]
fn native_account_simultaneous_scopes_charge_one_aggregate() {
    let native = Native::transform(carrier()).unwrap();
    let baseline = native.usage();
    let barrier = Barrier::new(3);
    std::thread::scope(|scope| {
        let spawn = || {
            scope.spawn(|| {
                let attempt = native.attempt();
                barrier.wait();
                barrier.wait();
                attempt.is_ok()
            })
        };
        let first = spawn();
        let second = spawn();
        barrier.wait();
        // Both scopes are charged to the one aggregate.
        assert_eq!(
            native.usage(),
            (
                baseline.0 + 2,
                baseline.1 + 2 * std::mem::size_of::<Attempt>()
            )
        );
        assert_eq!(native.snapshot().first_refusal, None);
        barrier.wait();
        assert!(first.join().unwrap());
        assert!(second.join().unwrap());
    });
    assert_eq!(native.usage(), baseline);
    assert_eq!(native.snapshot().first_refusal, None);
}

fn plan() -> Vec<u8> {
    let constructor = identity::Selector::StructConstructor(identity::Form::Unit);
    let producer = identity::Selector::Function {
        item: identity::Definition::authenticated([21; 32]),
    };
    let source = [11; 32];
    let target = [12; 32];
    serde_json::to_vec(&serde_json::json!({
        "schema": 25, "aliases": [], "sources": [], "abis": [], "operations": [],
        "facades": [
            {"root": "native", "types": [{"path": "Source", "identity": source, "parameters": [], "copy": false, "default": false, "debug": false, "clone": false,
                "shape": {"Struct": {"form": "Unit", "constructor": constructor, "fields": [], "constructible": true}}}],
             "items": [{"path": "produce", "kind": "Function", "selector": producer,
                "signature": {"parameters": [], "bounds": [], "inputs": [], "output": {"Nominal": {"identity": source, "arguments": []}}}}], "traits": []},
            {"root": "target", "types": [{"path": "Event", "identity": target, "parameters": ["Message"], "copy": false, "default": false, "debug": false, "clone": false,
                "shape": {"Struct": {"form": "Unit", "constructor": null, "fields": [], "constructible": false}}}], "items": [], "traits": []}
        ]
    }))
    .unwrap()
}

#[test]
fn native_account_retains_bindings_structural_and_event_metadata() {
    let carrier = carrier_entries(&[([31; 32], [32; 32], 0), ([41; 32], [42; 32], 0)]);
    let mut native = Native::transform(carrier).unwrap();
    let initial = native.usage();
    let mut engine = Engine::new();
    let definitions = runner::install_retained(&mut engine, &plan(), &mut native).unwrap();
    let extended = native.usage();
    assert!(extended.0 > initial.0 && extended.1 > initial.1);
    let row = runner::StructuralRow::new(
        "native",
        "",
        identity::Selector::StructConstructor(identity::Form::Unit).fingerprint(),
        [31; 32],
        [32; 32],
        0,
        runner::StructuralKind::Constructor,
    );
    runner::admit_generated_structural_retained(&mut engine, &definitions, &[row], &mut native)
        .unwrap();
    let structural = native.usage();
    assert_eq!(structural.0, extended.0 + 1);
    let bindings =
        runner::authenticate_generated_structural_retained(&definitions, &[row], &mut native)
            .unwrap();
    assert_eq!(bindings.len(), 1);
    let bound = native.usage();
    assert_eq!(bound.0, structural.0 + 1);
    let producer = identity::Selector::Function {
        item: identity::Definition::authenticated([21; 32]),
    };
    let events = runner::authenticate_generated_events_retained(
        &definitions,
        &[runner::EventRow::new(
            "native",
            producer.fingerprint(),
            [41; 32],
            [42; 32],
            0,
            [11; 32],
            "target",
            "target::Event",
            [9; 16],
        )],
        &mut native,
    )
    .unwrap();
    assert_eq!(events.len(), 1);
    let retained = native.usage();
    assert_eq!(retained.0, bound.0 + 1);
    assert!(retained.1 > bound.1);
    let failed = native.attempt().unwrap();
    drop(failed);
    assert_eq!(native.usage(), retained);
}

fn runtime_fixture() -> (
    Engine,
    Box<[host::facade::Definition]>,
    runner::StructuralRow,
    runner::EventRow,
) {
    let carrier = carrier_entries(&[([31; 32], [32; 32], 0), ([41; 32], [42; 32], 0)]);
    let mut native = Native::transform(carrier).unwrap();
    let mut engine = Engine::new();
    let definitions = runner::install_retained(&mut engine, &plan(), &mut native).unwrap();
    engine.imported_native = Some(native);
    (
        engine,
        definitions,
        runner::StructuralRow::new(
            "native",
            "",
            identity::Selector::StructConstructor(identity::Form::Unit).fingerprint(),
            [31; 32],
            [32; 32],
            0,
            runner::StructuralKind::Constructor,
        ),
        runner::EventRow::new(
            "native",
            identity::Selector::Function {
                item: identity::Definition::authenticated([21; 32]),
            }
            .fingerprint(),
            [41; 32],
            [42; 32],
            0,
            [11; 32],
            "target",
            "target::Event",
            [9; 16],
        ),
    )
}

fn bind_runtime(
    scope: &runner::Runtime<'_>,
    definitions: &[host::facade::Definition],
) -> host::runtime::Registry {
    let mut registry = host::runtime::Registry::new();
    for definition in definitions {
        let mut builder = host::runtime::Builder::new(definition.clone());
        for adapter in scope
            .structural()
            .iter()
            .filter(|adapter| adapter.root() == definition.root())
        {
            builder
                .bind_imported_structural(adapter, |_, _| Ok(crate::Dynamic::UNIT))
                .unwrap();
        }
        for adapter in scope
            .events()
            .iter()
            .filter(|adapter| adapter.root() == definition.root())
        {
            builder
                .bind_event_replay(adapter, |_, _| Ok(crate::Dynamic::UNIT))
                .unwrap();
        }
        registry.install(builder.finish()).unwrap();
    }
    registry
}

#[test]
fn native_account_runtime_receipts_retire_scratch_and_count_overlapping_bindings() {
    let (engine, definitions, structural, event) = runtime_fixture();
    let native = engine.imported_native.as_ref().unwrap();
    let baseline = native.usage();
    let account = Arc::clone(&lock(&native.inner.allocation).account);
    let make = || {
        let mut scope = engine.begin_imported_runtime().unwrap();
        scope
            .authenticate_structural(&definitions, &[structural])
            .unwrap();
        scope
            .authenticate_events(&definitions, std::slice::from_ref(&event))
            .unwrap();
        let during = native.usage();
        let registry = bind_runtime(&scope, &definitions);
        let registry = scope.finish(registry).unwrap();
        let after = native.usage();
        assert!(after.0 < during.0 && after.1 < during.1);
        registry
    };
    let first = make();
    let one = native.usage();
    assert_eq!(one.0, baseline.0 + 3); // Receipt plus the two retained proof rows.
    let second = make();
    assert_eq!(
        native.usage(),
        (
            baseline.0 + 2 * (one.0 - baseline.0),
            baseline.1 + 2 * (one.1 - baseline.1)
        )
    );
    drop(second);
    assert_eq!(native.usage(), one);
    drop(engine);
    {
        let state = lock(&account);
        assert_eq!((state.items, state.bytes), one);
    }
    drop(first);
    let state = lock(&account);
    assert_eq!((state.items, state.bytes), (1, shared_bytes()));
}

#[test]
fn native_account_failed_runtime_binding_releases_only_its_attempt() {
    let (engine, definitions, structural, event) = runtime_fixture();
    let native = engine.imported_native.as_ref().unwrap();
    let baseline = native.usage();
    let mut live = engine.begin_imported_runtime().unwrap();
    live.authenticate_structural(&definitions, &[structural])
        .unwrap();
    live.authenticate_events(&definitions, std::slice::from_ref(&event))
        .unwrap();
    let registry = bind_runtime(&live, &definitions);
    let live = live.finish(registry).unwrap();
    let retained = native.usage();
    {
        let mut failed = engine.begin_imported_runtime().unwrap();
        failed
            .authenticate_structural(&definitions, &[structural])
            .unwrap();
        failed
            .authenticate_events(&definitions, std::slice::from_ref(&event))
            .unwrap();
        let source = definitions
            .iter()
            .find(|definition| definition.root() == "native")
            .unwrap();
        let mut builder = host::runtime::Builder::new(source.clone());
        let adapter = &failed.structural()[0];
        builder
            .bind_imported_structural(adapter, |_, _| Ok(crate::Dynamic::UNIT))
            .unwrap();
        assert!(matches!(
            builder.bind_imported_structural(adapter, |_, _| Ok(crate::Dynamic::UNIT)),
            Err(host::runtime::Error::DuplicateStructuralBinding { .. })
        ));
    }
    assert_eq!(native.usage(), retained);
    drop(live);
    assert_eq!(native.usage(), baseline);
}

#[test]
fn native_account_runtime_cannot_finish_incomplete_authentication() {
    let mut engine = Engine::new();
    engine.imported_native = Some(Native::transform(carrier()).unwrap());
    let native = engine.imported_native.as_ref().unwrap();
    let before = native.usage();
    let scope = engine.begin_imported_runtime().unwrap();
    assert!(scope.finish(host::runtime::Registry::new()).is_err());
    assert_eq!(native.usage(), before);
}

#[test]
fn native_account_structural_array_is_charged_before_row_materialization() {
    let (engine, definitions, structural, _) = runtime_fixture();
    let native = engine.imported_native.as_ref().unwrap();
    let baseline = native.usage();
    let mut attempt = native.attempt().unwrap();
    let before = attempt.usage();
    let array_bytes = std::mem::size_of::<host::runtime::StructuralAdapter>();
    // The output array is charged before any row is processed. A row the
    // empty carrier lacks is a rawr bug, so it panics after the charge.
    let missing = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ =
            runner::authenticate_structural(&definitions, &carrier(), &[structural], &mut attempt);
    }))
    .expect_err("a structural row outside the manifest panics");
    assert_eq!(
        missing.downcast_ref::<&str>(),
        Some(&"each structural adapter is in the runner's manifest")
    );
    assert_eq!(attempt.usage(), (before.0 + 1, before.1 + array_bytes));
    assert_eq!(native.snapshot().first_refusal, None);
    drop(attempt);
    assert_eq!(native.usage(), baseline);
}

/// A checked-arithmetic refusal is terminal for the shared account. A
/// later small charge on the same scope, a fresh attempt, an observation and
/// a reconstruction sharing the account are all refused without charging, and
/// the first cause stays; cleanup (dropping scopes) still releases usage.
#[test]
fn arithmetic_refusal_suppresses_every_later_admission_and_keeps_the_first_cause() {
    let native = Native::transform(carrier()).unwrap();
    let mut first = native.attempt().unwrap();
    let mut second = native.attempt().unwrap();
    let before = native.snapshot();
    assert_eq!(before.first_refusal, None);
    // A genuine cause: the producer could not represent the item amount.
    assert!(matches!(
        first.retain_checked(None, Some(1)),
        Err(rawr_import::surface::Error::Allocation)
    ));
    let refused = native.snapshot();
    let cause = refused.first_refusal.unwrap();
    assert_eq!(
        (cause.axis, cause.cause),
        (Axis::Items, Cause::MissingAmount)
    );
    assert_eq!(refused.current, before.current);
    // Small, representable charges are refused on both scopes and charge nothing.
    for attempt in [&mut first, &mut second] {
        assert!(matches!(
            attempt.retain_checked(Some(1), Some(1)),
            Err(rawr_import::surface::Error::Allocation)
        ));
    }
    // A later arithmetic failure on another axis does not replace the cause.
    assert!(matches!(
        second.retain_checked(Some(0), Some(usize::MAX)),
        Err(rawr_import::surface::Error::Allocation)
    ));
    // A fresh attempt and a reconstruction sharing the account are refused.
    assert!(matches!(
        native.attempt(),
        Err(rawr_import::surface::Error::Allocation)
    ));
    let (reconstructed, observation) = native.reconstruct_observed(&carrier_bytes(&[]));
    assert!(reconstructed.is_err());
    assert_eq!(
        observation.snapshot().unwrap().first_refusal,
        Some(cause),
        "the refused reconstruction reports the first cause"
    );
    let after = native.snapshot();
    assert_eq!(after.current, before.current, "nothing was charged");
    assert_eq!(after.peak, before.peak);
    assert_eq!(after.first_refusal, Some(cause), "the first cause stays");
    // Cleanup only: dropping the scopes still releases their contributions.
    let scope = std::mem::size_of::<Attempt>();
    drop(first);
    drop(second);
    assert_eq!(
        native.snapshot().current,
        [before.current[0] - 2, before.current[1] - 2 * scope]
    );
    assert_eq!(native.snapshot().first_refusal, Some(cause));
}
