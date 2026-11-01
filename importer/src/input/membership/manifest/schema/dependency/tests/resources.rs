use rawr_toml::document;

use super::super::super::super::{session, storage, value, walk, Error, Stop};
use super::super::{inheritable, Inheritable};
use super::parse;
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
