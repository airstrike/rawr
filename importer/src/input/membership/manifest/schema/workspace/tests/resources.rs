use rawr_toml::document;

use super::super::super::super::{session, storage, Error, Stop};
use super::{parse, Workspace};
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
            Err("workspace refusal")
        } else {
            Ok(())
        }
    }
}

const FIXTURE: &str = "value={members=['a','b','c','d','e'],exclude=['tmp'],default-members=['a'],resolver='3',metadata={z=[1,-0.0],a={text='decoded text'}},package={version='1.2.3+build',unknown={x=[true]}},dependencies={z={path='../z',extra=[1,2]},a='1'},lints={rust={unused={level='warn',priority=-1,extra=[true]}}},outer=true}";

fn prior() -> Account {
    let mut account = Account::default();
    document::Admission::admit(&mut account, document::Request::Items(11)).unwrap();
    document::Admission::admit(&mut account, document::Request::String(7)).unwrap();
    account
}

#[test]
fn every_refusal_preserves_the_original_prefix_and_precedes_storage() {
    let document = parse(FIXTURE);
    let mut full = prior();
    let mut session = session::Session::new(&mut full);
    let result = Workspace::interpret(super::input(&document), &mut session);
    session.finish(result).unwrap();
    assert!(full.requests.len() > 60);
    for refusal in 2..full.requests.len() {
        let mut account = prior();
        account.refusal = Some(refusal);
        let mut session = session::Session::new(&mut account);
        storage::ALLOCATIONS.with(|count| count.set(0));
        assert!(matches!(
            Workspace::interpret(super::input(&document), &mut session),
            Err(Stop::Refused)
        ));
        let allocations = storage::ALLOCATIONS.with(|count| count.get());
        assert!(matches!(
            Workspace::interpret(super::input(&document), &mut session),
            Err(Stop::Refused)
        ));
        assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), allocations);
        assert!(matches!(
            session.finish(Ok(())),
            Err(Error::Admission("workspace refusal"))
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
    let result = Workspace::interpret(super::input(document), &mut session);
    let okay = session.finish(result).is_ok();
    assert!(allocation.check().is_ok(), "accounting targets never latch");
    (okay, allocation.usage, allocation.exceedances())
}

#[test]
fn cumulative_workspace_ownership_and_string_limits_are_exact_with_prior_usage() {
    let document = parse(FIXTURE);
    let mut full = Account::default();
    let mut session = session::Session::new(&mut full);
    let result = Workspace::interpret(super::input(&document), &mut session);
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
