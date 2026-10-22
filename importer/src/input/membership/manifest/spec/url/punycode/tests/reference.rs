//! Direct comparisons with the complete unchanged internal IDNA source.

use super::super::super::reference;
use super::super::{encode, Decoder, Invalid};
use super::Account;
use crate::input::membership::manifest::session;

fn bytes(input: &[u8]) {
    let mut pinned = reference::Decoder::default();
    let expected = pinned
        .decode::<u8, reference::InternalCaller>(input)
        .map(|decode| decode.collect::<Vec<_>>())
        .ok();
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let mut decoder = Decoder::new();
    let result = decoder.bytes(input, &mut session);
    let actual = session
        .finish(result)
        .unwrap()
        .map(|decode| decode.collect::<Vec<_>>())
        .ok();
    assert_eq!(actual, expected, "bytes {input:?}");
}

fn chars(input: &[char]) {
    let mut pinned = reference::Decoder::default();
    let expected = pinned
        .decode::<char, reference::InternalCaller>(input)
        .map(|decode| decode.collect::<Vec<_>>())
        .ok();
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let mut decoder = Decoder::new();
    let result = decoder.chars(input, &mut session);
    let actual = session
        .finish(result)
        .unwrap()
        .map(|decode| decode.collect::<Vec<_>>())
        .ok();
    assert_eq!(actual, expected, "chars {input:?}");
}

fn encoded(input: &[char]) {
    let mut expected = String::new();
    let pinned = reference::encode_into::<_, _, reference::InternalCaller>(
        input.iter().copied(),
        &mut expected,
    );
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let result = encode(input, &mut session);
    let actual = session.finish(result).unwrap();
    match (pinned, actual) {
        (Ok(()), Ok(actual)) => {
            assert_eq!(actual.as_bytes(), expected.as_bytes(), "encode {input:?}");
            if expected.len() <= 2000 {
                bytes(actual.as_bytes());
                chars(&expected.chars().collect::<Vec<_>>());
            }
        }
        (Err(reference::PunycodeEncodeError::Overflow), Err(Invalid::Overflow)) => {}
        _ => panic!("different encode acceptance for {input:?}"),
    }
}

#[test]
fn exact_internal_decoder_covers_every_byte_and_case_delimiter_position() {
    bytes(b"");
    chars(&[]);
    for byte in 0..=255 {
        for input in [
            vec![byte],
            vec![b'A', byte, b'-'],
            vec![b'a', b'-', byte],
            vec![b'a', byte, b'-', b'b'],
        ] {
            bytes(&input);
            chars(
                &input
                    .iter()
                    .map(|byte| char::from(*byte))
                    .collect::<Vec<_>>(),
            );
        }
    }
    for first in b"aAzZ09-!_" {
        for second in b"aAzZ09-!_" {
            for third in b"aAzZ09-!_" {
                let input = [*first, *second, *third];
                bytes(&input);
                chars(&input.map(char::from));
            }
        }
    }
    for input in [
        "BCHER-KVA",
        "bcher-kva",
        "-",
        "--",
        "a--",
        "a--a",
        "99999999999999999999",
        "abc-123456789",
        "💩-a",
        "Ü-",
        "Ü-a",
        "a-ü",
        "\u{10ffff}-a",
    ] {
        bytes(input.as_bytes());
        chars(&input.chars().collect::<Vec<_>>());
    }
    for length in [59, 60, 64, 255, 1000, 2000] {
        bytes(&vec![b'a'; length]);
        chars(&vec!['a'; length]);
    }
}

#[test]
fn exact_internal_encoder_covers_unicode_families_and_insertion_reordering() {
    for input in [
        "",
        "Bach",
        "ü",
        "üëäö♥",
        "bücher",
        "mañana",
        "例え",
        "日本語",
        "☃-⌘",
        "ليهمابتكلموشعربي؟",
        "他们为什么不说中文",
        "यहपरीक्षाहै",
        "💩💩",
        "\0a\u{7f}ü",
        "\u{80}\u{d7ff}\u{e000}\u{10ffff}",
    ] {
        encoded(&input.chars().collect::<Vec<_>>());
    }
    for scalar in [
        0x80, 0x81, 0xff, 0x100, 0x301, 0x800, 0xd7ff, 0xe000, 0xffff, 0x10000, 0x10ffff,
    ] {
        let c = char::from_u32(scalar).unwrap();
        encoded(&[c]);
        encoded(&['A', c, 'z', c, '\u{80}', c]);
        encoded(&[c; 60]);
    }
    for seed in 0..128u32 {
        let mut input = vec!['z', 'A', '-'];
        for index in 0..16u32 {
            let scalar = 0x80 + ((seed * 7919 + index * 104729) % (0x110000 - 0x80));
            if let Some(c) = char::from_u32(scalar) {
                input.push(c);
            }
        }
        encoded(&input);
        input.reverse();
        encoded(&input);
    }
    for length in [59, 60, 255, 1000] {
        let ascending = (0..length)
            .map(|index| char::from_u32(0x80 + index).unwrap())
            .collect::<Vec<_>>();
        encoded(&ascending);
        encoded(&ascending.iter().rev().copied().collect::<Vec<_>>());
    }
    encoded(&vec!['ü'; 1000]);
    encoded(&vec!['a'; 1001]);
    encoded(&vec!['a'; 3854]);
    encoded(&vec!['a'; 3855]);
}
