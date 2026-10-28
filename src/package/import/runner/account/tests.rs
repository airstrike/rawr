use super::{
    baseline_bytes, lock, shared_bytes, Attempt, Axis, Cause, Native, Observation,
    ObservationState, Receipt, BYTES_RESOURCE, ITEMS_RESOURCE,
};
use crate::{host, host::ImportAllocation as _, package::import::runner, Engine};
use rawr_import::{exceedance, runner::manifest::Carrier, surface::identity};
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

/// Empty native metadata with only the requested remaining resource headroom.
///
/// The headroom is an accounting target: charges beyond it are
/// admitted and recorded, never refused.
pub(crate) fn native_with_headroom(items: usize, bytes: usize) -> Native {
    let native = Native::transform(carrier()).unwrap();
    let (base_items, base_bytes) = native.usage();
    Native::with_limits(carrier(), base_items + items, base_bytes + bytes).unwrap()
}

/// The `(target, first_requested, high_water)` triple of one recorded axis.
fn crossing(axis: &exceedance::Axis) -> (u64, u64, u64) {
    (axis.target, axis.first_requested, axis.high_water)
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
    assert!(!native.snapshot().exceeded.is_exceeded());
    let (items, bytes) = native.usage();
    // Exact targets admit silently; a target one below the seeded usage admits
    // the same usage and records the truthful aggregate.
    let exact = Native::with_limits(self::carrier(), items, bytes).unwrap();
    assert_eq!(exact.usage(), (items, bytes));
    assert!(!exact.snapshot().exceeded.is_exceeded());
    let over_items = Native::with_limits(self::carrier(), items - 1, bytes).unwrap();
    assert_eq!(over_items.usage(), (items, bytes));
    let snapshot = over_items.snapshot();
    assert_eq!(snapshot.first_refusal, None);
    assert_eq!(snapshot.limits, [items - 1, bytes]);
    assert_eq!(
        crossing(snapshot.exceeded.axis(ITEMS_RESOURCE).unwrap()),
        ((items - 1) as u64, items as u64, items as u64)
    );
    assert!(snapshot.exceeded.axis(BYTES_RESOURCE).is_none());
    let over_bytes = Native::with_limits(self::carrier(), items, bytes - 1).unwrap();
    assert_eq!(over_bytes.usage(), (items, bytes));
    let snapshot = over_bytes.snapshot();
    assert_eq!(snapshot.first_refusal, None);
    assert_eq!(
        crossing(snapshot.exceeded.axis(BYTES_RESOURCE).unwrap()),
        ((bytes - 1) as u64, bytes as u64, bytes as u64)
    );
    assert!(snapshot.exceeded.axis(ITEMS_RESOURCE).is_none());
}

#[test]
fn native_account_overlapping_receipts_add_and_dropped_attempt_releases_recorded_excess() {
    let native = native_with_headroom(12, 2 * std::mem::size_of::<Attempt>() + 100);
    let baseline = native.usage();
    let mut live = native.attempt().unwrap();
    live.retain_checked(Some(4), Some(40)).unwrap();
    let live = live.finish();
    let mut pending = native.attempt().unwrap();
    pending.retain_checked(Some(6), Some(60)).unwrap();
    let full = native.usage();
    assert_eq!(full.0, baseline.0 + live.usage().0 + pending.usage().0);
    assert_eq!(full.1, baseline.1 + live.usage().1 + pending.usage().1);
    assert_eq!(native.snapshot().limits, [full.0, full.1]);
    assert!(!native.snapshot().exceeded.is_exceeded());
    // One charge past each exact target is admitted and recorded, never refused.
    pending.retain_checked(Some(1), Some(0)).unwrap();
    pending.retain_checked(Some(0), Some(1)).unwrap();
    assert_eq!(native.usage(), (full.0 + 1, full.1 + 1));
    let crossed = native.snapshot();
    assert_eq!(crossed.first_refusal, None);
    assert_eq!(
        crossing(crossed.exceeded.axis(ITEMS_RESOURCE).unwrap()),
        (full.0 as u64, full.0 as u64 + 1, full.0 as u64 + 1)
    );
    assert_eq!(
        crossing(crossed.exceeded.axis(BYTES_RESOURCE).unwrap()),
        (full.1 as u64, full.1 as u64 + 1, full.1 as u64 + 1)
    );
    drop(pending);
    assert_eq!(
        native.usage(),
        (baseline.0 + live.usage().0, baseline.1 + live.usage().1)
    );
    assert_eq!(native.snapshot().exceeded, crossed.exceeded);
    drop(live);
    assert_eq!(native.usage(), baseline);
    assert_eq!(native.snapshot().exceeded, crossed.exceeded);
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
        assert!(!native.snapshot().exceeded.is_exceeded());
    }
}

