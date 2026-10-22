//! Actual complete fixed-entry IDNA comparisons, including Cow ownership.

use std::borrow::Cow;

use super::{interpret, Account};

fn compare(input: &[u8]) {
    let expected =
        ::idna::domain_to_ascii_from_cow(Cow::Borrowed(input), ::idna::AsciiDenyList::URL);
    let mut account = Account::default();
    let actual = interpret(input, &mut account).unwrap();
    match (expected, actual) {
        (Ok(expected), Ok(actual)) => {
            assert_eq!(actual.as_bytes(), expected.as_bytes(), "input {input:?}");
            assert_eq!(
                actual.is_borrowed(),
                matches!(expected, Cow::Borrowed(_)),
                "ownership for {input:?}"
            );
            assert!(actual.as_bytes().is_ascii(), "input {input:?}");
        }
        (Err(_), Err(_)) => {}
        (Ok(expected), Err(_)) => panic!("contained rejection for {input:?}; pinned {expected:?}"),
        (Err(_), Ok(actual)) => panic!(
            "contained acceptance for {input:?}: {:?}",
            actual.as_bytes()
        ),
    }
}

#[test]
fn full_entry_matches_every_ascii_byte_and_mixed_passthrough_shape() {
    let mut compared = 0;
    for byte in 0..=127 {
        for input in [
            vec![byte],
            vec![b'a', byte, b'b'],
            vec![b'a', b'.', byte, b'.', b'Z'],
        ] {
            compare(&input);
            compared += 1;
        }
    }
    for input in [
        "",
        ".",
        "..",
        "abc.example",
        "a1.example",
        "0.example",
        "-a.example",
        "a-.example",
        "ab--cd.example",
        "_srv._tcp",
        "foo.A",
        "A.foo",
        "a.B.c.D.",
        "xn--bcher-kva.example",
        "XN--BCHER-KVA.Example",
        "a.xn--bcher-kva.A",
    ] {
        compare(input.as_bytes());
        compared += 1;
    }
    assert_eq!(compared, 400);
}

#[test]
fn full_entry_matches_unicode_mapping_composition_and_all_dot_forms() {
    let cases = [
        "bücher",
        "BÜCHER",
        "café",
        "cafe\u{301}",
        "例え.テスト",
        "日本語.jp",
        "faß.de",
        "σς.gr",
        "a\u{3002}b\u{ff0e}c\u{ff61}",
        "\u{3002}",
        "\u{ff61}\u{3002}",
        "\u{ff0e}A.",
        "ＡＢＣ.example",
        "ｘｎ－－bcher-kva",
        "xn--bcher-kva\u{ad}",
        "\u{ad}",
        "a\u{ad}b",
        "a\u{34f}b",
        "a\u{200b}b",
        "a\u{feff}b",
        "K.Å",
        "㍿.example",
        "ﬃ",
        "İ.example",
        "e\u{301}.A",
        "a\u{301}\u{323}",
        "a\u{323}\u{301}",
        "\u{301}a",
        "\u{ad}\u{301}a",
        "가.각",
        "각",
        "𐐀.𐐨",
        "💩.☃",
        "\u{10ffff}",
        "a\u{fffd}b",
    ];
    for input in cases {
        compare(input.as_bytes());
    }
    assert_eq!(cases.len(), 35);
    for length in [1, 17, 18, 59, 60, 127] {
        let suffix = "\u{301}\u{323}\u{315}".repeat(length);
        for starter in ["a", "", "\u{ad}", "가"] {
            compare(format!("{starter}{suffix}.example").as_bytes());
        }
    }
}

