use rawr_toml::document;

use super::super::super::super::{session, storage, Error, Stop};
use super::{parse, Platform};
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
            Err("platform refusal")
        } else {
            Ok(())
        }
    }
}

const FIXTURE: &str = "value={dependencies={z={workspace=true,features=['a','b','c','d','e'],extra={x=[1]}},a={version='1',extra=[true]}},build-dependencies={a='2'},build_dependencies={a={path='../a'}},dev-dependencies={a='3'},dev_dependencies={a={workspace=true}},unknown={nested=[1,2]}}";

fn prior() -> Account {
    let mut account = Account::default();
    document::Admission::admit(&mut account, document::Request::Items(11)).unwrap();
    document::Admission::admit(&mut account, document::Request::String(7)).unwrap();
    account
}

#[test]
fn every_refusal_stops_all_alias_visitors_before_further_admission_or_allocation() {
    let document = parse(FIXTURE);
    let mut full = prior();
    let mut session = session::Session::new(&mut full);
    let result = Platform::interpret(super::input(&document), &mut session);
    session.finish(result).unwrap();
    assert!(full.requests.len() > 60);
    for refusal in 2..full.requests.len() {
        let mut account = prior();
        account.refusal = Some(refusal);
        let mut session = session::Session::new(&mut account);
        storage::ALLOCATIONS.with(|count| count.set(0));
        assert!(matches!(
            Platform::interpret(super::input(&document), &mut session),
            Err(Stop::Refused)
        ));
        let allocations = storage::ALLOCATIONS.with(|count| count.get());
        assert!(matches!(
            Platform::interpret(super::input(&document), &mut session),
            Err(Stop::Refused)
        ));
        assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), allocations);
        assert!(matches!(
            session.finish(Ok(())),
            Err(Error::Admission("platform refusal"))
        ));
        assert_eq!(
            account.requests,
            full.requests[..=refusal],
            "request {refusal}"
        );
        let admitted = full.requests[2..refusal]
            .iter()
            .filter(|request| matches!(request, document::Request::Allocation { .. }))
            .count();
        assert_eq!(allocations, admitted, "request {refusal}");
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
    let result = Platform::interpret(super::input(document), &mut session);
    let okay = session.finish(result).is_ok();
    assert!(allocation.check().is_ok(), "accounting targets never latch");
    (okay, allocation.usage, allocation.exceedances())
}

#[test]
fn all_five_fields_share_the_original_exact_aggregate_and_string_accounts() {
    let document = parse(FIXTURE);
    let mut full = Account::default();
    let mut session = session::Session::new(&mut full);
    let result = Platform::interpret(super::input(&document), &mut session);
    session.finish(result).unwrap();
    let string = full
        .requests
        .iter()
        .filter_map(|request| match request {
            document::Request::String(length) => Some(*length),
            _ => None,
        })
        .max()
        .unwrap();
    let (okay, exact, _) = budgeted(&document, usize::MAX, usize::MAX, usize::MAX);
    assert!(okay);
    assert_eq!(
        budgeted(&document, exact.items, exact.bytes, string),
        (true, exact, crate::exceedance::Record::new())
    );
    // One short of an aggregate target admits and records that axis.
    admits_one_short(
        budgeted(&document, exact.items - 1, exact.bytes, string),
        exact,
        input::INPUT_ITEMS_RESOURCE,
        exact.items,
    );
    admits_one_short(
        budgeted(&document, exact.items, exact.bytes - 1, string),
        exact,
        input::INPUT_BYTES_RESOURCE,
        exact.bytes,
    );
    // One short of the longest decoded string admits the same work and
    // records only the string axis; the account never latches.
    let (okay, usage, exceeded) = budgeted(&document, exact.items, exact.bytes, string - 1);
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