#[test]
fn native_account_zero_headroom_admits_a_scope_and_records_both_axes() {
    let no_room = native_with_headroom(0, 0);
    let before = no_room.usage();
    assert!(!no_room.snapshot().exceeded.is_exceeded());
    let scope = no_room.attempt().unwrap();
    assert_eq!(
        no_room.usage(),
        (before.0 + 1, before.1 + std::mem::size_of::<Attempt>())
    );
    let crossed = no_room.snapshot();
    assert_eq!(crossed.first_refusal, None);
    assert_eq!(
        crossing(crossed.exceeded.axis(ITEMS_RESOURCE).unwrap()),
        (before.0 as u64, before.0 as u64 + 1, before.0 as u64 + 1)
    );
    assert_eq!(
        crossing(crossed.exceeded.axis(BYTES_RESOURCE).unwrap()),
        (
            before.1 as u64,
            (before.1 + std::mem::size_of::<Attempt>()) as u64,
            (before.1 + std::mem::size_of::<Attempt>()) as u64
        )
    );
    drop(scope);
    assert_eq!(no_room.usage(), before);
    assert_eq!(no_room.snapshot().exceeded, crossed.exceeded);
}

#[test]
fn observed_native_decode_charges_only_its_fixed_slot_and_keeps_the_same_account() {
    let ordinary = Native::decode(&carrier_bytes(&[])).unwrap();
    let (native, observation) = Native::decode_observed(&carrier_bytes(&[]));
    let native = native.unwrap();
    let snapshot = observation.snapshot().unwrap();
    assert_eq!(snapshot, native.snapshot());
    assert_ne!(snapshot.account, ordinary.snapshot().account);
    assert!(snapshot.account.is_some());
    assert_eq!(
        snapshot.current,
        [
            ordinary.usage().0 + 1,
            ordinary.usage().1 + std::mem::size_of::<Observation>()
        ]
    );
    assert_eq!(snapshot.peak, snapshot.current);
    assert_eq!(snapshot.first_refusal, None);
    assert!(!snapshot.exceeded.is_exceeded());
    drop(observation);
    assert_eq!(native.usage(), ordinary.usage());
    assert_eq!(native.snapshot().peak, snapshot.peak);
    let engine = Engine::new().retain_imported_native(native).unwrap();
    assert_eq!(
        engine.imported_native_snapshot().unwrap().account,
        snapshot.account
    );
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
    assert_eq!(overlapping.account, initial.account);
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
    assert_eq!(next.snapshot().account, initial.account);
    drop(next);
    assert_eq!(
        next_observation.snapshot().unwrap().current,
        [2, shared_bytes() + std::mem::size_of::<Observation>(),]
    );
}

