use super::{retained_bytes, Pending};
use crate::host::ImportAllocation as _;
use crate::package::import::runner::account::{self, Attempt};

fn native() -> account::Native {
    account::Native::transform(account::tests::carrier()).unwrap()
}

#[test]
fn holder_is_precharged_and_unfinished_construction_releases_its_attempt() {
    let bytes = std::mem::size_of::<Attempt>() + retained_bytes();
    let native = native();
    let baseline = native.usage();
    let pending = Pending::transform(native.attempt().unwrap()).unwrap();
    let owner = pending.support_owner().unwrap();
    assert_eq!(native.usage(), (baseline.0 + 2, baseline.1 + bytes));
    assert!(owner.0.get().is_none());
    drop(owner);
    drop(pending);
    assert_eq!(native.usage(), baseline);
}

#[test]
fn completed_clones_share_one_contribution_and_overlap_in_the_same_account() {
    let native = native();
    let baseline = native.usage();
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

    // An overlapping holder charges the same account and releases on drop.
    let mut overlapping = Pending::transform(native.attempt().unwrap()).unwrap();
    let full = native.usage();
    overlapping.retain_checked(Some(1), Some(0)).unwrap();
    assert_eq!(native.usage(), (full.0 + 1, full.1));
    assert_eq!(native.snapshot().first_refusal, None);
    drop(overlapping);
    assert_eq!(native.usage(), live);
    drop(closure);
    assert_eq!(native.usage(), baseline);
}
