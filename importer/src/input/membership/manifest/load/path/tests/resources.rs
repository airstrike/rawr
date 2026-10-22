use rawr_toml::document;

use crate::input::membership::manifest::tests::admits_one_short;
use crate::input::membership::manifest::{session, storage, Error, Stop};
use crate::input::membership::Budget;
use crate::{input, node};

use super::exercise;

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

#[test]
fn refusal_is_permanent_and_precedes_every_unadmitted_native_allocation() {
    let prior = vec![document::Request::Items(9), document::Request::String(7)];
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
        assert!(
            matches!(exercise(&mut session), Err(Stop::Refused)),
            "{refusal}"
        );
        let allocations = storage::ALLOCATIONS.with(|count| count.get());
        let admitted = full.requests[prior.len()..refusal]
            .iter()
            .filter(|request| matches!(request, document::Request::Allocation { .. }))
            .count();
        assert_eq!(
            allocations, admitted,
            "allocation after refused request {refusal}"
        );
        assert!(matches!(exercise(&mut session), Err(Stop::Refused)));
        assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), allocations);
        assert!(matches!(session.finish(Ok(())), Err(Error::Admission(index)) if index == refusal));
        assert_eq!(account.requests, full.requests[..=refusal], "{refusal}");
    }
}

fn limited(
    items: usize,
    bytes: usize,
    string: usize,
) -> (bool, input::Usage, crate::exceedance::Record) {
    let graph = node::GraphBudget::new();
    graph.paths(node::MAX_PATH_CHECKS_PER_GRAPH).unwrap();
    graph
        .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
        .unwrap();
    let mut allocation = input::Allocation::with_limits(items, bytes, string);
    allocation.items(9).unwrap();
    allocation.bytes(27).unwrap();
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut session = session::Session::new(&mut budget);
    let result = exercise(&mut session);
    let success = session.finish(result).is_ok();
    assert!(allocation.check().is_ok(), "accounting targets never latch");
    (success, allocation.usage, allocation.exceedances())
}

#[test]
fn all_native_and_string_storage_uses_the_original_exact_remaining_account() {
    let (success, usage, _) = limited(usize::MAX, usize::MAX, usize::MAX);
    assert!(success);
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let result = exercise(&mut session);
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
        limited(usage.items, usage.bytes, string),
        (true, usage, crate::exceedance::Record::new())
    );
    // One short of an aggregate target admits and records that axis.
    admits_one_short(
        limited(usage.items - 1, usage.bytes, string),
        usage,
        input::INPUT_ITEMS_RESOURCE,
        usage.items,
    );
    admits_one_short(
        limited(usage.items, usage.bytes - 1, string),
        usage,
        input::INPUT_BYTES_RESOURCE,
        usage.bytes,
    );
    // One short of the longest string records the decoded-string axis
    // while the same native and string storage is admitted.
    let (success, spent, exceeded) = limited(usage.items, usage.bytes, string - 1);
    admits_one_short(
        (success, spent, exceeded),
        usage,
        "membership decoded TOML string bytes",
        string,
    );
    let axis = exceeded
        .axis("membership decoded TOML string bytes")
        .unwrap();
    assert_eq!(
        (axis.target, axis.first_requested),
        (string as u64 - 1, string as u64)
    );
}

#[test]
fn expansion_is_admitted_before_storage_and_empty_result_has_no_heap_allocation() {
    let mut account = Account {
        requests: Vec::new(),
        refusal: Some(2),
    };
    let mut session = session::Session::new(&mut account);
    storage::ALLOCATIONS.with(|count| count.set(0));
    assert!(matches!(
        super::join(
            std::path::Path::new("a"),
            std::path::Path::new("b"),
            &mut session
        ),
        Err(Stop::Refused)
    ));
    assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), 0);
    assert!(session.finish(Ok(())).is_err());
    assert_eq!(
        account.requests,
        [
            document::Request::String(3),
            document::Request::Items(1),
            document::Request::Allocation {
                bytes: 3,
                alignment: 1
            }
        ]
    );
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    assert!(super::normalize_text("", &mut session).unwrap().is_empty());
    session.finish(Ok(())).unwrap();
    assert!(!account
        .requests
        .iter()
        .any(|request| matches!(request, document::Request::Allocation { .. })));
}
