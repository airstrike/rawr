use rawr_toml::document;

use super::{exercise, Account, Output, Set};
use crate::input::membership::manifest::tests::admits_one_short;
use crate::input::membership::manifest::{session, storage, Error, Stop};
use crate::input::membership::Budget;
use crate::{input, node};

#[test]
fn every_refusal_preserves_prior_usage_and_blocks_all_later_mutation() {
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
        assert!(matches!(exercise(&mut session), Err(Stop::Refused)));
        let allocations = storage::ALLOCATIONS.with(|count| count.get());
        let mut output = Output::new();
        assert!(matches!(
            output.append("x", &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(output.push('é', &mut session), Err(Stop::Refused)));
        assert!(matches!(
            output.encode("", Set::Path, &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(
            output.insert(0, "", &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(
            output.remove(0..0, &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(
            output.truncate(0, &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(output.clear(&mut session), Err(Stop::Refused)));
        assert!(matches!(output.pop(&mut session), Err(Stop::Refused)));
        assert!(matches!(output.offset(&mut session), Err(Stop::Refused)));
        assert!(matches!(output.as_str(&mut session), Err(Stop::Refused)));
        assert!(matches!(session.finish(Ok(())), Err(Error::Admission(index)) if index == refusal));
        assert_eq!(account.requests, full.requests[..=refusal], "{refusal}");
        assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), allocations);
        assert_eq!(
            allocations,
            full.requests[prior.len()..refusal]
                .iter()
                .filter(|request| matches!(request, document::Request::Allocation { .. }))
                .count()
        );
    }
}

#[test]
fn refused_multibyte_append_and_insert_roll_back_only_content() {
    for insert in [false, true] {
        let mut full = Account::default();
        let mut session = session::Session::new(&mut full);
        let mut output = Output::new();
        output.append("abc", &mut session).unwrap();
        session.finish(Ok(())).unwrap();
        let prior = full.requests.len();
        // String succeeds, then every byte/item and growth refusal is exercised.
        for refusal in prior + 1..prior + 6 {
            let mut account = Account {
                requests: Vec::new(),
                refusal: Some(refusal),
            };
            let mut session = session::Session::new(&mut account);
            let mut output = Output::new();
            output.append("abc", &mut session).unwrap();
            let result = if insert {
                output.insert(1, "💩", &mut session)
            } else {
                output.append("💩", &mut session)
            };
            assert!(matches!(result, Err(Stop::Refused)));
            assert_eq!(output.as_bytes(), b"abc", "{insert}, {refusal}");
            assert!(std::str::from_utf8(output.as_bytes()).is_ok());
            assert!(session.finish(Ok(())).is_err());
            assert_eq!(account.requests.len(), refusal + 1);
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
    let result = exercise(&mut session);
    let okay = session.finish(result).is_ok();
    assert!(allocation.check().is_ok(), "accounting targets never latch");
    (okay, allocation.usage, allocation.exceedances())
}

#[test]
fn cumulative_item_string_and_full_geometric_layout_floors_are_exact() {
    let (okay, exact, _) = budgeted(usize::MAX, usize::MAX, usize::MAX);
    assert!(okay);
    assert_eq!(exact.items, 13 + 12 + 28 + 5 + 3 + 1);
    assert_eq!(exact.bytes, 31 + 4 + 8 + 16 + 32 + 64);
    assert_eq!(
        budgeted(exact.items, exact.bytes, 45),
        (true, exact, crate::exceedance::Record::new())
    );
    // One short of an aggregate target admits and records that axis.
    admits_one_short(
        budgeted(exact.items - 1, exact.bytes, 45),
        exact,
        input::INPUT_ITEMS_RESOURCE,
        exact.items,
    );
    admits_one_short(
        budgeted(exact.items, exact.bytes - 1, 45),
        exact,
        input::INPUT_BYTES_RESOURCE,
        exact.bytes,
    );
    // One short of the 45-byte longest output admits the same work and
    // records only the string axis with 45 as its high water.
    admits_one_short_string(budgeted(exact.items, exact.bytes, 44), exact, 44, 45);
}

#[test]
fn string_target_crossing_records_while_copy_and_layout_complete_with_existing_content() {
    // The cumulative output length is measured against the string
    // target before the copy. Crossing it records the axis on the original
    // account; the copy and any growth layout still complete and are charged
    // (the raw copy fits the existing 4-byte layout; the expansion grows to 8).
    for (text, encoded, limit, expected, allocations, items, bytes) in [
        ("é", false, 2, "xé", 0, 16, 35),
        ("é", true, 6, "x%C3%A9", 1, 20, 43),
    ] {
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, limit);
        allocation.items(13).unwrap();
        allocation.bytes(31).unwrap();
        let mut budget = Budget {
            graph: &graph,
            allocation: &mut allocation,
        };
        let mut session = session::Session::new(&mut budget);
        let mut output = Output::new();
        output.append("x", &mut session).unwrap();
        storage::ALLOCATIONS.with(|count| count.set(0));
        let result = if encoded {
            output.encode(text, Set::Path, &mut session)
        } else {
            output.append(text, &mut session)
        };
        assert!(result.is_ok(), "{encoded}");
        assert_eq!(output.as_bytes(), expected.as_bytes(), "{encoded}");
        assert_eq!(
            storage::ALLOCATIONS.with(|count| count.get()),
            allocations,
            "{encoded}"
        );
        session.finish(result).unwrap();
        assert!(allocation.check().is_ok(), "accounting targets never latch");
        assert_eq!(
            (allocation.usage.items, allocation.usage.bytes),
            (items, bytes),
            "{encoded}"
        );
        let exceeded = allocation.exceedances();
        let mut axes = exceeded.axes();
        let (Some(axis), None) = (axes.next(), axes.next()) else {
            panic!("exactly one exceeded axis: {exceeded:?}")
        };
        assert_eq!(axis.name, STRING_RESOURCE, "{encoded}");
        assert_eq!(axis.target, limit as u64, "{encoded}");
        assert_eq!(axis.first_requested, expected.len() as u64, "{encoded}");
        assert_eq!(axis.high_water, expected.len() as u64, "{encoded}");
    }
}

#[test]
fn truncation_and_clear_reuse_only_previously_admitted_capacity() {
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let mut output = Output::new();
    output.append("12345", &mut session).unwrap();
    storage::ALLOCATIONS.with(|count| count.set(0));
    output.truncate(1, &mut session).unwrap();
    output.append("2345678", &mut session).unwrap();
    output.clear(&mut session).unwrap();
    output.append("12345678", &mut session).unwrap();
    assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), 0);
    output.push('9', &mut session).unwrap();
    assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), 1);
    session.finish(Ok(())).unwrap();
    assert_eq!(
        account
            .requests
            .iter()
            .filter_map(|request| {
                if let document::Request::Allocation { bytes, .. } = request {
                    Some(*bytes)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>(),
        [4, 8, 16]
    );
}

#[test]
fn output_work_neither_spends_nor_resets_graph_accounts() {
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
    let result = exercise(&mut session);
    session.finish(result).unwrap();
}
