use rawr_toml::document;

use crate::input::membership::manifest::{session, storage, Error, Stop};

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
