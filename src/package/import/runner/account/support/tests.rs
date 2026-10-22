use super::{retained_bytes, Pending};
use crate::host::ImportAllocation as _;
use crate::package::import::runner::account::{self, Attempt};

#[test]
fn holder_is_precharged_and_unfinished_construction_releases_its_attempt() {
    let bytes = std::mem::size_of::<Attempt>() + retained_bytes();
    let native = account::tests::native_with_headroom(2, bytes);
    let baseline = native.usage();
    let pending = Pending::transform(native.attempt().unwrap()).unwrap();
    let owner = pending.support_owner().unwrap();
    assert_eq!(native.usage(), (baseline.0 + 2, baseline.1 + bytes));
    assert!(owner.0.get().is_none());
    drop(owner);
    drop(pending);
    assert_eq!(native.usage(), baseline);

    // Headroom one short on either axis is an accounting target. The
    // holder is still constructed and charged, the crossed axis records its
    // truthful aggregate, nothing latches, and dropping releases the charge.
    for (items, short_bytes, axis) in [
        (1, bytes, "runner retained items"),
        (2, bytes - 1, "runner retained bytes"),
    ] {
        let native = account::tests::native_with_headroom(items, short_bytes);
        let baseline = native.usage();
        let pending = Pending::transform(native.attempt().unwrap())
            .expect("a holder past its target is still constructed");
        assert_eq!(native.usage(), (baseline.0 + 2, baseline.1 + bytes));
        let snapshot = native.snapshot();
        assert_eq!(snapshot.first_refusal, None);
        let recorded = snapshot
            .exceeded
            .axis(axis)
            .expect("the crossed axis records");
        assert_eq!(recorded.high_water - recorded.target, 1);
        assert_eq!(snapshot.exceeded.axes().count(), 1);
        drop(pending);
        assert_eq!(native.usage(), baseline);
        assert!(native.snapshot().exceeded.axis(axis).is_some());
    }
}

#[test]
fn completed_clones_share_one_contribution_and_record_overlap_in_the_same_account() {
    let native = account::tests::native_with_headroom(
        5,
        2 * std::mem::size_of::<Attempt>() + 2 * retained_bytes() + 41,
    );
    let baseline = native.usage();
    let identity = native.snapshot().account;
    let mut pending = Pending::transform(native.attempt().unwrap()).unwrap();
    pending.retain_checked(Some(1), Some(41)).unwrap();
    let executable = pending.support_owner().unwrap();
    let closure = executable.clone();
    pending.finish().require(&executable);
    let live = native.usage();
    assert!(executable.0.get().is_some());
    let contribution = executable.0.get().unwrap().usage();
    assert_eq!(
        live,
        (baseline.0 + contribution.0, baseline.1 + contribution.1)
    );
    drop(executable);
    assert_eq!(native.usage(), live);

    let mut overlapping = Pending::transform(native.attempt().unwrap()).unwrap();
    let full = native.usage();
    assert_eq!(native.snapshot().account, identity);
    assert!(!native.snapshot().exceeded.is_exceeded());
    // One item past the exact aggregate target is admitted on the same
    // account, recorded with its truthful aggregate, and released on drop.
    overlapping
        .retain_checked(Some(1), Some(0))
        .expect("an overlap past the item target is charged, not refused");
    assert_eq!(native.usage(), (full.0 + 1, full.1));
    let snapshot = native.snapshot();
    assert_eq!(snapshot.first_refusal, None);
    let axis = snapshot
        .exceeded
        .axis("runner retained items")
        .expect("the item target records its axis");
    assert_eq!(
        (axis.target, axis.first_requested, axis.high_water),
        (full.0 as u64, (full.0 + 1) as u64, (full.0 + 1) as u64)
    );
    drop(overlapping);
    assert_eq!(native.usage(), live);
    drop(closure);
    assert_eq!(native.usage(), baseline);
}
