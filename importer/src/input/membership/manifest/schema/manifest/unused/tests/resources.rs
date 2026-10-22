use rawr_toml::document;

use crate::input::membership::manifest::tests::admits_one_short;
use crate::input::membership::manifest::{session, storage, Error, Stop};
use crate::input::membership::Budget;
use crate::{input, node};

use super::{parse, source, Original, SOURCE};

#[derive(Default)]
struct Account {
    requests: Vec<document::Request>,
    refusal: Option<usize>,
}

impl document::Admission for Account {
    type Error = usize;

    fn admit(&mut self, request: document::Request) -> Result<(), usize> {
        let index = self.requests.len();
        self.requests.push(request);
        if self.refusal == Some(index) {
            Err(index)
        } else {
            Ok(())
        }
    }
}

fn prior() -> Account {
    Account {
        requests: vec![document::Request::Items(7), document::Request::String(3)],
        refusal: None,
    }
}

#[test]
fn every_original_admission_refusal_stops_before_further_storage_or_callback_work() {
    let document = parse(SOURCE);
    let mut full = prior();
    let mut session = session::Session::new(&mut full);
    let result = Original::interpret(source(&document), &mut session);
    session.finish(result).unwrap();
    for refusal in 2..full.requests.len() {
        let mut account = prior();
        account.refusal = Some(refusal);
        let mut session = session::Session::new(&mut account);
        storage::ALLOCATIONS.with(|count| count.set(0));
        super::super::ALLOCATIONS.with(|count| count.set(0));
        assert!(
            matches!(
                Original::interpret(source(&document), &mut session),
                Err(Stop::Refused)
            ),
            "request {refusal}"
        );
        let containers = storage::ALLOCATIONS.with(|count| count.get());
        let strings = super::super::ALLOCATIONS.with(|count| count.get());
        assert!(matches!(
            Original::interpret(source(&document), &mut session),
            Err(Stop::Refused)
        ));
        assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), containers);
        assert_eq!(super::super::ALLOCATIONS.with(|count| count.get()), strings);
        assert!(matches!(session.finish(Ok(())), Err(Error::Admission(index)) if index == refusal));
        assert_eq!(
            account.requests,
            full.requests[..=refusal],
            "request {refusal}"
        );
    }
}

fn limited<'d>(
    document: &'d document::Document<'d>,
    items: usize,
    bytes: usize,
    string: usize,
) -> (bool, input::Usage, crate::exceedance::Record) {
    let graph = node::GraphBudget::new();
    // Pure typed bookkeeping must not spend filesystem/hash/candidate work.
    graph.paths(node::MAX_PATH_CHECKS_PER_GRAPH).unwrap();
    graph
        .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
        .unwrap();
    let mut allocation = input::Allocation::with_limits(items, bytes, string);
    allocation.items(7).unwrap();
    allocation.bytes(17).unwrap();
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut session = session::Session::new(&mut budget);
    let result = Original::interpret(source(document), &mut session);
    let success = session.finish(result).is_ok();
    assert!(allocation.check().is_ok(), "accounting targets never latch");
    (success, allocation.usage, allocation.exceedances())
}

#[test]
fn complete_original_and_bookkeeping_share_exact_prior_item_byte_and_string_floors() {
    let document = parse(SOURCE);
    let (success, usage, _) = limited(&document, usize::MAX, usize::MAX, usize::MAX);
    assert!(success);
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let result = Original::interpret(source(&document), &mut session);
    session.finish(result).unwrap();
    let string = account
        .requests
        .iter()
        .filter_map(|request| match request {
            document::Request::String(length) => Some(*length),
            _ => None,
        })
        .max()
        .unwrap();
    assert_eq!(
        limited(&document, usage.items, usage.bytes, string),
        (true, usage, crate::exceedance::Record::new())
    );
    // One short of an aggregate target admits and records that axis.
    admits_one_short(
        limited(&document, usage.items - 1, usage.bytes, string),
        usage,
        input::INPUT_ITEMS_RESOURCE,
        usage.items,
    );
    admits_one_short(
        limited(&document, usage.items, usage.bytes - 1, string),
        usage,
        input::INPUT_BYTES_RESOURCE,
        usage.bytes,
    );
    // One short of the longest decoded string admits the same work and
    // records only the string axis; the account never latches.
    let (okay, spent, exceeded) = limited(&document, usage.items, usage.bytes, string - 1);
    assert!(okay, "a string target is not an admission gate");
    assert_eq!(spent, usage, "the exact charge is still spent");
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
fn rendering_refusal_precedes_string_allocation_and_empty_path_owns_no_heap_buffer() {
    let key = crate::input::membership::manifest::value::Key {
        text: crate::input::membership::manifest::value::Text::Borrowed("expanded.path"),
        span: None,
    };
    let path = [super::package::Position::Map(key)];
    for refusal in [0, 1] {
        let mut account = Account {
            requests: Vec::new(),
            refusal: Some(refusal),
        };
        let mut session = session::Session::new(&mut account);
        super::super::ALLOCATIONS.with(|count| count.set(0));
        assert!(matches!(
            super::render(&path, &mut session),
            Err(Stop::Refused)
        ));
        assert_eq!(super::super::ALLOCATIONS.with(|count| count.get()), 0);
        assert!(session.finish(Ok(())).is_err());
    }
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    assert_eq!(super::render(&[], &mut session).unwrap(), "");
    session.finish(Ok(())).unwrap();
    assert_eq!(account.requests, [document::Request::String(0)]);
}
