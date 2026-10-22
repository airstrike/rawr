use rawr_toml::document;

use super::{reference, v4, v6, Invalid};
use crate::input::membership::manifest::{session, Error, Step};

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

fn ipv4(input: &str) -> Result<([u8; 4], String), Invalid> {
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let result = v4::Address::parse(input, &mut session)
        .unwrap()
        .map(|value| {
            let text = value.serialize(&mut session).unwrap();
            (*value.octets(), text.as_str().to_owned())
        });
    session.finish(Ok(result)).unwrap()
}

fn ipv6(input: &str) -> Result<([u16; 8], String), Invalid> {
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let result = v6::Address::parse(input, &mut session)
        .unwrap()
        .map(|value| {
            let text = value.serialize(&mut session).unwrap();
            (*value.segments(), text.as_str().to_owned())
        });
    session.finish(Ok(result)).unwrap()
}

fn candidate(input: &str) -> bool {
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let result = v4::candidate(input, &mut session);
    session.finish(result).unwrap()
}

#[test]
fn numeric_candidate_is_distinct_from_valid_address() {
    for (input, expected) in [
        ("", false),
        (".", false),
        ("..", false),
        ("a", false),
        ("a.", false),
        ("a..", false),
        ("0", true),
        ("0.", true),
        ("0..", false),
        ("a.09", true),
        ("a.0x", true),
        ("a.0X", true),
        ("a.0xg", false),
        ("1.2.3.256", true),
        ("0x100000000", true),
        ("09", true),
        ("1.2.3.4.5", true),
        ("a.１２", false),
        ("0x１２", false),
    ] {
        assert_eq!(candidate(input), expected, "{input:?}");
        assert_eq!(candidate(input), reference::candidate(input), "{input:?}");
    }
    for prefix in ["", "name.", "name.name."] {
        for digits in ["9".repeat(400), "f".repeat(400), "0".repeat(400)] {
            for radix in ["", "0", "0x", "0X"] {
                for suffix in ["", "g", ".", ".."] {
                    let input = format!("{prefix}{radix}{digits}{suffix}");
                    assert_eq!(candidate(&input), reference::candidate(&input), "{input}");
                }
            }
        }
    }
    assert!(candidate("09"));
    assert_eq!(ipv4("09"), Err(Invalid::V4));
}

#[test]
fn ipv4_radix_width_trailing_and_invalid_cases_match_the_private_reference() {
    for input in [
        ".",
        "..",
        "0",
        "0.",
        "0..",
        "00",
        "0x",
        "0X",
        "0xff",
        "0177.1",
        "127.1",
        "127.0.1",
        "127.0.0.1",
        "127.0.0.1.",
        "1.2.3.4.5",
        "1.2.3.4.5.",
        "1..2",
        ".1",
        "1.2.",
        "1.2..",
        "4294967295",
        "4294967296",
        "0xffffffff",
        "0x100000000",
        "037777777777",
        "040000000000",
        "1.16777215",
        "1.16777216",
        "1.2.65535",
        "1.2.65536",
        "1.2.3.255",
        "1.2.3.256",
        "256.1",
        "1.256.1",
        "09",
        "08",
        "0xg",
        "0xFFFFFFFFz",
        "+1",
        "-0",
        " 1",
        "1 ",
        "1\n",
        "１",
        "é",
    ] {
        assert_eq!(ipv4(input), reference::v4(input), "{input:?}");
    }
    // The original Host rejects an empty domain before reaching its private
    // IPv4 parser, which otherwise panics on that unreachable input.
    assert_eq!(ipv4(""), Err(Invalid::V4));
    for width in 1..=4 {
        for value in [
            0u64, 1, 7, 8, 9, 10, 255, 256, 65535, 65536, 16777215, 16777216, 4294967295,
            4294967296,
        ] {
            for last in [
                value.to_string(),
                format!("0x{value:x}"),
                format!("0{value:o}"),
            ] {
                let prefix = "1.".repeat(width - 1);
                for suffix in ["", "."] {
                    let input = format!("{prefix}{last}{suffix}");
                    assert_eq!(ipv4(&input), reference::v4(&input), "{input}");
                }
            }
        }
    }
}

