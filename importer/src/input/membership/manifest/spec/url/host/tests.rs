use rawr_toml::document;

use super::{Host, Invalid, Kind};
use crate::input::membership::manifest::{session, Step};

mod reference;
mod resources;

#[derive(Default)]
pub(super) struct Account {
    pub(super) requests: Vec<document::Request>,
    pub(super) refusal: Option<usize>,
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

fn parse<'input, A: document::Admission>(
    input: &'input str,
    opaque: bool,
    session: &mut session::Session<'_, A>,
) -> Step<Result<Host<'input>, Invalid>> {
    if opaque {
        Host::opaque(input, session)
    } else {
        Host::special(input, session)
    }
}

#[test]
fn unchanged_domains_borrow_exact_input_and_transformed_domains_own() {
    for (input, opaque, expected, borrowed) in [
        ("abc.example", false, "abc.example", true),
        (
            "xn--bcher-kva.example",
            false,
            "xn--bcher-kva.example",
            true,
        ),
        ("ABC.example", false, "abc.example", false),
        ("%61bc.example", false, "abc.example", false),
        ("%41bc.example", false, "abc.example", false),
        ("b%C3%BCcher.example", false, "xn--bcher-kva.example", false),
        ("", true, "", true),
        ("ABC.example", true, "ABC.example", true),
        ("%61bc.example", true, "%61bc.example", true),
        ("%ZZ", true, "%ZZ", true),
        ("\u{1}", true, "%01", false),
        ("\u{7f}", true, "%7F", false),
        ("é", true, "%C3%A9", false),
    ] {
        let mut account = Account::default();
        let mut session = session::Session::new(&mut account);
        let value = parse(input, opaque, &mut session).unwrap().unwrap();
        let Kind::Domain(domain) = value.kind() else {
            panic!("domain: {input}")
        };
        assert_eq!(domain.as_bytes(), expected.as_bytes(), "{input}");
        assert_eq!(domain.is_borrowed(), borrowed, "{input}");
        if borrowed {
            assert!(std::ptr::eq(domain.as_bytes(), input.as_bytes()));
        }
        session.finish(Ok(())).unwrap();
    }
}

#[test]
fn bracket_empty_idna_and_numeric_errors_preserve_the_selected_stage() {
    for (input, opaque, expected) in [
        ("[", false, Invalid::Ipv6),
        ("[", true, Invalid::Ipv6),
        ("[::1]tail", false, Invalid::Ipv6),
        ("[::1%5D", true, Invalid::Ipv6),
        ("[]", false, Invalid::Ipv6),
        ("[é]", false, Invalid::Ipv6),
        ("%5B::1%5D", false, Invalid::Idna),
        ("", false, Invalid::Empty),
        ("\u{ad}", false, Invalid::Empty),
        ("%C2%AD", false, Invalid::Empty),
        ("abc%FF", false, Invalid::Idna),
        ("1%FF", false, Invalid::Idna),
        ("09", false, Invalid::Ipv4),
        ("1.2.3.256", false, Invalid::Ipv4),
        ("1.2.3.4.5", false, Invalid::Ipv4),
        ("exa%mple", false, Invalid::Idna),
        ("a b", true, Invalid::Character),
        ("a\nb", true, Invalid::Character),
    ] {
        let mut account = Account::default();
        let mut session = session::Session::new(&mut account);
        let result = parse(input, opaque, &mut session).unwrap();
        assert!(
            matches!(result, Err(error) if error == expected),
            "{input:?}, opaque={opaque}"
        );
        // A semantic error permits another trial under the same original account.
        assert!(Host::opaque("next", &mut session).unwrap().is_ok());
        session.finish(Ok(())).unwrap();
    }
}

#[test]
fn opaque_forbidden_scan_finishes_before_any_encoding_storage() {
    let input = "é\u{1}x/y";
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let result = Host::opaque(input, &mut session).unwrap();
    assert!(matches!(result, Err(Invalid::Character)));
    session.finish(Ok(())).unwrap();
    assert_eq!(
        account.requests,
        [
            document::Request::String(input.len()),
            document::Request::Items(1)
        ]
    );
}

#[test]
fn ip_hosts_retain_typed_addresses_and_opaque_numeric_names_remain_domains() {
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let value = Host::special("%EF%BC%91", &mut session).unwrap().unwrap();
    assert!(matches!(value.kind(), Kind::Ipv4(address) if address.octets() == &[0,0,0,1]));
    let value = Host::opaque("127.1", &mut session).unwrap().unwrap();
    assert!(matches!(value.kind(), Kind::Domain(domain) if domain.as_bytes() == b"127.1"));
    for opaque in [false, true] {
        let value = parse("[::ffff:192.168.0.1]", opaque, &mut session)
            .unwrap()
            .unwrap();
        assert!(
            matches!(value.kind(), Kind::Ipv6(address) if address.segments() == &[0,0,0,0,0,0xffff,0xc0a8,1])
        );
    }
    session.finish(Ok(())).unwrap();
}
