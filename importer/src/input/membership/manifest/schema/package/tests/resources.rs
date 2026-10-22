use rawr_toml::document;

use super::super::super::super::{session, storage, Error};
use super::{parse, Defaults, Package};
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
            Err("original refusal")
        } else {
            Ok(())
        }
    }
}

const FIXTURE: &str = "value={name='package',version=' 1.2.3-pre+build ',authors=['a','b','c','d','e'],build=['a.rs','b.rs'],metabuild='build',readme=false,publish=['registry'],edition={workspace=true,unknown={a=[1]}},other=[2],metadata={x=[1,true,'s']}}";

#[test]
fn each_refusal_stops_package_allocations_and_cannot_be_discarded() {
    let document = parse(FIXTURE);
    let mut full = Account::default();
    let mut session = session::Session::new(&mut full);
    let result = Package::interpret(super::input(&document), &mut session);
    session.finish(result).unwrap();
    assert!(full.requests.len() > 40);
    for refusal in 0..full.requests.len() {
        let mut account = Account {
            refusal: Some(refusal),
            ..Account::default()
        };
        let mut session = session::Session::new(&mut account);
        storage::ALLOCATIONS.with(|count| count.set(0));
        let result = Package::interpret(super::input(&document), &mut session);
        assert!(result.is_err());
        assert!(matches!(
            session.finish(Ok(())),
            Err(Error::Admission("original refusal"))
        ));
        assert_eq!(account.requests, full.requests[..=refusal]);
        let allocations = full.requests[..refusal]
            .iter()
            .filter(|request| matches!(request, document::Request::Allocation { .. }))
            .count();
        assert_eq!(
            storage::ALLOCATIONS.with(|count| count.get()),
            allocations,
            "request {refusal}"
        );
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
    allocation.items(11).unwrap();
    allocation.bytes(29).unwrap();
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut session = session::Session::new(&mut budget);
    let result = Package::interpret(super::input(document), &mut session);
    let okay = session.finish(result).is_ok();
    assert!(allocation.check().is_ok(), "accounting targets never latch");
    (okay, allocation.usage, allocation.exceedances())
}

#[test]
fn package_uses_exact_original_aggregate_and_string_floors_with_prior_usage() {
    let document = parse(FIXTURE);
    let (okay, exact, _) = budgeted(&document, usize::MAX, usize::MAX, usize::MAX);
    assert!(okay);
    assert_eq!(
        budgeted(&document, exact.items, exact.bytes, 17),
        (true, exact, crate::exceedance::Record::new())
    );
    // One short of an aggregate target admits and records that axis.
    admits_one_short(
        budgeted(&document, exact.items - 1, exact.bytes, 17),
        exact,
        input::INPUT_ITEMS_RESOURCE,
        exact.items,
    );
    admits_one_short(
        budgeted(&document, exact.items, exact.bytes - 1, 17),
        exact,
        input::INPUT_BYTES_RESOURCE,
        exact.bytes,
    );
    // One short of the longest decoded string (17 bytes) admits the same
    // work and records only the string axis; the account never latches.
    let (okay, usage, exceeded) = budgeted(&document, exact.items, exact.bytes, 16);
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
        ("membership decoded TOML string bytes", 16, 17, 17)
    );
}

#[test]
fn default_badge_growth_refusal_precedes_every_owned_layout() {
    let document = parse("value={version='1.2.3',badges={z={e='e',d='d',c='c',b='b',a='a'},y={s='y'},x={s='x'},w={s='w'},v={s='v'}},include=['a','b','c','d','e'],rust-version='1.96'}");
    let mut full = Account::default();
    let mut session = session::Session::new(&mut full);
    let result = Defaults::interpret(super::input(&document), &mut session);
    session.finish(result).unwrap();
    for refusal in 0..full.requests.len() {
        let mut account = Account {
            refusal: Some(refusal),
            ..Account::default()
        };
        let mut session = session::Session::new(&mut account);
        storage::ALLOCATIONS.with(|count| count.set(0));
        let result = Defaults::interpret(super::input(&document), &mut session);
        assert!(matches!(
            session.finish(result),
            Err(Error::Admission("original refusal"))
        ));
        assert_eq!(account.requests, full.requests[..=refusal]);
        let allocations = full.requests[..refusal]
            .iter()
            .filter(|request| matches!(request, document::Request::Allocation { .. }))
            .count();
        assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), allocations);
    }
}

#[test]
fn package_metadata_and_ignored_descendants_have_bounded_stack_teardown() {
    for field in ["metadata", "unknown"] {
        let mut source = format!("value={{{field}={{");
        for _ in 0..78 {
            for _ in 0..78 {
                source.push_str("a.");
            }
            source.push_str("a={");
        }
        source.push_str("leaf=1");
        for _ in 0..80 {
            source.push('}');
        }
        let document = parse(&source);
        let mut account = Account::default();
        let mut session = session::Session::new(&mut account);
        let result = Package::interpret(super::input(&document), &mut session);
        let package = session.finish(result).unwrap();
        if field == "metadata" {
            let Some(super::super::Value::Metadata(value)) =
                package.get(super::super::Field::Metadata)
            else {
                panic!("metadata")
            };
            assert!(value.nodes.len() > 6_000);
        } else {
            assert_eq!(package.ignored().len(), 1);
        }
        drop(package);
        drop(document);
    }
}
