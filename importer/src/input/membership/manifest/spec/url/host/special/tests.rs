use rawr_toml::document;

use super::super::super::idna;
use super::super::{percent, tests};
use crate::input::membership::manifest::session;

#[test]
fn unchanged_idna_consumes_and_reuses_the_exact_decoded_owner_without_allocation() {
    let mut account = tests::Account::default();
    let mut session = session::Session::new(&mut account);
    let decoded = percent::decode(b"%61bc.example", &mut session).unwrap();
    let idna::Bytes::Owned(decoded) = decoded else {
        panic!("decoded owner")
    };
    let pointer = decoded.as_slice().as_ptr();
    session.finish(Ok(())).unwrap();
    let before = account.requests.len();
    let mut session = session::Session::new(&mut account);
    let output = super::normalize(idna::Bytes::Owned(decoded), &mut session)
        .unwrap()
        .unwrap();
    let idna::Bytes::Owned(output) = output else {
        panic!("retained owner")
    };
    assert_eq!(output.as_slice(), b"abc.example");
    assert_eq!(output.as_slice().as_ptr(), pointer);
    session.finish(Ok(())).unwrap();
    assert!(!account.requests[before..]
        .iter()
        .any(|request| matches!(request, document::Request::Allocation { .. })));
}

#[test]
fn changed_idna_moves_its_output_without_borrowing_the_decoded_temporary() {
    let mut account = tests::Account::default();
    let mut session = session::Session::new(&mut account);
    let decoded = percent::decode(b"%41bc.example", &mut session).unwrap();
    let idna::Bytes::Owned(decoded) = decoded else {
        panic!("decoded owner")
    };
    let output = super::normalize(idna::Bytes::Owned(decoded), &mut session)
        .unwrap()
        .unwrap();
    assert!(matches!(output, idna::Bytes::Owned(_)));
    assert_eq!(output.as_slice(), b"abc.example");
    session.finish(Ok(())).unwrap();
}