#[test]
fn full_entry_matches_contextj_bidi_and_punycode_validation() {
    let cases = [
        "abc.א",
        "1.א",
        "a-.א",
        "_.א",
        "א.abc",
        "א.1",
        "א.א",
        "ا١",
        "ا1",
        "ا١1",
        "ا1١",
        "ا\u{64b}",
        "a\u{64b}.א",
        "א-a",
        "a-א",
        "\u{200c}a",
        "a\u{200c}b",
        "a\u{200d}b",
        "ب\u{200c}ب",
        "ب\u{64b}\u{200c}\u{64b}ب",
        "क्\u{200c}क",
        "क्\u{200d}क",
        "xn--",
        "xn--a-",
        "xn---a",
        "xn--aaaaa!",
        "xn--a",
        "xn--9",
        "xn--99999999999999999999",
        "xn--bcher-kva",
        "xn--BCHER-KVA",
        "xn--fa-hia",
        "xn--caf-dma",
        "xn--a-ecp.ru",
        "xn--ls8h",
        "xn--\u{ff42}cher-kva",
        "xn--bcher-kva\u{3002}A",
        "a.xn--bcher-kva.א",
        "1.xn--4db",
    ];
    for input in cases {
        compare(input.as_bytes());
    }
    assert_eq!(cases.len(), 39);
}

#[test]
fn full_entry_matches_invalid_utf8_replacement_and_early_failure() {
    let mut compared = 0;
    for byte in 128..=255 {
        for input in [
            vec![byte],
            vec![b'a', byte, b'b'],
            vec![b'a', b'.', byte, b'.', b'Z'],
        ] {
            compare(&input);
            compared += 1;
        }
    }
    for suffix in [
        &[0xc2][..],
        &[0xe0, 0xa0][..],
        &[0xf0, 0x90, 0x80][..],
        &[0xc0, 0x80][..],
        &[0xe0, 0x80, 0x80][..],
        &[0xed, 0xa0, 0x80][..],
        &[0xf4, 0x90, 0x80, 0x80][..],
        &[0xf5, 0x80, 0x80, 0x80][..],
        &[0xe1, 0x80, b'.'][..],
        &[0xe1, b'A', 0x80][..],
    ] {
        for prefix in [b"".as_slice(), b"abc.", b"xn--", b"a"] {
            let mut input = prefix.to_vec();
            input.extend_from_slice(suffix);
            compare(&input);
            compared += 1;
        }
    }
    assert_eq!(compared, 424);
}

#[test]
fn full_entry_matches_scalar_families_and_caller_work_boundaries() {
    let scalars = [
        0x7f, 0x80, 0xa0, 0xad, 0xdf, 0x130, 0x301, 0x34f, 0x590, 0x5d0, 0x627, 0x64b, 0x661,
        0x94d, 0x200c, 0x200d, 0x200f, 0x212a, 0x3002, 0xd7ff, 0xe000, 0xfb1c, 0xff0e, 0xff61,
        0xfffd, 0xffff, 0x10000, 0x10400, 0x1e900, 0x1f000, 0x20000, 0x10ffff,
    ];
    let mut compared = 0;
    for scalar in scalars {
        let c = char::from_u32(scalar).unwrap();
        for input in [c.to_string(), format!("a{c}b"), format!("prefix.{c}.A")] {
            compare(input.as_bytes());
            compared += 1;
        }
    }
    for seed in 0..256u32 {
        let mut input = String::from("prefix.");
        for index in 0..8 {
            if let Some(c) =
                char::from_u32(0x80 + ((seed * 7919 + index * 104729) % (0x110000 - 0x80)))
            {
                input.push(c);
            }
        }
        input.push_str(".A");
        compare(input.as_bytes());
        compared += 1;
    }
    for length in [59, 60, 63, 64, 253, 254, 1000, 1001, 2000, 2001, 3855, 5000] {
        compare("a".repeat(length).as_bytes());
        compare(format!("{}..A", "a".repeat(length)).as_bytes());
        compare(format!("xn--{}", "a".repeat(length)).as_bytes());
        compared += 3;
    }
    for length in [999, 1000, 1001] {
        compare("ü".repeat(length).as_bytes());
        compare(format!("{}\u{ad}", "ü".repeat(length)).as_bytes());
        compared += 2;
    }
    assert_eq!(compared, 394);
}