#[test]
fn reconstructed_native_overlap_cannot_reset_original_limits_and_records_exceedance() {
    let (items, bytes) = carrier().retained_usage();
    let native = native_with_headroom(
        items + 2,
        bytes + baseline_bytes() + std::mem::size_of::<Observation>(),
    );
    let before = native.snapshot();
    let (replacement, observation) = native.reconstruct_observed(&carrier_bytes(&[]));
    let replacement = replacement.unwrap();
    assert_eq!(replacement.snapshot().current, before.limits);
    assert!(!replacement.snapshot().exceeded.is_exceeded());
    // A further overlapping scope crosses both exact targets: it is admitted and
    // recorded on the one shared account, and no refusal latches.
    let scope = replacement.attempt().unwrap();
    let crossed = replacement.snapshot();
    assert_eq!(crossed.first_refusal, None);
    assert_eq!(
        crossed.current,
        [
            before.limits[0] + 1,
            before.limits[1] + std::mem::size_of::<Attempt>()
        ]
    );
    assert_eq!(
        crossing(crossed.exceeded.axis(ITEMS_RESOURCE).unwrap()),
        (
            before.limits[0] as u64,
            crossed.current[0] as u64,
            crossed.current[0] as u64
        )
    );
    assert_eq!(
        crossing(crossed.exceeded.axis(BYTES_RESOURCE).unwrap()),
        (
            before.limits[1] as u64,
            crossed.current[1] as u64,
            crossed.current[1] as u64
        )
    );
    assert_eq!(native.snapshot(), crossed);
    drop(scope);
    drop(replacement);
    drop(observation);
    assert_eq!(native.snapshot().current, before.current);
    assert_eq!(native.snapshot().limits, before.limits);
    assert_eq!(native.snapshot().first_refusal, None);
    assert_eq!(native.snapshot().exceeded, crossed.exceeded);

    // Only the observation slot fits: the baseline charge is the first crossing.
    let native = native_with_headroom(1, std::mem::size_of::<Observation>());
    let before = native.snapshot();
    let (replacement, observation) = native.reconstruct_observed(&carrier_bytes(&[]));
    let replacement = replacement.unwrap();
    let crossed = native.snapshot();
    assert_eq!(observation.snapshot(), Some(crossed));
    assert_eq!(replacement.snapshot(), crossed);
    assert_eq!(crossed.first_refusal, None);
    assert_eq!(
        crossed.current,
        [
            before.current[0] + 1 + items + 1,
            before.current[1] + std::mem::size_of::<Observation>() + bytes + baseline_bytes(),
        ]
    );
    assert_eq!(
        crossing(crossed.exceeded.axis(ITEMS_RESOURCE).unwrap()),
        (
            before.limits[0] as u64,
            crossed.current[0] as u64,
            crossed.current[0] as u64
        )
    );
    assert_eq!(
        crossing(crossed.exceeded.axis(BYTES_RESOURCE).unwrap()),
        (
            before.limits[1] as u64,
            crossed.current[1] as u64,
            crossed.current[1] as u64
        )
    );
    drop(replacement);
    assert_eq!(
        native.snapshot().current,
        [
            before.current[0] + 1,
            before.current[1] + std::mem::size_of::<Observation>(),
        ]
    );
    drop(observation);
    assert_eq!(native.snapshot().current, before.current);
    assert_eq!(native.snapshot().first_refusal, None);
    assert_eq!(native.snapshot().exceeded, crossed.exceeded);

    // No headroom at all: the observation slot is the first crossing and the
    // baseline charge raises the high water further.
    let native = native_with_headroom(0, 0);
    let before = native.snapshot();
    let (replacement, observation) = native.reconstruct_observed(&carrier_bytes(&[]));
    let replacement = replacement.unwrap();
    let crossed = native.snapshot();
    assert_eq!(observation.snapshot(), Some(crossed));
    assert_eq!(crossed.first_refusal, None);
    assert_eq!(
        crossing(crossed.exceeded.axis(ITEMS_RESOURCE).unwrap()),
        (
            before.current[0] as u64,
            before.current[0] as u64 + 1,
            crossed.current[0] as u64
        )
    );
    assert_eq!(
        crossing(crossed.exceeded.axis(BYTES_RESOURCE).unwrap()),
        (
            before.current[1] as u64,
            (before.current[1] + std::mem::size_of::<Observation>()) as u64,
            crossed.current[1] as u64
        )
    );
    assert!(crossed.current[0] > before.current[0] + 1);
    drop(replacement);
    drop(observation);
    assert_eq!(native.snapshot().current, before.current);
    assert_eq!(native.snapshot().first_refusal, None);
    assert_eq!(native.snapshot().exceeded, crossed.exceeded);
}

#[test]
fn reconstructed_native_preserves_decode_error_and_requires_original_ownership() {
    let engine = Engine::new();
    let (replacement, observation) = engine.reconstruct_imported_native(&carrier_bytes(&[]));
    assert!(matches!(replacement, Err(runner::Error::NativeOwner)));
    assert!(observation.snapshot().is_none());

    let native = native_with_headroom(0, 0);
    let before = native.snapshot();
    let engine = engine.retain_imported_native(native).unwrap();
    let (replacement, observation) = engine.reconstruct_imported_native(b"invalid manifest");
    assert!(matches!(replacement, Err(runner::Error::Manifest(_))));
    assert_eq!(observation.snapshot(), Some(before));
    assert_eq!(engine.imported_native_snapshot(), Some(before));
}

