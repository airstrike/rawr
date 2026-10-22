use percent_encoding::{AsciiSet, CONTROLS};

use super::{Account, Output, Set};
use crate::input::membership::manifest::session;

// Exact reached set expressions from url 2.5.8 parser.rs:19–35,44–45.
const FRAGMENT: &AsciiSet = &CONTROLS.add(b' ').add(b'"').add(b'<').add(b'>').add(b'`');
const PATH: &AsciiSet = &FRAGMENT.add(b'#').add(b'?').add(b'{').add(b'}');
const SPECIAL_PATH_SEGMENT: &AsciiSet = &PATH.add(b'/').add(b'%').add(b'\\');
const USERINFO: &AsciiSet = &PATH
    .add(b'/')
    .add(b':')
    .add(b';')
    .add(b'=')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'|');
const QUERY: &AsciiSet = &CONTROLS.add(b' ').add(b'"').add(b'#').add(b'<').add(b'>');
const SPECIAL_QUERY: &AsciiSet = &QUERY.add(b'\'');

#[test]
fn every_byte_has_the_exact_pinned_percent_encoding_membership() {
    for (set, reference) in [
        (Set::Controls, CONTROLS),
        (Set::Userinfo, USERINFO),
        (Set::Path, PATH),
        (Set::SpecialPathSegment, SPECIAL_PATH_SEGMENT),
        (Set::Query, QUERY),
        (Set::SpecialQuery, SPECIAL_QUERY),
        (Set::Fragment, FRAGMENT),
    ] {
        for byte in 0..=u8::MAX {
            let bytes = [byte];
            let expected = percent_encoding::percent_encode(&bytes, reference).to_string();
            assert_eq!(set.encoded(byte), expected.len() == 3, "{set:?}, {byte}");
        }
    }
}

#[test]
fn complete_encoding_matches_pinned_iterator_across_ascii_and_unicode() {
    let mut sources = vec![
        String::new(),
        "%00%2f%ZZ%+?#/\\'`{}[]@:=;|".into(),
        "é💩終\u{85}\u{a0}\u{ad}\u{10ffff}".into(),
    ];
    for byte in 0..=127 {
        sources.push(format!("x{}z", char::from(byte)));
    }
    for (set, reference) in [
        (Set::Controls, CONTROLS),
        (Set::Userinfo, USERINFO),
        (Set::Path, PATH),
        (Set::SpecialPathSegment, SPECIAL_PATH_SEGMENT),
        (Set::Query, QUERY),
        (Set::SpecialQuery, SPECIAL_QUERY),
        (Set::Fragment, FRAGMENT),
    ] {
        for source in &sources {
            let mut account = Account::default();
            let mut session = session::Session::new(&mut account);
            let mut output = Output::new();
            output.append("prefix:", &mut session).unwrap();
            output.encode(source, set, &mut session).unwrap();
            let expected = format!(
                "prefix:{}",
                percent_encoding::utf8_percent_encode(source, reference)
            );
            assert_eq!(
                output.as_str(&mut session).unwrap(),
                expected,
                "{set:?}, {source:?}"
            );
            session.finish(Ok(())).unwrap();
        }
    }
}
