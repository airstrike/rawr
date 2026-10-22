use rawr_toml::document;

use super::super::Host;
use super::{parse, Account};
use crate::input::membership::manifest::tests::admits_one_short;
use crate::input::membership::manifest::{session, storage, Error, Stop};
use crate::input::membership::Budget;
use crate::{input, node};

#[test]
fn every_admission_refusal_stops_host_trials_before_additional_storage() {
    for (source, opaque) in [
        ("abc", false),
        ("%61bc", false),
        ("%41bc", false),
        ("b%C3%BCcher.example", false),
        ("abc%FF", false),
        ("%C2%AD", false),
        ("127.1", false),
        ("[::1]", true),
        ("[", false),
        ("é\u{1}", true),
        ("é/", true),
    ] {
        let prior = vec![document::Request::Items(11), document::Request::String(7)];
        let mut full = Account {
            requests: prior.clone(),
            refusal: None,
        };
        let mut session = session::Session::new(&mut full);
        let result = parse(source, opaque, &mut session);
        let _ = session.finish(result).unwrap();
        for refusal in prior.len()..full.requests.len() {
            let mut account = Account {
                requests: prior.clone(),
                refusal: Some(refusal),
            };
            let mut session = session::Session::new(&mut account);
            storage::ALLOCATIONS.with(|count| count.set(0));
            assert!(
                matches!(parse(source, opaque, &mut session), Err(Stop::Refused)),
                "{source}, {refusal}"
            );
            assert!(matches!(
                Host::special("next", &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(
                Host::opaque("next", &mut session),
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
            let allocations = full.requests[prior.len()..refusal]
                .iter()
                .filter(|request| matches!(request, document::Request::Allocation { .. }))
                .count();
            assert_eq!(
                storage::ALLOCATIONS.with(|count| count.get()),
                allocations,
                "{source}, {refusal}"
            );
        }
    }
}

const SPECIAL: &str = "%42%C3%9Ccher.example";
const OPAQUE: &str = "ééééééééé";

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
        assert!(Host::special(SPECIAL, &mut session)?.is_ok());
        assert!(Host::opaque(OPAQUE, &mut session)?.is_ok());
        Ok(())
    })();
    let okay = session.finish(result).is_ok();
    assert!(allocation.check().is_ok(), "accounting targets never latch");
    (okay, allocation.usage, allocation.exceedances())
}

#[test]
fn decoded_idna_and_opaque_storage_share_one_exact_original_account() {
    let (okay, exact, _) = budgeted(usize::MAX, usize::MAX, usize::MAX);
    assert!(okay);
    let string = OPAQUE.len() * 3;
    assert_eq!(
        budgeted(exact.items, exact.bytes, string),
        (true, exact, crate::exceedance::Record::new())
    );
    // One short of an aggregate target admits and records that axis.
    admits_one_short(
        budgeted(exact.items - 1, exact.bytes, string),
        exact,
        input::INPUT_ITEMS_RESOURCE,
        exact.items,
    );
    admits_one_short(
        budgeted(exact.items, exact.bytes - 1, string),
        exact,
        input::INPUT_BYTES_RESOURCE,
        exact.bytes,
    );
    // One short of the longest decoded string admits the same work and
    // records only the string axis, whose high water is that exact length.
    admits_one_short_string(
        budgeted(exact.items, exact.bytes, string - 1),
        exact,
        string - 1,
        string,
    );
}

#[test]
fn host_scalar_work_does_not_reset_or_spend_unrelated_graph_accounts() {
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
    assert!(Host::special(SPECIAL, &mut session).unwrap().is_ok());
    assert!(Host::opaque(OPAQUE, &mut session).unwrap().is_ok());
    session.finish(Ok(())).unwrap();
}

fn opaque(source: &str, string: usize) -> (bool, input::Usage, crate::exceedance::Record) {
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, string);
    allocation.items(3).unwrap();
    allocation.bytes(7).unwrap();
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut session = session::Session::new(&mut budget);
    let result = Host::opaque(source, &mut session);
    let okay = matches!(session.finish(result), Ok(Ok(_)));
    assert!(allocation.check().is_ok(), "accounting targets never latch");
    (okay, allocation.usage, allocation.exceedances())
}

#[test]
fn raw_and_expanded_string_targets_record_and_the_first_owned_layout_completes() {
    for (source, limit) in [("é", 1), ("é", 5)] {
        // The string requests this opaque host makes, in order: the raw
        // spelling first, then the percent-expanded output.
        let mut recorder = Account {
            requests: Vec::new(),
            refusal: None,
        };
        let mut session = session::Session::new(&mut recorder);
        let result = Host::opaque(source, &mut session);
        assert!(session.finish(result).unwrap().is_ok());
        let strings = recorder
            .requests
            .iter()
            .filter_map(|request| match request {
                document::Request::String(length) => Some(*length),
                _ => None,
            })
            .collect::<Vec<_>>();
        let first = *strings.iter().find(|length| **length > limit).unwrap();
        let total = *strings.iter().max().unwrap();
        assert_eq!(strings, [2, 6], "{source}, {limit}");

        // A raw or expanded string target below the request records the
        // crossing on the original account; the owned layout still completes
        // with the same charge as the unbounded run.
        let (okay, exact, exceeded) = opaque(source, usize::MAX);
        assert!(okay && !exceeded.is_exceeded(), "{source}, {limit}");
        assert_eq!(
            exact,
            input::Usage {
                items: 10,
                bytes: 19
            },
            "{source}, {limit}"
        );
        let axis = admits_one_short_string(opaque(source, limit), exact, limit, total);
        assert_eq!(axis.first_requested, first as u64, "{source}, {limit}");
    }
}