#[test]
fn observed_native_constructor_preserves_unreached_and_records_each_reached_exceedance() {
    let (result, observation) = Native::decode_observed(b"invalid carrier");
    let ordinary = Native::decode(b"invalid carrier");
    assert!(matches!(result, Err(runner::Error::Manifest(_))));
    assert_eq!(
        result.err().unwrap().to_string(),
        ordinary.err().unwrap().to_string()
    );
    assert!(observation.snapshot().is_none());

    // Shared storage, the observation slot and the decoded baseline, in charge order.
    let (items, bytes) = carrier().retained_usage();
    let full = [
        items + 3,
        shared_bytes() + std::mem::size_of::<Observation>() + bytes + baseline_bytes(),
    ];

    // An item target below the shared storage: that storage is the first crossing.
    let (result, observation) = Native::with_limits_observed(carrier(), 0, usize::MAX);
    let native = result.unwrap();
    assert!(matches!(observation.state, ObservationState::Retained(_)));
    let snapshot = observation.snapshot().unwrap();
    assert_eq!(snapshot, native.snapshot());
    assert_eq!(snapshot.current, full);
    assert_eq!(snapshot.peak, full);
    assert_eq!(snapshot.first_refusal, None);
    assert_eq!(
        crossing(snapshot.exceeded.axis(ITEMS_RESOURCE).unwrap()),
        (0, 1, full[0] as u64)
    );
    assert!(snapshot.exceeded.axis(BYTES_RESOURCE).is_none());

    // A byte target below the shared storage.
    let (result, observation) =
        Native::with_limits_observed(carrier(), usize::MAX, shared_bytes() - 1);
    let native = result.unwrap();
    let snapshot = observation.snapshot().unwrap();
    assert_eq!(snapshot, native.snapshot());
    assert_eq!(snapshot.current, full);
    assert_eq!(snapshot.first_refusal, None);
    assert_eq!(
        crossing(snapshot.exceeded.axis(BYTES_RESOURCE).unwrap()),
        (
            (shared_bytes() - 1) as u64,
            shared_bytes() as u64,
            full[1] as u64
        )
    );
    assert!(snapshot.exceeded.axis(ITEMS_RESOURCE).is_none());

    // Shared storage meets the target; the single observation slot is the crossing.
    let (result, observation) = Native::with_limits_observed(carrier(), 1, usize::MAX);
    let native = result.unwrap();
    assert!(matches!(observation.state, ObservationState::Retained(_)));
    let snapshot = observation.snapshot().unwrap();
    assert_eq!(snapshot, native.snapshot());
    assert_eq!(snapshot.current, full);
    assert_eq!(snapshot.first_refusal, None);
    assert_eq!(
        crossing(snapshot.exceeded.axis(ITEMS_RESOURCE).unwrap()),
        (1, 2, full[0] as u64)
    );

    // Storage and slot meet the target; the baseline admission is the crossing.
    let (result, observation) = Native::with_limits_observed(carrier(), 2, usize::MAX);
    let native = result.unwrap();
    assert!(matches!(observation.state, ObservationState::Retained(_)));
    let snapshot = observation.snapshot().unwrap();
    assert_eq!(snapshot, native.snapshot());
    assert_eq!(snapshot.current, full);
    assert_eq!(snapshot.first_refusal, None);
    let axis = *snapshot.exceeded.axis(ITEMS_RESOURCE).unwrap();
    assert_eq!(crossing(&axis), (2, full[0] as u64, full[0] as u64));
    assert!(axis.first_requested > 2);
    // The slot and the record outlive the native owner.
    drop(native);
    let after = observation.snapshot().unwrap();
    assert_eq!(
        after.current,
        [2, shared_bytes() + std::mem::size_of::<Observation>()]
    );
    assert_eq!(after.exceeded, snapshot.exceeded);
}

