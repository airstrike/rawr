use rawr_toml::document;

use super::{exercise, Account};
use crate::input::membership::manifest::{session, storage, Error, Stop};

const SOURCE: &str = "profile.dev={unknown=1,build-override={debug='full',unknown=1},package={'foo@1.2'={opt-level=2},'*'={trim-paths=['object','macro']}},rustflags=['flag']}";

#[test]
fn every_recursive_trial_storage_and_callback_refusal_latches_original_first_cause() {
    let parsed = super::document(SOURCE);
    let prior = vec![document::Request::Items(13), document::Request::String(7)];
    let mut full = Account {
        requests: prior.clone(),
        refusal: None,
    };
    let mut session = session::Session::new(&mut full);
    let result = exercise(super::input(&parsed), &mut session);
    session.finish(result).unwrap();
    for refusal in prior.len()..full.requests.len() {
        let mut account = Account {
            requests: prior.clone(),
            refusal: Some(refusal),
        };
        let mut session = session::Session::new(&mut account);
        assert!(
            matches!(
                exercise(super::input(&parsed), &mut session),
                Err(Stop::Refused)
            ),
            "{refusal}"
        );
        let allocations = storage::ALLOCATIONS.with(|count| count.get());
        assert!(matches!(
            exercise(super::input(&parsed), &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(session.finish(Ok(())), Err(Error::Admission(index)) if index == refusal));
        assert_eq!(account.requests, full.requests[..=refusal], "{refusal}");
        assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), allocations);
    }
}
