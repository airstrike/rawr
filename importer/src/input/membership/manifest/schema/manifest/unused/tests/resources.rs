use rawr_toml::document;

use crate::input::membership::manifest::{session, storage, Error, Stop};

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