#[test]
fn observed_native_keeps_overlap_peak_and_exceedance_after_aborted_attempt() {
    let (native, observation) = Native::with_limits_observed(carrier(), usize::MAX, usize::MAX);
    let baseline = native.unwrap().usage();
    drop(observation);
    let (native, observation) = Native::with_limits_observed(
        carrier(),
        baseline.0 + 12,
        baseline.1 + 2 * std::mem::size_of::<Attempt>() + 100,
    );
    let native = native.unwrap();
    let mut live = native.attempt().unwrap();
    live.retain_checked(Some(4), Some(40)).unwrap();
    let live = live.finish();
    let mut pending = native.attempt().unwrap();
    pending.retain_checked(Some(6), Some(60)).unwrap();
    let full = native.snapshot();
    assert_eq!(full.current, full.limits);
    assert_eq!(full.peak, full.current);
    assert!(!full.exceeded.is_exceeded());
    // The first charge past the item target is admitted and recorded alone.
    pending.retain_checked(Some(1), Some(0)).unwrap();
    let first = observation.snapshot().unwrap();
    assert_eq!(first.first_refusal, None);
    assert_eq!(
        crossing(first.exceeded.axis(ITEMS_RESOURCE).unwrap()),
        (
            full.current[0] as u64,
            full.current[0] as u64 + 1,
            full.current[0] as u64 + 1
        )
    );
    assert!(first.exceeded.axis(BYTES_RESOURCE).is_none());
    // A later charge raises the item high water and crosses the byte target.
    pending.retain_checked(Some(1), Some(1)).unwrap();
    let crossed = observation.snapshot().unwrap();
    assert_eq!(crossed.first_refusal, None);
    assert_eq!(crossed.current, [full.current[0] + 2, full.current[1] + 1]);
    assert_eq!(crossed.peak, crossed.current);
    assert_eq!(
        crossing(crossed.exceeded.axis(ITEMS_RESOURCE).unwrap()),
        (
            full.current[0] as u64,
            full.current[0] as u64 + 1,
            full.current[0] as u64 + 2
        )
    );
    assert_eq!(
        crossing(crossed.exceeded.axis(BYTES_RESOURCE).unwrap()),
        (
            full.current[1] as u64,
            full.current[1] as u64 + 1,
            full.current[1] as u64 + 1
        )
    );
    drop(pending);
    let after = observation.snapshot().unwrap();
    assert_eq!(
        after.current,
        [baseline.0 + live.usage().0, baseline.1 + live.usage().1]
    );
    assert_eq!(after.peak, crossed.peak);
    assert_eq!(after.exceeded, crossed.exceeded);
    assert_eq!(after.first_refusal, None);
    drop(live);
    assert_eq!(
        observation.snapshot().unwrap().current,
        [baseline.0, baseline.1]
    );
    assert_eq!(observation.snapshot().unwrap().peak, crossed.peak);
    assert_eq!(observation.snapshot().unwrap().exceeded, crossed.exceeded);
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
    assert_eq!(after.account, before.account);
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
        assert!(!after.exceeded.is_exceeded());
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
        assert!(!after.exceeded.is_exceeded());
        let refusal = after.first_refusal.unwrap();
        assert_eq!(
            (refusal.axis, refusal.cause),
            (axis, Cause::AggregateOverflow)
        );
        assert_eq!(refusal.requested, None);
    }
}

#[test]
fn observation_identity_exhaustion_stays_unknown_without_changing_account_records() {
    // Runner identities come from the one process-wide exceedance id space,
    // so a runner account can never share a registry key with another account.
    let before = exceedance::next_id();
    let native = native_with_headroom(2, std::mem::size_of::<Attempt>() + 1);
    let after = exceedance::next_id();
    let identity = native.snapshot().account.unwrap().get();
    assert!(before < identity && identity < after);
    lock(&lock(&native.inner.allocation).account).identity = None;
    let mut attempt = native.attempt().unwrap();
    attempt.retain_checked(Some(1), Some(1)).unwrap();
    let full = native.snapshot();
    assert!(full.account.is_none());
    assert_eq!(full.current, full.limits);
    assert!(!full.exceeded.is_exceeded());
    // The crossing is recorded on the anonymous account exactly as on a named one.
    attempt.retain_checked(Some(1), Some(0)).unwrap();
    let crossed = native.snapshot();
    assert!(crossed.account.is_none());
    assert_eq!(crossed.first_refusal, None);
    assert_eq!(crossed.current, [full.current[0] + 1, full.current[1]]);
    assert_eq!(
        crossing(crossed.exceeded.axis(ITEMS_RESOURCE).unwrap()),
        (
            full.limits[0] as u64,
            full.limits[0] as u64 + 1,
            full.limits[0] as u64 + 1
        )
    );
    // A genuine arithmetic refusal still latches without an identity.
    assert!(matches!(
        attempt.retain_checked(None, Some(0)),
        Err(rawr_import::surface::Error::Allocation)
    ));
    let refusal = native.snapshot().first_refusal.unwrap();
    assert_eq!(
        (refusal.axis, refusal.cause),
        (Axis::Items, Cause::MissingAmount)
    );
    assert_eq!(native.snapshot().exceeded, crossed.exceeded);
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
fn native_account_simultaneous_scopes_cannot_partition_the_aggregate_target() {
    let native = native_with_headroom(1, std::mem::size_of::<Attempt>());
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
        // Both scopes are charged to the one aggregate: whichever came second
        // crossed the shared target, and the record says so truthfully.
        assert_eq!(
            native.usage(),
            (
                baseline.0 + 2,
                baseline.1 + 2 * std::mem::size_of::<Attempt>()
            )
        );
        let crossed = native.snapshot();
        assert_eq!(crossed.first_refusal, None);
        assert_eq!(
            crossing(crossed.exceeded.axis(ITEMS_RESOURCE).unwrap()),
            (
                (baseline.0 + 1) as u64,
                (baseline.0 + 2) as u64,
                (baseline.0 + 2) as u64
            )
        );
        assert_eq!(
            crossing(crossed.exceeded.axis(BYTES_RESOURCE).unwrap()),
            (
                (baseline.1 + std::mem::size_of::<Attempt>()) as u64,
                (baseline.1 + 2 * std::mem::size_of::<Attempt>()) as u64,
                (baseline.1 + 2 * std::mem::size_of::<Attempt>()) as u64
            )
        );
        barrier.wait();
        assert!(first.join().unwrap());
        assert!(second.join().unwrap());
    });
    assert_eq!(native.usage(), baseline);
    assert!(native.snapshot().exceeded.is_exceeded());
    assert_eq!(native.snapshot().first_refusal, None);
}

