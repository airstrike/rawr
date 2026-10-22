use rawr_toml::document;

use super::{interpret, Account, Ascii};
use crate::input::membership::manifest::tests::admits_one_short;
use crate::input::membership::manifest::{session, storage, Error, Stop};
use crate::input::membership::Budget;
use crate::{input, node};

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

fn prior() -> Account {
    let mut account = Account::default();
    document::Admission::admit(&mut account, document::Request::Items(11)).unwrap();
    document::Admission::admit(&mut account, document::Request::String(7)).unwrap();
    account
}

#[test]
fn every_full_entry_refusal_stops_both_processing_and_delayed_output() {
    let cases = [
        "prefix.BÜCHER.Example".to_owned(),
        "a.ｘｎ－－bcher-kva\u{3002}A".to_owned(),
        "prefix.xn--aaaaa!".to_owned(),
        format!("a{}", "\u{315}\u{323}\u{301}".repeat(20)),
        format!("{}a", "\u{301}".repeat(60)),
    ];
    for source in cases {
        let mut full = prior();
        let _ = interpret(source.as_bytes(), &mut full).unwrap();
        assert!(full.requests.len() > 5, "{source}");
        for refusal in 2..full.requests.len() {
            let mut account = prior();
            account.refusal = Some(refusal);
            let mut session = session::Session::new(&mut account);
            storage::ALLOCATIONS.with(|count| count.set(0));
            assert!(
                matches!(
                    Ascii::interpret(source.as_bytes(), &mut session),
                    Err(Stop::Refused)
                ),
                "{source}: request {refusal}"
            );
            let allocations = storage::ALLOCATIONS.with(|count| count.get());
            assert!(matches!(
                Ascii::interpret(b"", &mut session),
                Err(Stop::Refused)
            ));
            assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), allocations);
            assert!(
                matches!(session.finish(Ok(())), Err(Error::Admission(index)) if index == refusal)
            );
            assert_eq!(
                account.requests,
                full.requests[..=refusal],
                "{source}: request {refusal}"
            );
            let expected = full.requests[2..refusal]
                .iter()
                .filter(|request| matches!(request, document::Request::Allocation { .. }))
                .count();
            assert_eq!(allocations, expected, "{source}: request {refusal}");
        }
    }
}

fn budgeted(
    source: &[u8],
    items: usize,
    bytes: usize,
    string: usize,
) -> (bool, input::Usage, crate::exceedance::Record) {
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::with_limits(items, bytes, string);
    allocation.items(11).unwrap();
    allocation.bytes(29).unwrap();
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut session = session::Session::new(&mut budget);
    let result = Ascii::interpret(source, &mut session);
    let okay = matches!(session.finish(result), Ok(Ok(_)));
    assert!(allocation.check().is_ok(), "accounting targets never latch");
    (okay, allocation.usage, allocation.exceedances())
}

#[test]
fn transformed_domains_charge_original_cumulative_storage_and_string_extents() {
    for source in [
        "prefix.BÜCHER.Example",
        "a.ｘｎ－－bcher-kva\u{3002}A",
        "A.क्\u{200d}क",
    ] {
        let mut full = Account::default();
        assert!(
            interpret(source.as_bytes(), &mut full).unwrap().is_ok(),
            "{source}"
        );
        let string = full
            .requests
            .iter()
            .filter_map(|request| match request {
                document::Request::String(length) => Some(*length),
                _ => None,
            })
            .max()
            .unwrap();
        let (okay, exact, _) = budgeted(source.as_bytes(), usize::MAX, usize::MAX, usize::MAX);
        assert!(okay, "{source}");
        assert_eq!(
            budgeted(source.as_bytes(), exact.items, exact.bytes, string),
            (true, exact, crate::exceedance::Record::new()),
            "{source}"
        );
        // One short of an aggregate target admits and records that axis.
        admits_one_short(
            budgeted(source.as_bytes(), exact.items - 1, exact.bytes, string),
            exact,
            input::INPUT_ITEMS_RESOURCE,
            exact.items,
        );
        admits_one_short(
            budgeted(source.as_bytes(), exact.items, exact.bytes - 1, string),
            exact,
            input::INPUT_BYTES_RESOURCE,
            exact.bytes,
        );
        // One short of the longest decoded string admits the same work
        // and records only the string axis at that exact length.
        admits_one_short_string(
            budgeted(source.as_bytes(), exact.items, exact.bytes, string - 1),
            exact,
            string - 1,
            string,
        );
    }
}

#[test]
fn borrowed_ascii_and_empty_results_still_measure_against_the_input_string_target() {
    let empty = budgeted(b"", 11, 29, 0);
    assert!(empty.0);
    assert!(!empty.2.is_exceeded());
    for source in [
        "abc.example",
        "a123.example",
        "0.example",
        "_srv._tcp",
        "xn--bcher-kva.example",
    ] {
        let mut full = Account::default();
        let value = interpret(source.as_bytes(), &mut full).unwrap().unwrap();
        assert!(value.is_borrowed(), "{source}");
        assert_eq!(full.requests[0], document::Request::String(source.len()));
        let (okay, exact, exceeded) =
            budgeted(source.as_bytes(), usize::MAX, usize::MAX, source.len());
        assert!(okay && !exceeded.is_exceeded(), "{source}");
        // The input spelling is the first and longest string request, so
        // any target below it records that one axis with the spelling's length
        // as both first request and high water; the borrow still completes.
        for target in [source.len() - 1, 0] {
            let axis = admits_one_short_string(
                budgeted(source.as_bytes(), usize::MAX, usize::MAX, target),
                exact,
                target,
                source.len(),
            );
            assert_eq!(axis.first_requested, source.len() as u64, "{source}");
        }
    }
}

#[test]
fn semantic_invalidity_remains_distinct_from_terminal_admission_refusal() {
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    assert!(matches!(Ascii::interpret(b"a/b", &mut session), Ok(Err(_))));
    let output = Ascii::interpret(b"abc", &mut session).unwrap().unwrap();
    assert_eq!(output.as_bytes(), b"abc");
    session.finish(Ok(())).unwrap();

    let mut full = Account::default();
    assert!(interpret(b"a/b", &mut full).unwrap().is_err());
    let refusal = full.requests.len() - 1;
    assert_eq!(full.requests[refusal], document::Request::Items(1));
    let mut refused = Account {
        refusal: Some(refusal),
        ..Account::default()
    };
    assert!(
        matches!(interpret(b"a/b", &mut refused), Err(Error::Admission(index)) if index == refusal)
    );
}
