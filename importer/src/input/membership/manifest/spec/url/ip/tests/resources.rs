use rawr_toml::document;

use super::super::{text, v4, v6};
use super::{exercise, Account};
use crate::input::membership::manifest::tests::admits_one_short;
use crate::input::membership::manifest::{session, storage, Error, Stop};
use crate::input::membership::Budget;
use crate::{input, node};

#[test]
fn every_request_refusal_preserves_prior_usage_and_stops_subsequent_operations() {
    for (source, version6) in [
        ("127.1", false),
        ("09", false),
        ("::ffff:192.168.0.1", true),
        ("1::2::3", true),
    ] {
        let prior = vec![document::Request::Items(7), document::Request::String(19)];
        let mut full = Account {
            requests: prior.clone(),
            refusal: None,
        };
        let mut session = session::Session::new(&mut full);
        let result = exercise(source, version6, &mut session);
        session.finish(result).unwrap();
        assert!(!full
            .requests
            .iter()
            .any(|request| matches!(request, document::Request::Allocation { .. })));
        for refusal in prior.len()..full.requests.len() {
            let mut account = Account {
                requests: prior.clone(),
                refusal: Some(refusal),
            };
            let mut session = session::Session::new(&mut account);
            storage::ALLOCATIONS.with(|count| count.set(0));
            assert!(matches!(
                exercise(source, version6, &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(
                v4::candidate("0", &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(
                v4::Address::parse("0", &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(
                v6::Address::parse("::", &mut session),
                Err(Stop::Refused)
            ));
            assert!(
                matches!(session.finish(Ok(())), Err(Error::Admission(index)) if index == refusal)
            );
            assert_eq!(
                account.requests,
                full.requests[..=refusal],
                "{source}, {refusal}"
            );
            assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), 0);
        }
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

fn budgeted(
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
    let result = (|| {
        let parsed = v6::Address::parse("ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff", &mut session)?;
        let Ok(value) = parsed else {
            panic!("valid address")
        };
        let mut retained = storage::Buffer::new();
        for _ in 0..5 {
            let text = value.serialize(&mut session)?;
            retained.push(text, &mut session)?;
        }
        assert_eq!(retained.len(), 5);
        Ok(())
    })();
    let okay = session.finish(result).is_ok();
    assert!(allocation.check().is_ok(), "accounting targets never latch");
    (okay, allocation.usage, allocation.exceedances())
}

#[test]
fn original_account_exact_item_string_and_caller_replacement_layout_floors() {
    let (okay, exact, _) = budgeted(usize::MAX, usize::MAX, usize::MAX);
    assert!(okay);
    assert_eq!(
        budgeted(exact.items, exact.bytes, 39),
        (true, exact, crate::exceedance::Record::new())
    );
    // One short of an aggregate target admits and records that axis.
    admits_one_short(
        budgeted(exact.items - 1, exact.bytes, 39),
        exact,
        input::INPUT_ITEMS_RESOURCE,
        exact.items,
    );
    admits_one_short(
        budgeted(exact.items, exact.bytes - 1, 39),
        exact,
        input::INPUT_BYTES_RESOURCE,
        exact.bytes,
    );
    // One short of the 39-byte serialized text admits the same work and
    // records only the string axis with 39 as its high water.
    admits_one_short_string(budgeted(exact.items, exact.bytes, 38), exact, 38, 39);
    assert_eq!(exact.items, 24); // prior 13, one address, five texts, five slots
    let layout = std::alloc::Layout::array::<text::Text>(4).unwrap().size()
        + std::alloc::Layout::array::<text::Text>(8).unwrap().size();
    assert_eq!(exact.bytes, 31 + layout);
}

#[test]
fn fixed_text_checks_before_output_and_preserves_terminal_refusal() {
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let value = v6::Address::parse("::1", &mut session).unwrap().unwrap();
    session.finish(Ok(())).unwrap();
    for refusal in 0..2 {
        let mut account = Account {
            requests: Vec::new(),
            refusal: Some(refusal),
        };
        let mut session = session::Session::new(&mut account);
        assert!(matches!(value.serialize(&mut session), Err(Stop::Refused)));
        assert!(matches!(
            text::Text::render(|_| panic!("latched refusal must not emit"), &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(session.finish(Ok(())), Err(Error::Admission(index)) if index == refusal));
        assert_eq!(account.requests.len(), refusal + 1);
    }
}

#[test]
fn ip_conversion_does_not_spend_unrelated_graph_accounts() {
    let graph = node::GraphBudget::new();
    graph.paths(node::MAX_PATH_CHECKS_PER_GRAPH).unwrap();
    graph
        .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
        .unwrap();
    graph
        .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
        .unwrap();
    for _ in 0..64 {
        graph.comparison().unwrap();
    }
    let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, usize::MAX);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut session = session::Session::new(&mut budget);
    exercise("0x7f.1", false, &mut session).unwrap();
    exercise("::ffff:192.168.0.1", true, &mut session).unwrap();
    session.finish(Ok(())).unwrap();
}