fn plan() -> rawr_import::surface::facade::Plan {
    let constructor = identity::Selector::StructConstructor(identity::Form::Unit);
    let producer = identity::Selector::Function {
        item: identity::Definition::authenticated([21; 32]),
    };
    let source = [11; 32];
    let target = [12; 32];
    let bytes = serde_json::to_vec(&serde_json::json!({
        "schema": 17, "aliases": [], "sources": [], "abis": [], "operations": [],
        "facades": [
            {"root": "native", "types": [{"path": "Source", "identity": source, "parameters": [], "copy": false, "default": false, "debug": false, "clone": false,
                "shape": {"Struct": {"form": "Unit", "constructor": constructor, "fields": [], "constructible": true}}}],
             "items": [{"path": "produce", "kind": "Function", "selector": producer,
                "signature": {"parameters": [], "bounds": [], "inputs": [], "output": {"Nominal": {"identity": source, "arguments": []}}}}], "traits": []},
            {"root": "target", "types": [{"path": "Event", "identity": target, "parameters": ["Message"], "copy": false, "default": false, "debug": false, "clone": false,
                "shape": {"Struct": {"form": "Unit", "constructor": null, "fields": [], "constructible": false}}}], "items": [], "traits": []}
        ]
    })).unwrap();
    rawr_import::surface::facade::Plan::decode(&bytes, &rawr_import::cargo::Limits::standard())
        .unwrap()
}

