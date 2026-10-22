use rawr_toml::document;

use super::{exercise, Account, Url};
use crate::input::membership::manifest::tests::admits_one_short;
use crate::input::membership::manifest::{session, storage, Error, Stop};
use crate::input::membership::Budget;
use crate::{input, node};

#[test]
fn every_admission_refusal_preserves_prior_requests_and_stops_new_trials() {
    let prior = vec![document::Request::Items(13), document::Request::String(7)];
    let mut full = Account {
        requests: prior.clone(),
        refusal: None,
    };
    let mut session = session::Session::new(&mut full);
    let result = exercise(&mut session);
    session.finish(result).unwrap();
    for refusal in prior.len()..full.requests.len() {
        let mut account = Account {
            requests: prior.clone(),
            refusal: Some(refusal),
        };
        let mut session = session::Session::new(&mut account);
        storage::ALLOCATIONS.with(|count| count.set(0));
        assert!(
            matches!(exercise(&mut session), Err(Stop::Refused)),
            "{refusal}"
        );
        let allocations = storage::ALLOCATIONS.with(|count| count.get());
        assert!(matches!(
            Url::parse("http://next/", &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(session.finish(Ok(())), Err(Error::Admission(index)) if index == refusal));
        assert_eq!(account.requests, full.requests[..=refusal], "{refusal}");
        assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), allocations);
    }
}

const STRING_RESOURCE: &str = "membership decoded TOML string bytes";

/// A string target one short of the longest decoded string admits the
/// same work, spends the same charge, and records only the string axis with
/// that exact length as its high water. Returns the axis for closer checks.
fn admits_one_short_string(
    (okay, usage, exceeded): (bool, input::Usage, crate::exceedance::Record),
    exact: input::Usage,
    target: usize,
    total: usize,
) -> crate::exceedance::Axis {
    assert!(okay, "a string target is not an admission gate");
    assert_eq!(usage, exact, "the exact charge is still spent");
    let mut axes = exceeded.axes();
    let (Some(axis), None) = (axes.next(), axes.next()) else {
        panic!("exactly one exceeded axis: {exceeded:?}")
    };
    assert_eq!(axis.name, STRING_RESOURCE);
    assert_eq!(axis.target, target as u64);
    assert_eq!(axis.high_water, total as u64);
    assert!(axis.first_requested > axis.target);
    assert!(axis.first_requested <= axis.high_water);
    *axis
}

fn limited(
    items: usize,
    bytes: usize,
    string: usize,
) -> (bool, input::Usage, crate::exceedance::Record) {
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::with_limits(items, bytes, string);
    allocation.items(13).unwrap();
    allocation.bytes(31).unwrap();
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut session = session::Session::new(&mut budget);
    let result = exercise(&mut session);
    let success = session.finish(result).is_ok();
    assert!(allocation.check().is_ok(), "accounting targets never latch");
    (success, allocation.usage, allocation.exceedances())
}

#[test]
fn original_cumulative_item_layout_and_string_floors_are_exact() {
    let (success, usage, _) = limited(usize::MAX, usize::MAX, usize::MAX);
    assert!(success);
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let result = exercise(&mut session);
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
    // records only the string axis, whose high water is that exact length.
    admits_one_short_string(
        limited(usage.items, usage.bytes, string - 1),
        usage,
        string - 1,
        string,
    );
}

#[test]
fn refused_session_blocks_every_borrow_and_removal_without_storage_or_mutation() {
    let source = "custom:opaque   ?query#fragment";
    let mut full = Account::default();
    let mut session = session::Session::new(&mut full);
    let value = Url::parse(source, &mut session).unwrap().unwrap();
    session.finish(Ok(())).unwrap();
    let before = value.output.as_bytes().to_vec();
    let mut account = Account {
        requests: Vec::new(),
        refusal: Some(full.requests.len()),
    };
    let mut session = session::Session::new(&mut account);
    let mut value = Url::parse(source, &mut session).unwrap().unwrap();
    assert!(matches!(session.items(1), Err(Stop::Refused)));
    storage::ALLOCATIONS.with(|count| count.set(0));
    assert!(matches!(value.as_str(&mut session), Err(Stop::Refused)));
    assert!(matches!(value.scheme(&mut session), Err(Stop::Refused)));
    assert!(matches!(value.query(&mut session), Err(Stop::Refused)));
    assert!(matches!(value.fragment(&mut session), Err(Stop::Refused)));
    assert!(matches!(
        value.last_path_segment(&mut session),
        Err(Stop::Refused)
    ));
    assert!(matches!(
        value.remove_query(&mut session),
        Err(Stop::Refused)
    ));
    assert!(matches!(
        value.remove_fragment(&mut session),
        Err(Stop::Refused)
    ));
    assert_eq!(value.output.as_bytes(), before);
    assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), 0);
    assert!(session.finish(Ok(())).is_err());
    assert_eq!(account.requests.len(), full.requests.len() + 1);
}

#[test]
fn absent_and_empty_tails_mutate_without_new_ownership_and_strip_only_opaque_spaces() {
    for source in [
        "x:opaque   ?#",
        "x:opaque   #",
        "x:opaque   ?",
        "x:/path%20%20?#",
        "x://host?#",
    ] {
        let mut account = Account::default();
        let mut session = session::Session::new(&mut account);
        let mut value = Url::parse(source, &mut session).unwrap().unwrap();
        let mut reference = ::url::Url::parse(source).unwrap();
        storage::ALLOCATIONS.with(|count| count.set(0));
        value.remove_query(&mut session).unwrap();
        reference.set_query(None);
        value.remove_fragment(&mut session).unwrap();
        reference.set_fragment(None);
        assert_eq!(
            value.as_str(&mut session).unwrap(),
            reference.as_str(),
            "{source}"
        );
        assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), 0);
        session.finish(Ok(())).unwrap();
    }
}
