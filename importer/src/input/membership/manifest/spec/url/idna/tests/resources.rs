use rawr_toml::document;

use super::{interpret, Account, Ascii};
use crate::input::membership::manifest::{session, storage, Error, Stop};

fn prior() -> Account {
    let mut account = Account::default();
    document::Admission::admit(&mut account, document::Request::Items(11)).unwrap();
    document::Admission::admit(&mut account, document::Request::String(7)).unwrap();
    account
}

#[test]
fn every_full_entry_refusal_stops_both_processing_and_delayed_output() {
    let cases = [
        "prefix.BÜCHER.Example".to_owned(),
        "a.ｘｎ－－bcher-kva\u{3002}A".to_owned(),
        "prefix.xn--aaaaa!".to_owned(),
        format!("a{}", "\u{315}\u{323}\u{301}".repeat(20)),
        format!("{}a", "\u{301}".repeat(60)),
    ];
    for source in cases {
        let mut full = prior();
        let _ = interpret(source.as_bytes(), &mut full).unwrap();
        assert!(full.requests.len() > 5, "{source}");
        for refusal in 2..full.requests.len() {
            let mut account = prior();
            account.refusal = Some(refusal);
            let mut session = session::Session::new(&mut account);
            storage::ALLOCATIONS.with(|count| count.set(0));
            assert!(
                matches!(
                    Ascii::interpret(source.as_bytes(), &mut session),
                    Err(Stop::Refused)
                ),
                "{source}: request {refusal}"
            );
            let allocations = storage::ALLOCATIONS.with(|count| count.get());
            assert!(matches!(
                Ascii::interpret(b"", &mut session),
                Err(Stop::Refused)
            ));
            assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), allocations);
            assert!(
                matches!(session.finish(Ok(())), Err(Error::Admission(index)) if index == refusal)
            );
            assert_eq!(
                account.requests,
                full.requests[..=refusal],
                "{source}: request {refusal}"
            );
            let expected = full.requests[2..refusal]
                .iter()
                .filter(|request| matches!(request, document::Request::Allocation { .. }))
                .count();
            assert_eq!(allocations, expected, "{source}: request {refusal}");
        }
    }
}

#[test]
fn borrowed_ascii_results_request_the_input_spelling_first() {
    for source in [
        "abc.example",
        "a123.example",
        "0.example",
        "_srv._tcp",
        "xn--bcher-kva.example",
    ] {
        let mut full = Account::default();
        let value = interpret(source.as_bytes(), &mut full).unwrap().unwrap();
        assert!(value.is_borrowed(), "{source}");
        assert_eq!(full.requests[0], document::Request::String(source.len()));
    }
}

#[test]
fn semantic_invalidity_remains_distinct_from_terminal_admission_refusal() {
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    assert!(matches!(Ascii::interpret(b"a/b", &mut session), Ok(Err(_))));
    let output = Ascii::interpret(b"abc", &mut session).unwrap().unwrap();
    assert_eq!(output.as_bytes(), b"abc");
    session.finish(Ok(())).unwrap();

    let mut full = Account::default();
    assert!(interpret(b"a/b", &mut full).unwrap().is_err());
    let refusal = full.requests.len() - 1;
    assert_eq!(full.requests[refusal], document::Request::Items(1));
    let mut refused = Account {
        refusal: Some(refusal),
        ..Account::default()
    };
    assert!(
        matches!(interpret(b"a/b", &mut refused), Err(Error::Admission(index)) if index == refusal)
    );
}