#[test]
fn ipv6_complete_grammar_and_embedded_ipv4_match_the_private_reference() {
    for input in [
        "",
        ":",
        "::",
        ":::",
        "1",
        "1:",
        ":1",
        "1::",
        "::1",
        "::1:",
        "1:::2",
        "1::2::3",
        "1:2:3:4:5:6:7:8",
        "1:2:3:4:5:6:7",
        "1:2:3:4:5:6:7:8:9",
        "1:2:3:4:5:6:7::",
        "1:2:3:4:5:6:7:8::",
        "::1:2:3:4:5:6:7",
        "::1:2:3:4:5:6:7:8",
        "0:0:0:0:0:0:0:0",
        "FFFF:ABCD:0000:0001:2:3:4:5",
        "fffff::",
        "g::",
        "[::1]",
        "fe80::1%lo0",
        " ::1",
        "::1\n",
        "é::",
        "１２::",
        "::ffff:192.168.0.1",
        "0:0:0:0:0:ffff:192.168.0.1",
        "1:2:3:4:5:6:192.168.0.1",
        "1:2:3:4:5:192.168.0.1",
        "1:2:3:4:5:6:7:192.168.0.1",
        "::192.168.0.1",
        "192.168.0.1",
        "::ffff:192.168.0.1.",
        "::ffff:192.168.0",
        "::ffff:192.168.0.1.2",
        "::ffff:192..0.1",
        "::ffff:192.168.0.256",
        "::ffff:192.168.00.1",
        "::ffff:0x1.2.3.4",
        "::ffff:1.2.3.-1",
        "::ffff:.1.2.3.4",
    ] {
        assert_eq!(ipv6(input), reference::v6(input), "{input:?}");
    }
    for prefix in [
        "::",
        "::ffff:",
        "1:2:3:4:5:6:",
        "1:2:3:4:5:",
        "1:2:3:4:5:6:7:",
    ] {
        for part in [
            "0", "00", "01", "1", "9", "10", "99", "100", "255", "256", "999", "9999",
        ] {
            for index in 0..4 {
                let mut pieces = ["1", "2", "3", "4"];
                pieces[index] = part;
                let input = format!("{prefix}{}", pieces.join("."));
                assert_eq!(ipv6(&input), reference::v6(&input), "{input}");
            }
        }
    }
}

#[test]
fn serialization_compresses_the_first_longest_run_and_never_a_lone_zero() {
    for mask in 0..256 {
        let pieces: Vec<_> = (0..8)
            .map(|index| {
                if mask & (1 << index) != 0 {
                    "0"
                } else {
                    "abcd"
                }
            })
            .collect();
        let input = pieces.join(":");
        let actual = ipv6(&input);
        assert_eq!(actual, reference::v6(&input), "{input}");
        assert!(actual.unwrap().1.len() <= 39);
    }
    for (input, expected) in [
        ("1:0:0:2:0:0:3:4", "1::2:0:0:3:4"),
        ("1:0:2:3:4:5:6:7", "1:0:2:3:4:5:6:7"),
        ("0:0:0:0:0:0:0:0", "::"),
        ("::ffff:192.168.0.1", "::ffff:c0a8:1"),
        ("1:2:3:4:5:6:0:0", "1:2:3:4:5:6::"),
    ] {
        assert_eq!(ipv6(input).unwrap().1, expected);
    }
    assert_eq!(
        ipv6("ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff")
            .unwrap()
            .1
            .len(),
        39
    );
    assert_eq!(ipv4("255.255.255.255").unwrap().1.len(), 15);
}

fn exercise<A: document::Admission>(
    input: &str,
    version6: bool,
    session: &mut session::Session<'_, A>,
) -> Step<()> {
    if version6 {
        if let Ok(value) = v6::Address::parse(input, session)? {
            value.serialize(session)?;
        }
    } else {
        v4::candidate(input, session)?;
        if let Ok(value) = v4::Address::parse(input, session)? {
            value.serialize(session)?;
        }
    }
    Ok(())
}

#[test]
fn semantic_invalidity_remains_available_for_a_later_trial() {
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    assert_eq!(
        v4::Address::parse("09", &mut session).unwrap(),
        Err(Invalid::V4)
    );
    assert_eq!(
        v6::Address::parse(":::1", &mut session).unwrap(),
        Err(Invalid::V6)
    );
    assert!(v4::Address::parse("127.1", &mut session).unwrap().is_ok());
    assert!(v6::Address::parse("::1", &mut session).unwrap().is_ok());
    let result: Result<(), Error<usize>> = session.finish(Ok(()));
    assert!(result.is_ok());
    assert_eq!(
        account
            .requests
            .iter()
            .filter(|request| matches!(request, document::Request::Items(1)))
            .count(),
        4
    );
}