#[test]
fn native_account_retains_facades_extensions_structural_and_event_metadata() {
    let carrier = carrier_entries(&[([31; 32], [32; 32], 0), ([41; 32], [42; 32], 0)]);
    let mut native = Native::transform(carrier).unwrap();
    let initial = native.usage();
    let mut engine = Engine::new();
    let definitions = runner::install_decoded_profile_retained(
        &mut engine,
        &plan(),
        &runner::Ordinary,
        &mut native,
    )
    .unwrap();
    let facades = native.usage();
    assert!(facades.0 > initial.0 && facades.1 > initial.1);
    let extension = host::facade::Builder::new("extension", host::facade::Abi::new(1).unwrap())
        .unwrap()
        .finish()
        .unwrap();
    runner::install_profile_facade_retained(&mut engine, extension, &mut native).unwrap();
    let extended = native.usage();
    assert!(extended.0 > facades.0 && extended.1 > facades.1);
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
    let definitions = runner::install_decoded_profile_retained(
        &mut engine,
        &plan(),
        &runner::Ordinary,
        &mut native,
    )
    .unwrap();
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
fn native_account_runtime_scope_records_exceedance_and_types_only_arithmetic_refusal() {
    // The retained scope meets the target but the authentication scratch crosses
    // it: the runtime scope is admitted and the crossing recorded.
    let mut engine = Engine::new();
    engine.imported_native = Some(native_with_headroom(1, std::mem::size_of::<Attempt>()));
    let native = engine.imported_native.as_ref().unwrap();
    let before = native.usage();
    let scope = engine.begin_imported_runtime().unwrap();
    assert_eq!(
        native.usage(),
        (
            before.0 + 3,
            before.1 + std::mem::size_of::<runner::Runtime<'_>>()
        )
    );
    let crossed = native.snapshot();
    assert_eq!(crossed.first_refusal, None);
    assert_eq!(
        crossing(crossed.exceeded.axis(ITEMS_RESOURCE).unwrap()),
        (
            (before.0 + 1) as u64,
            (before.0 + 2) as u64,
            (before.0 + 3) as u64
        )
    );
    assert_eq!(
        crossing(crossed.exceeded.axis(BYTES_RESOURCE).unwrap()),
        (
            (before.1 + std::mem::size_of::<Attempt>()) as u64,
            (before.1 + 2 * std::mem::size_of::<Attempt>()) as u64,
            (before.1 + std::mem::size_of::<runner::Runtime<'_>>()) as u64
        )
    );
    drop(scope);
    assert_eq!(native.usage(), before);
    assert_eq!(native.snapshot().exceeded, crossed.exceeded);

    // Only a checked-arithmetic failure refuses, and that refusal is typed.
    lock(&lock(&native.inner.allocation).account).items = usize::MAX;
    assert!(matches!(
        engine.begin_imported_runtime(),
        Err(host::runtime::Error::ImportedAllocation)
    ));
    lock(&lock(&native.inner.allocation).account).items = before.0;
    assert_eq!(native.usage(), before);
    let refusal = native.snapshot().first_refusal.unwrap();
    assert_eq!(
        (refusal.axis, refusal.cause),
        (Axis::Items, Cause::AggregateOverflow)
    );
    assert_eq!(native.snapshot().exceeded, crossed.exceeded);
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
fn native_account_structural_array_records_exceedance_before_row_materialization() {
    let (engine, definitions, structural, _) = runtime_fixture();
    let native = engine.imported_native.as_ref().unwrap();
    let baseline = native.usage();
    let mut attempt = native.attempt().unwrap();
    {
        let mut state = lock(&attempt.account);
        state.item_limit = state.items;
        state.byte_limit = state.bytes;
    }
    let before = attempt.usage();
    let aggregate = native.usage();
    let array_bytes = std::mem::size_of::<host::runtime::StructuralAdapter>();
    // The output array crosses both exact targets before any row is processed:
    // it is charged and recorded, not refused. The empty carrier then
    // rejects the first row for its own reason.
    assert!(matches!(
        runner::authenticate_structural(&definitions, &carrier(), &[structural], &mut attempt),
        Err(runner::Error::StructuralManifest)
    ));
    assert_eq!(attempt.usage(), (before.0 + 1, before.1 + array_bytes));
    let crossed = native.snapshot();
    assert_eq!(crossed.first_refusal, None);
    assert_eq!(
        crossing(crossed.exceeded.axis(ITEMS_RESOURCE).unwrap()),
        (
            aggregate.0 as u64,
            aggregate.0 as u64 + 1,
            aggregate.0 as u64 + 1
        )
    );
    assert_eq!(
        crossing(crossed.exceeded.axis(BYTES_RESOURCE).unwrap()),
        (
            aggregate.1 as u64,
            (aggregate.1 + array_bytes) as u64,
            (aggregate.1 + array_bytes) as u64
        )
    );
    drop(attempt);
    assert_eq!(native.usage(), baseline);
    assert_eq!(native.snapshot().exceeded, crossed.exceeded);
}

#[test]
fn native_account_structural_path_records_exceedance_after_exact_array_reservation() {
    let (engine, definitions, structural, _) = runtime_fixture();
    let native = engine.imported_native.as_ref().unwrap();
    let baseline = native.usage();
    let mut attempt = native.attempt().unwrap();
    let before = attempt.usage();
    let aggregate = native.usage();
    let array_bytes = std::mem::size_of::<host::runtime::StructuralAdapter>();
    {
        let mut state = lock(&attempt.account);
        state.item_limit = state.items + 1;
        state.byte_limit = state.bytes + array_bytes;
    }
    // The array reservation meets both targets exactly; the dynamic path storage
    // then crosses the byte target and is charged and recorded, not refused.
    let adapters = runner::authenticate_structural(
        &definitions,
        native.carrier(),
        &[structural],
        &mut attempt,
    )
    .unwrap();
    assert_eq!(adapters.len(), 1);
    let after = attempt.usage();
    assert_eq!(after.0, before.0 + 1);
    assert!(after.1 > before.1 + array_bytes);
    let path_bytes = after.1 - before.1 - array_bytes;
    let crossed = native.snapshot();
    assert_eq!(crossed.first_refusal, None);
    assert!(crossed.exceeded.axis(ITEMS_RESOURCE).is_none());
    assert_eq!(
        crossing(crossed.exceeded.axis(BYTES_RESOURCE).unwrap()),
        (
            (aggregate.1 + array_bytes) as u64,
            (aggregate.1 + array_bytes + path_bytes) as u64,
            (aggregate.1 + array_bytes + path_bytes) as u64
        )
    );
    drop(adapters);
    drop(attempt);
    assert_eq!(native.usage(), baseline);
    assert_eq!(native.snapshot().exceeded, crossed.exceeded);
}

#[test]
fn runner_account_records_target_exceedance_without_latching() {
    let (items_exact, bytes_exact) = Native::transform(carrier()).unwrap().usage();

    // Exact targets admit silently.
    let exact = Native::with_limits(carrier(), items_exact, bytes_exact).unwrap();
    assert_eq!(exact.usage(), (items_exact, bytes_exact));
    assert!(!exact.snapshot().exceeded.is_exceeded());
    assert_eq!(exact.snapshot().first_refusal, None);

    // One below: the same usage is admitted, the crossing recorded, no latch set.
    let native = Native::with_limits(carrier(), items_exact - 1, bytes_exact).unwrap();
    assert_eq!(native.usage(), (items_exact, bytes_exact));
    let recorded = native.snapshot();
    assert_eq!(recorded.first_refusal, None);
    assert_eq!(recorded.limits, [items_exact - 1, bytes_exact]);
    assert_eq!(recorded.current, [items_exact, bytes_exact]);
    assert_eq!(recorded.exceeded.axes().count(), 1);
    let axis = *recorded.exceeded.axis(ITEMS_RESOURCE).unwrap();
    assert_eq!(axis.name, ITEMS_RESOURCE);
    assert_eq!(
        crossing(&axis),
        (
            (items_exact - 1) as u64,
            items_exact as u64,
            items_exact as u64
        )
    );
    assert_eq!(recorded.exceeded.first(), Some(&axis));
    // The record is published to the process registry under this account.
    match exceedance::published(exceedance::Account::Runner, recorded.account.unwrap().get()) {
        Some(published) => assert_eq!(
            published.record.axis(ITEMS_RESOURCE).unwrap().target,
            (items_exact - 1) as u64
        ),
        None => assert_eq!(
            exceedance::snapshot().len(),
            exceedance::CAPACITY,
            "only a full process registry holds no slot for a published account"
        ),
    }

    // A later scope raises the high water by its charge and stays admitted.
    let mut attempt = native.attempt().unwrap();
    assert_eq!(
        native
            .snapshot()
            .exceeded
            .axis(ITEMS_RESOURCE)
            .unwrap()
            .high_water,
        (items_exact + 1) as u64
    );
    attempt.retain_checked(Some(1), Some(1)).unwrap();
    let raised = native.snapshot();
    assert_eq!(raised.first_refusal, None);
    assert_eq!(
        raised.current,
        [
            items_exact + 2,
            bytes_exact + std::mem::size_of::<Attempt>() + 1
        ]
    );
    assert_eq!(raised.peak, raised.current);
    assert_eq!(
        crossing(raised.exceeded.axis(ITEMS_RESOURCE).unwrap()),
        (
            (items_exact - 1) as u64,
            items_exact as u64,
            (items_exact + 2) as u64
        )
    );
    assert_eq!(
        crossing(raised.exceeded.axis(BYTES_RESOURCE).unwrap()),
        (
            bytes_exact as u64,
            (bytes_exact + std::mem::size_of::<Attempt>()) as u64,
            raised.current[1] as u64
        )
    );

    // Dropping the scope releases its usage; the record and peak stay.
    drop(attempt);
    let released = native.snapshot();
    assert_eq!(released.current, [items_exact, bytes_exact]);
    assert_eq!(released.peak, raised.peak);
    assert_eq!(released.exceeded, raised.exceeded);
    assert_eq!(released.first_refusal, None);
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
