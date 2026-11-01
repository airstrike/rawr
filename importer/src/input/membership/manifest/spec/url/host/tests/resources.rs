use rawr_toml::document;

use super::super::Host;
use super::{parse, Account};
use crate::input;
use crate::input::membership::manifest::{session, storage, Error, Stop};
use crate::input::membership::Budget;

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

#[test]
fn host_scalar_work_does_not_reset_or_spend_unrelated_graph_accounts() {
    for _ in 0..64 {}
    let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, usize::MAX);
    let mut budget = Budget {
        allocation: &mut allocation,
    };
    let mut session = session::Session::new(&mut budget);
    assert!(Host::special(SPECIAL, &mut session).unwrap().is_ok());
    assert!(Host::opaque(OPAQUE, &mut session).unwrap().is_ok());
    session.finish(Ok(())).unwrap();
}

fn opaque(source: &str) -> (bool, input::Usage) {
    let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, usize::MAX);
    allocation.items(3).unwrap();
    allocation.bytes(7).unwrap();
    let mut budget = Budget {
        allocation: &mut allocation,
    };
    let mut session = session::Session::new(&mut budget);
    let result = Host::opaque(source, &mut session);
    let okay = matches!(session.finish(result), Ok(Ok(_)));
    assert!(allocation.check().is_ok());
    (okay, allocation.usage)
}

#[test]
fn raw_and_expanded_strings_are_requested_and_the_owned_layout_completes() {
    let source = "é";
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
    assert_eq!(strings, [2, 6]);

    // The owned layout completes on the original account after its prior
    // usage.
    let (okay, usage) = opaque(source);
    assert!(okay);
    assert_eq!(
        usage,
        input::Usage {
            items: 10,
            bytes: 19
        }
    );
}
