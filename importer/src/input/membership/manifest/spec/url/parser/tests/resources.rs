use rawr_toml::document;

use super::{exercise, Account, Url};
use crate::input::membership::manifest::{session, storage, Error, Stop};

#[test]
fn every_admission_refusal_preserves_prior_requests_and_stops_new_trials() {
    let prior = vec![document::Request::Items(13), document::Request::String(7)];
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
        assert!(matches!(
            Url::parse("http://next/", &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(session.finish(Ok(())), Err(Error::Admission(index)) if index == refusal));
        assert_eq!(account.requests, full.requests[..=refusal], "{refusal}");
        assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), allocations);
    }
}

#[test]
fn refused_session_blocks_every_borrow_and_removal_without_storage_or_mutation() {
    let source = "custom:opaque   ?query#fragment";
    let mut full = Account::default();
    let mut session = session::Session::new(&mut full);
    let value = Url::parse(source, &mut session).unwrap().unwrap();
    session.finish(Ok(())).unwrap();
    let before = value.output.as_bytes().to_vec();
    let mut account = Account {
        requests: Vec::new(),
        refusal: Some(full.requests.len()),
    };
    let mut session = session::Session::new(&mut account);
    let mut value = Url::parse(source, &mut session).unwrap().unwrap();
    assert!(matches!(session.items(1), Err(Stop::Refused)));
    storage::ALLOCATIONS.with(|count| count.set(0));
    assert!(matches!(value.as_str(&mut session), Err(Stop::Refused)));
    assert!(matches!(value.scheme(&mut session), Err(Stop::Refused)));
    assert!(matches!(value.query(&mut session), Err(Stop::Refused)));
    assert!(matches!(value.fragment(&mut session), Err(Stop::Refused)));
    assert!(matches!(
        value.last_path_segment(&mut session),
        Err(Stop::Refused)
    ));
    assert!(matches!(
        value.remove_query(&mut session),
        Err(Stop::Refused)
    ));
    assert!(matches!(
        value.remove_fragment(&mut session),
        Err(Stop::Refused)
    ));
    assert_eq!(value.output.as_bytes(), before);
    assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), 0);
    assert!(session.finish(Ok(())).is_err());
    assert_eq!(account.requests.len(), full.requests.len() + 1);
}

#[test]
fn absent_and_empty_tails_mutate_without_new_ownership_and_strip_only_opaque_spaces() {
    for source in [
        "x:opaque   ?#",
        "x:opaque   #",
        "x:opaque   ?",
        "x:/path%20%20?#",
        "x://host?#",
    ] {
        let mut account = Account::default();
        let mut session = session::Session::new(&mut account);
        let mut value = Url::parse(source, &mut session).unwrap().unwrap();
        let mut reference = ::url::Url::parse(source).unwrap();
        storage::ALLOCATIONS.with(|count| count.set(0));
        value.remove_query(&mut session).unwrap();
        reference.set_query(None);
        value.remove_fragment(&mut session).unwrap();
        reference.set_fragment(None);
        assert_eq!(
            value.as_str(&mut session).unwrap(),
            reference.as_str(),
            "{source}"
        );
        assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), 0);
        session.finish(Ok(())).unwrap();
    }
}
