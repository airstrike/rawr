use super::{parse, Account, Invalid, Kind};
use crate::input::membership::manifest::session;

#[derive(Debug, Eq, PartialEq)]
enum Observed {
    Domain(Vec<u8>),
    Ipv4([u8; 4], String),
    Ipv6([u16; 8], String),
}

fn actual(input: &str, opaque: bool) -> Result<Observed, Invalid> {
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let value = parse(input, opaque, &mut session)
        .unwrap()
        .map(|value| match value.kind() {
            Kind::Domain(domain) => Observed::Domain(domain.as_bytes().to_vec()),
            Kind::Ipv4(address) => Observed::Ipv4(
                *address.octets(),
                address.serialize(&mut session).unwrap().as_str().to_owned(),
            ),
            Kind::Ipv6(address) => Observed::Ipv6(
                *address.segments(),
                format!("[{}]", address.serialize(&mut session).unwrap().as_str()),
            ),
        });
    session.finish(Ok(value)).unwrap()
}

fn pinned(input: &str, opaque: bool) -> Result<Observed, Invalid> {
    let value = if opaque {
        ::url::Host::parse_opaque(input)
    } else {
        ::url::Host::parse(input)
    };
    value
        .map(|value| {
            let spelling = value.to_string();
            match value {
                ::url::Host::Domain(value) => Observed::Domain(value.into_bytes()),
                ::url::Host::Ipv4(value) => Observed::Ipv4(value.octets(), spelling),
                ::url::Host::Ipv6(value) => Observed::Ipv6(value.segments(), spelling),
            }
        })
        .map_err(|error| match error {
            ::url::ParseError::EmptyHost => Invalid::Empty,
            ::url::ParseError::IdnaError => Invalid::Idna,
            ::url::ParseError::InvalidIpv4Address => Invalid::Ipv4,
            ::url::ParseError::InvalidIpv6Address => Invalid::Ipv6,
            ::url::ParseError::InvalidDomainCharacter => Invalid::Character,
            error => panic!("unexpected pinned Host error: {error}"),
        })
}

fn compare(input: &str) {
    for opaque in [false, true] {
        assert_eq!(
            actual(input, opaque),
            pinned(input, opaque),
            "{input:?}, opaque={opaque}"
        );
    }
}

#[test]
fn complete_special_and_opaque_outputs_and_errors_match_pinned_url() {
    for input in [
        "",
        ".",
        "..",
        "localhost",
        "EXAMPLE.COM",
        "example.com.",
        "example..com",
        "-x",
        "x-",
        "foo_bar",
        "a+b",
        "a!b",
        "a$b",
        "a&b",
        "a'b",
        "a(b)",
        "a*b",
        "a,b",
        "a;b",
        "a=b",
        "a\"b",
        "a`b",
        "a{b}",
        "0",
        "09",
        "0x",
        "0Xffffffff",
        "0x100000000",
        "0177.1",
        "127.0.0.1.",
        "1.2.3.256",
        "a.1",
        "a.0xg",
        "a.0x100000000",
        "a..",
        "1.2.3.4.5",
        "[",
        "[]",
        "[::]",
        "[::1]",
        "[::ffff:192.168.0.1]",
        "[1::2::3]",
        "[fe80::1%lo0]",
        "[::1]tail",
        "[::1%5D",
        "%5B::1%5D",
        "%",
        "%0",
        "%GG",
        "%25",
        "%2E",
        "%41",
        "%61",
        "%7F",
        "%FF",
        "%C0%AF",
        "%ED%A0%80",
        "%E2%82",
        "%F4%90%80%80",
        "prefix%FF",
        "%31%32%37.1",
        "%EF%BC%91",
        "a%2Eb",
        "b%C3%BCcher.example",
        "%E2%84%AA.example",
        "bücher.example",
        "faß.de",
        "BÜCHER.example",
        "a\u{301}.example",
        "\u{301}a",
        "ＡＢＣ.example",
        "ＦＯＯ。ＢＡＲ",
        "\u{ad}",
        "a\u{ad}b",
        "%C2%AD",
        "☃.example",
        "xn--bcher-kva.example",
        "XN--BCHER-KVA.example",
        "xn--",
        "xn--a-",
        "xn--tda",
        "xn--a",
        "a\u{200d}b",
        "ا.example",
        "אa.example",
        "क्\u{200d}क.example",
        "é\u{1}x/y",
    ] {
        compare(input);
    }
}

#[test]
fn every_ascii_input_byte_and_percent_decoded_byte_matches_pinned_url() {
    for byte in 0u8..=127 {
        let input = char::from(byte).to_string();
        compare(&input);
        compare(&format!("a{input}b"));
    }
    for byte in 0u8..=255 {
        compare(&format!("%{byte:02X}"));
        compare(&format!("a%{byte:02x}b"));
    }
}

#[test]
fn host_entry_keeps_url_preprocessing_outside_its_scope() {
    for input in [
        "\texample.com",
        "example.com\n",
        " example.com ",
        "a/b",
        "a?b",
        "a#b",
        "a:b",
        "u@h",
    ] {
        compare(input);
    }
    for scalar in ['é', '中', '𐐀', '\u{feff}', '\u{fffd}', char::MAX] {
        compare(&scalar.to_string());
        compare(&format!("a{scalar}.example"));
    }
}
