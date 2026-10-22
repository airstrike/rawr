use rawr_toml::document;

use super::{exercise, Account};
use crate::input::membership::manifest::tests::admits_one_short;
use crate::input::membership::manifest::{session, storage, Error, Stop};
use crate::input::membership::Budget;
use crate::{input, node};

const SOURCE: &str = "profile.dev={unknown=1,build-override={debug='full',unknown=1},package={'foo@1.2'={opt-level=2},'*'={trim-paths=['object','macro']}},rustflags=['flag']}";

#[test]
fn every_recursive_trial_storage_and_callback_refusal_latches_original_first_cause() {
    let parsed = super::document(SOURCE);
    let prior = vec![document::Request::Items(13), document::Request::String(7)];
    let mut full = Account {
        requests: prior.clone(),
        refusal: None,
    };
    let mut session = session::Session::new(&mut full);
    let result = exercise(super::input(&parsed), &mut session);
    session.finish(result).unwrap();
    for refusal in prior.len()..full.requests.len() {
        let mut account = Account {
            requests: prior.clone(),
            refusal: Some(refusal),
        };
        let mut session = session::Session::new(&mut account);
        assert!(
            matches!(
                exercise(super::input(&parsed), &mut session),
                Err(Stop::Refused)
            ),
            "{refusal}"
        );
        let allocations = storage::ALLOCATIONS.with(|count| count.get());
        assert!(matches!(
            exercise(super::input(&parsed), &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(session.finish(Ok(())), Err(Error::Admission(index)) if index == refusal));
        assert_eq!(account.requests, full.requests[..=refusal], "{refusal}");
        assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), allocations);
    }
}

fn limited(
    items: usize,
    bytes: usize,
    string: usize,
) -> (bool, input::Usage, crate::exceedance::Record) {
    let parsed = super::document(SOURCE);
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::with_limits(items, bytes, string);
    allocation.items(13).unwrap();
    allocation.bytes(31).unwrap();
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut session = session::Session::new(&mut budget);
    let result = exercise(super::input(&parsed), &mut session);
    let success = session.finish(result).is_ok();
    assert!(allocation.check().is_ok(), "accounting targets never latch");
    (success, allocation.usage, allocation.exceedances())
}

#[test]
fn actual_flat_node_continuation_map_and_callback_layouts_have_exact_original_floors() {
    let (success, usage, _) = limited(usize::MAX, usize::MAX, usize::MAX);
    assert!(success);
    let parsed = super::document(SOURCE);
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let result = exercise(super::input(&parsed), &mut session);
    session.finish(result).unwrap();
    let string = account
        .requests
        .iter()
        .filter_map(|request| {
            if let document::Request::String(length) = request {
                Some(*length)
            } else {
                None
            }
        })
        .max()
        .unwrap();
    assert_eq!(
        limited(usage.items, usage.bytes, string),
        (true, usage, crate::exceedance::Record::new())
    );
    // One short of an aggregate target admits and records that axis.
    admits_one_short(
        limited(usage.items - 1, usage.bytes, string),
        usage,
        input::INPUT_ITEMS_RESOURCE,
        usage.items,
    );
    admits_one_short(
        limited(usage.items, usage.bytes - 1, string),
        usage,
        input::INPUT_BYTES_RESOURCE,
        usage.bytes,
    );
    // One short of the longest decoded string admits the same work and
    // records only the string axis; the account never latches.
    let (okay, spent, exceeded) = limited(usage.items, usage.bytes, string - 1);
    assert!(okay, "a string target is not an admission gate");
    assert_eq!(spent, usage, "the exact charge is still spent");
    let mut axes = exceeded.axes();
    let (Some(axis), None) = (axes.next(), axes.next()) else {
        panic!("only the string axis is recorded: {exceeded:?}");
    };
    assert_eq!(
        (
            axis.name,
            axis.target,
            axis.first_requested,
            axis.high_water
        ),
        (
            "membership decoded TOML string bytes",
            (string - 1) as u64,
            string as u64,
            string as u64
        )
    );
}
