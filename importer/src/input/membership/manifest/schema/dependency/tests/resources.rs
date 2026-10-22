use rawr_toml::document;

use super::super::super::super::{session, storage, value, walk, Error, Stop};
use super::super::{inheritable, Inheritable};
use super::parse;
use crate::input::membership::manifest::tests::admits_one_short;
use crate::input::membership::Budget;
use crate::{input, node};

#[derive(Default)]
struct Account {
    requests: Vec<document::Request>,
    refusal: Option<usize>,
}
impl document::Admission for Account {
    type Error = &'static str;
    fn admit(&mut self, request: document::Request) -> Result<(), Self::Error> {
        let index = self.requests.len();
        self.requests.push(request);
        if self.refusal == Some(index) {
            Err("dependency refusal")
        } else {
            Ok(())
        }
    }
}

const FIXTURE:&str="value={workspace='fallback',version='1',features=['a','b','c','d','e'],artifact=['bin','staticlib'],unused={z=[1,2,3,4,5],a='retained'}}";

#[test]
fn every_capture_trial_and_fallback_refusal_is_terminal_before_further_allocation() {
    let document = parse(FIXTURE);
    let prior = vec![document::Request::Items(7), document::Request::String(9)];
    let mut full = Account {
        requests: prior.clone(),
        refusal: None,
    };
    let mut session = session::Session::new(&mut full);
    let result = Inheritable::interpret(super::input(&document), &mut session);
    let output = session.finish(result).unwrap();
    assert!(matches!(output.kind(), inheritable::Kind::Value(_)));
    for refusal in prior.len()..full.requests.len() {
        let mut account = Account {
            requests: prior.clone(),
            refusal: Some(refusal),
        };
        let mut session = session::Session::new(&mut account);
        storage::ALLOCATIONS.with(|count| count.set(0));
        let result = Inheritable::interpret(super::input(&document), &mut session);
        assert!(matches!(result, Err(Stop::Refused)), "request {refusal}");
        assert!(matches!(
            Inheritable::interpret(super::input(&document), &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(
            session.finish(result),
            Err(Error::Admission("dependency refusal"))
        ));
        assert_eq!(account.requests, full.requests[..=refusal]);
        let allocations = full.requests[prior.len()..refusal]
            .iter()
            .filter(|request| matches!(request, document::Request::Allocation { .. }))
            .count();
        assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), allocations);
    }
}

fn budgeted(
    document: &document::Document<'_>,
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
    let result = Inheritable::interpret(super::input(document), &mut session);
    let okay = session.finish(result).is_ok();
    assert!(allocation.check().is_ok(), "accounting targets never latch");
    (okay, allocation.usage, allocation.exceedances())
}

#[test]
fn failed_trial_charges_remain_in_exact_original_account_and_each_floor_is_exact() {
    let document = parse(FIXTURE);
    let (okay, exact, _) = budgeted(&document, usize::MAX, usize::MAX, usize::MAX);
    assert!(okay);
    assert_eq!(
        budgeted(&document, exact.items, exact.bytes, 9),
        (true, exact, crate::exceedance::Record::new())
    );
    // The failed trial charges stay spent; one short records the axis.
    admits_one_short(
        budgeted(&document, exact.items - 1, exact.bytes, 9),
        exact,
        input::INPUT_ITEMS_RESOURCE,
        exact.items,
    );
    admits_one_short(
        budgeted(&document, exact.items, exact.bytes - 1, 9),
        exact,
        input::INPUT_BYTES_RESOURCE,
        exact.bytes,
    );
    // One short of the longest decoded string (9 bytes) admits the same
    // work and records only the string axis; the account never latches.
    admits_string_one_short(budgeted(&document, exact.items, exact.bytes, 8), exact, 9);
}

/// One short of the longest decoded TOML string admits the whole run, spends
/// exactly `exact`, and records only the string axis, crossed first and
/// highest by that longest string.
fn admits_string_one_short(
    (okay, usage, exceeded): (bool, input::Usage, crate::exceedance::Record),
    exact: input::Usage,
    string: usize,
) {
    assert!(okay, "a string target is not an admission gate");
    assert_eq!(usage, exact, "the exact charge is still spent");
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

#[test]
fn deep_initial_and_flattened_capture_has_flat_success_and_failure_teardown() {
    for leaf in ["1", "340282366920938463463374607431768211456"] {
        let mut source = String::from("value={unused={");
        for _ in 0..78 {
            for _ in 0..78 {
                source.push_str("a.");
            }
            source.push_str("a={");
        }
        source.push_str("leaf=");
        source.push_str(leaf);
        for _ in 0..80 {
            source.push('}');
        }
        let document = parse(&source);
        let mut account = Account::default();
        let mut session = session::Session::new(&mut account);
        let result = Inheritable::interpret(super::input(&document), &mut session);
        let output = session.finish(result);
        assert_eq!(output.is_ok(), leaf == "1");
        drop(output);
        drop(document);
    }
}

#[test]
fn borrowed_trials_do_not_mint_a_fresh_graph_account() {
    let document = parse(FIXTURE);
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
    let captured =
        walk::project(super::input(&document), value::Mode::Direct, &mut budget).unwrap();
    let mut session = session::Session::new(&mut budget);
    let result = Inheritable::interpret(value::Input::replay(&captured, 0), &mut session);
    session.finish(result).unwrap();
}
