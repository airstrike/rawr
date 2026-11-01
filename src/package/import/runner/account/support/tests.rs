use super::Pending;
use crate::host::ImportAllocation as _;
use crate::package::import::runner::account::{self, Axis, Refusal};

fn native() -> account::Native {
    account::Native::transform(account::tests::carrier())
}

#[test]
fn unfinished_construction_releases_its_attempt() {
    let native = native();
    let baseline = native.owners();
    let pending = Pending::transform(native.attempt().unwrap());
    let owner = pending.support_owner().unwrap();
    assert_eq!(native.owners(), baseline + 1);
    assert!(owner.0.get().is_none());
    drop(owner);
    drop(pending);
    assert_eq!(native.owners(), baseline);
}

#[test]
fn completed_clones_share_one_receipt_until_the_last_retires() {
    let native = native();
    let baseline = native.owners();
    let mut pending = Pending::transform(native.attempt().unwrap());
    pending.retain_checked(Some(1), Some(41)).unwrap();
    let executable = pending.support_owner().unwrap();
    let closure = executable.clone();
    pending.finish().require(&executable);
    assert!(executable.0.get().is_some());
    assert_eq!(native.owners(), baseline + 1);
    drop(executable);
    assert_eq!(native.owners(), baseline + 1);

    // An overlapping holder shares the latch: its refusal latches the account.
    let mut overlapping = Pending::transform(native.attempt().unwrap());
    assert_eq!(native.owners(), baseline + 2);
    assert!(overlapping.retain_checked(Some(1), None).is_err());
    assert_eq!(native.first_refusal(), Some(Refusal { axis: Axis::Bytes }));
    drop(overlapping);
    assert_eq!(native.owners(), baseline + 1);
    drop(closure);
    assert_eq!(native.owners(), baseline);
}
