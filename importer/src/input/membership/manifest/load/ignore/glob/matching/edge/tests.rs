use super::{multi, Kind, Strategy};
use crate::input::membership::manifest::{reference::Account, session::Session, storage::Buffer};

fn build(patterns: &[(usize, &str)]) -> Strategy {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut source = multi::Builder::new();
    for &(index, text) in patterns {
        source.add(index, text.to_owned(), &mut session).unwrap();
    }
    Strategy::build(source, &mut session)
        .unwrap()
        .expect("edge engine")
}

#[test]
fn edges_filter_actual_overlap_order_on_truncated_byte_paths() {
    let prefix = build(&[(8, "a"), (3, "ab"), (5, "a")]);
    let mut found = Buffer::new();
    let mut account = Account;
    let mut session = Session::new(&mut account);
    prefix
        .matches_into(Kind::Prefix, b"abZ", &mut found, &mut session)
        .unwrap();
    assert_eq!(found.as_slice(), [8, 5, 3]);
    assert!(!prefix.is_match(Kind::Prefix, b"za", &mut session).unwrap());
    found.clear();
    prefix
        .matches_into(Kind::Suffix, b"aaab", &mut found, &mut session)
        .unwrap();
    assert_eq!(found.as_slice(), [3]);

    let suffix = build(&[(7, "é/x"), (4, "/x"), (2, "x")]);
    found.clear();
    suffix
        .matches_into(
            Kind::Suffix,
            b"\xffprefix/\xc3\xa9/x",
            &mut found,
            &mut session,
        )
        .unwrap();
    assert_eq!(found.as_slice(), [7, 4, 2]);
    assert!(suffix
        .is_match(Kind::Suffix, b"\xffprefix/\xc3\xa9/x", &mut session)
        .unwrap());
    assert!(!suffix.is_match(Kind::Suffix, b"x/", &mut session).unwrap());
}
