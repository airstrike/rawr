use rawr_toml::document;

use super::super::{text, v4, v6};
use super::{exercise, Account};
use crate::input;
use crate::input::membership::manifest::{session, storage, Error, Stop};
use crate::input::membership::Budget;

#[test]
fn every_request_refusal_preserves_prior_usage_and_stops_subsequent_operations() {
    for (source, version6) in [
        ("127.1", false),
        ("09", false),
        ("::ffff:192.168.0.1", true),
        ("1::2::3", true),
    ] {
        let prior = vec![document::Request::Items(7), document::Request::String(19)];
        let mut full = Account {
            requests: prior.clone(),
            refusal: None,
        };
        let mut session = session::Session::new(&mut full);
        let result = exercise(source, version6, &mut session);
        session.finish(result).unwrap();
        assert!(!full
            .requests
            .iter()
            .any(|request| matches!(request, document::Request::Allocation { .. })));
        for refusal in prior.len()..full.requests.len() {
            let mut account = Account {
                requests: prior.clone(),
                refusal: Some(refusal),
            };
            let mut session = session::Session::new(&mut account);
            storage::ALLOCATIONS.with(|count| count.set(0));
            assert!(matches!(
                exercise(source, version6, &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(
                v4::candidate("0", &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(
                v4::Address::parse("0", &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(
                v6::Address::parse("::", &mut session),
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
            assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), 0);
        }
    }
}

fn charged() -> (bool, input::Usage) {
    let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, usize::MAX);
    allocation.items(13).unwrap();
    allocation.bytes(31).unwrap();
    let mut budget = Budget {
        allocation: &mut allocation,
    };
    let mut session = session::Session::new(&mut budget);
    let result = (|| {
        let parsed = v6::Address::parse("ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff", &mut session)?;
        let Ok(value) = parsed else {
            panic!("valid address")
        };
        let mut retained = storage::Buffer::new();
        for _ in 0..5 {
            let text = value.serialize(&mut session)?;
            retained.push(text, &mut session)?;
        }
        assert_eq!(retained.len(), 5);
        Ok(())
    })();
    let okay = session.finish(result).is_ok();
    assert!(allocation.check().is_ok());
    (okay, allocation.usage)
}

#[test]
fn original_account_charges_items_and_caller_replacement_layouts() {
    let (okay, exact) = charged();
    assert!(okay);
    assert_eq!(exact.items, 24); // prior 13, one address, five texts, five slots
    let layout = std::alloc::Layout::array::<text::Text>(4).unwrap().size()
        + std::alloc::Layout::array::<text::Text>(8).unwrap().size();
    assert_eq!(exact.bytes, 31 + layout);
}

#[test]
fn fixed_text_checks_before_output_and_preserves_terminal_refusal() {
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let value = v6::Address::parse("::1", &mut session).unwrap().unwrap();
    session.finish(Ok(())).unwrap();
    for refusal in 0..2 {
        let mut account = Account {
            requests: Vec::new(),
            refusal: Some(refusal),
        };
        let mut session = session::Session::new(&mut account);
        assert!(matches!(value.serialize(&mut session), Err(Stop::Refused)));
        assert!(matches!(
            text::Text::render(|_| panic!("latched refusal must not emit"), &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(session.finish(Ok(())), Err(Error::Admission(index)) if index == refusal));
        assert_eq!(account.requests.len(), refusal + 1);
    }
}

#[test]
fn ip_conversion_does_not_spend_unrelated_graph_accounts() {
    for _ in 0..64 {}
    let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, usize::MAX);
    let mut budget = Budget {
        allocation: &mut allocation,
    };
    let mut session = session::Session::new(&mut budget);
    exercise("0x7f.1", false, &mut session).unwrap();
    exercise("::ffff:192.168.0.1", true, &mut session).unwrap();
    session.finish(Ok(())).unwrap();
}
