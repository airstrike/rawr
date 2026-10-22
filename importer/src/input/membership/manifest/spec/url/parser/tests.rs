use rawr_toml::document;

use super::{Invalid, Url};
use crate::input::membership::manifest::{session, Step};

mod resources;

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

fn same(
    url: &Url,
    reference: &::url::Url,
    session: &mut session::Session<'_, Account>,
    source: &str,
) {
    assert_eq!(
        url.as_str(session).unwrap(),
        reference.as_str(),
        "serialization: {source:?}"
    );
    assert_eq!(
        url.scheme(session).unwrap(),
        reference.scheme(),
        "scheme: {source:?}"
    );
    assert_eq!(
        url.query(session).unwrap(),
        reference.query(),
        "query: {source:?}"
    );
    assert_eq!(
        url.fragment(session).unwrap(),
        reference.fragment(),
        "fragment: {source:?}"
    );
    assert_eq!(
        url.last_path_segment(session).unwrap(),
        reference
            .path_segments()
            .and_then(|mut parts| parts.next_back()),
        "last path: {source:?}"
    );
}

fn compare(source: &str) {
    for query_first in [false, true] {
        let mut account = Account::default();
        let mut session = session::Session::new(&mut account);
        match (
            Url::parse(source, &mut session).unwrap(),
            ::url::Url::parse(source),
        ) {
            (Ok(mut value), Ok(mut reference)) => {
                same(&value, &reference, &mut session, source);
                for query in [query_first, !query_first] {
                    if query {
                        value.remove_query(&mut session).unwrap();
                        reference.set_query(None);
                    } else {
                        value.remove_fragment(&mut session).unwrap();
                        reference.set_fragment(None);
                    }
                    same(&value, &reference, &mut session, source);
                }
            }
            (Err(actual), Err(expected)) => {
                assert_eq!(actual.to_string(), expected.to_string(), "{source:?}")
            }
            (Err(actual), Ok(expected)) => panic!(
                "{source:?}: rejected {actual}, reference {}",
                expected.as_str()
            ),
            (Ok(actual), Err(expected)) => panic!(
                "{source:?}: accepted {}, reference {expected}",
                actual.as_str(&mut session).unwrap()
            ),
        }
        session.finish(Ok(())).unwrap();
    }
}

#[test]
fn all_pinned_wpt_inputs_match_actual_no_base_reference_and_both_removal_orders() {
    let inputs: Vec<String> = serde_json::from_str(include_str!("tests/inputs.json")).unwrap();
    assert!(inputs.len() > 800);
    for source in &inputs {
        compare(source);
    }
}

#[test]
fn all_ascii_and_unicode_scalars_match_across_each_reached_state() {
    let mut cases = 0;
    for scalar in (0..=127).filter_map(char::from_u32).chain([
        'é',
        '💩',
        '\u{a0}',
        '\u{ad}',
        '\u{200c}',
        '終',
        '\u{10ffff}',
    ]) {
        for (before, after) in [
            ("http://a", "b.test/p?q#f"),
            ("custom://a", "b/p?q#f"),
            ("http://u", "p@host/p?q#f"),
            ("file:///a", "b/p?q#f"),
            ("custom:/a", "b/p?q#f"),
            ("custom:a", "b?q#f"),
            ("https://host/p?q=", "x#f"),
            ("custom://host/p?q=", "x#f"),
            ("custom://host/p#", "x"),
        ] {
            compare(&format!("{before}{scalar}{after}"));
            cases += 1;
        }
    }
    assert_eq!(cases, 135 * 9);
}

#[test]
fn full_scheme_file_authority_port_and_dot_cross_product_matches_reference() {
    let mut count = 0;
    for prefix in [
        "http:",
        "https:",
        "ftp:",
        "ws:",
        "wss:",
        "file:",
        "gopher:",
        "git+http:",
        "web+demo:",
        "a:",
    ] {
        for rest in [
            "",
            "?",
            "#",
            "?#",
            "x   ?#f",
            "/",
            "//",
            "///",
            "\\\\host\\a",
            "/.//host/",
            "/path/..//host/",
            "//u:@host:80/a",
            "//:p@host:00021/",
            "//@/",
            "//@@host/",
            "//u@/",
            "//host:/",
            "//host:65535/",
            "//host:65536/",
            "//host:80\\x",
            "//host:12x/",
            "//[::1]/",
            "//[::ffff:127.0.0.1]/",
            "//[::1]x/",
            "//a\t.\nhost/",
            "//localhost/c|/../x",
            "//HOST/C:/../x",
            "//C|/x",
            "//c:/x",
            "//host//c:/x",
            "///c|/x",
            "/c|/x",
            "c|/x",
            "//host/a/%2e/../b/%2E%2e/c?x=' #f\0",
            "//host/a//../x",
            "//host/a/.%2e/x",
            "//host/%2e%2e/",
            "//host/a/..",
            "//%61%00/foo",
            "//%EF%BC%91/",
            "//bücher.example/β#名字",
            "//09/",
            "//1.2.3.256/",
        ] {
            compare(&format!("{prefix}{rest}"));
            count += 1;
        }
    }
    assert!(count >= 400);
}

#[test]
fn serialization_equality_and_order_follow_the_pinned_url_value() {
    for (left, right) in [
        ("HTTP://EXAMPLE:80/a/../b", "http://example/b"),
        ("file://localhost/c|/x", "file:///c:/x"),
        ("x://EXAMPLE/", "x://example/"),
        ("x:a?", "x:a"),
        ("x:a#", "x:a"),
        ("http://host:81/", "http://host:80/"),
    ] {
        let mut account = Account::default();
        let mut session = session::Session::new(&mut account);
        let a = Url::parse(left, &mut session).unwrap().unwrap();
        let b = Url::parse(right, &mut session).unwrap().unwrap();
        let expected_a = ::url::Url::parse(left).unwrap();
        let expected_b = ::url::Url::parse(right).unwrap();
        assert_eq!(a == b, expected_a == expected_b);
        assert_eq!(a.cmp(&b), expected_a.cmp(&expected_b));
        session.finish(Ok(())).unwrap();
    }
}

#[test]
fn semantic_errors_allow_later_trials_without_erasing_spent_admissions() {
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    for (text, expected) in [
        ("relative", Invalid::Relative),
        ("http://", Invalid::EmptyHost),
        ("http://h:65536", Invalid::Port),
        ("http://a%00/x", Invalid::Idna),
    ] {
        assert!(matches!(Url::parse(text, &mut session).unwrap(), Err(error) if error == expected));
    }
    assert!(Url::parse("git+http://a%00/x", &mut session)
        .unwrap()
        .is_ok());
    session.finish(Ok(())).unwrap();
    assert!(
        account
            .requests
            .iter()
            .filter(|r| matches!(r, document::Request::Allocation { .. }))
            .count()
            > 4
    );
}

fn exercise<A: document::Admission>(session: &mut session::Session<'_, A>) -> Step<()> {
    let result = Url::parse(
        "git+ssh://u:p@Éx\tample:22/a/%2e/../β?q='x'\t#frag",
        session,
    )?;
    let Ok(mut value) = result else {
        panic!("valid resource fixture")
    };
    value.as_str(session)?;
    value.scheme(session)?;
    value.query(session)?;
    value.fragment(session)?;
    value.last_path_segment(session)?;
    value.remove_query(session)?;
    value.remove_fragment(session)?;
    Ok(())
}
