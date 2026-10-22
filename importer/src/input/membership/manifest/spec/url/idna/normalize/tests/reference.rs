//! Compare output and delegate consumption against both exact pinned ICU modes.

use std::cell::Cell;
use std::convert::Infallible;

use icu_normalizer::uts46::Uts46MapperBorrowed;
use rawr_toml::document;

use super::{start, Tapped};
use crate::input::membership::manifest::session;

struct Account;

impl document::Admission for Account {
    type Error = Infallible;

    fn admit(&mut self, _: document::Request) -> Result<(), Infallible> {
        Ok(())
    }
}

fn paired<I: Iterator<Item = char>, E: Iterator<Item = char>>(
    input: I,
    mut expected: E,
    validate: bool,
    expected_calls: &Cell<usize>,
    actual_calls: &Cell<usize>,
) {
    let mut account = Account;
    let mut session = session::Session::new(&mut account);
    let mut actual = start(input, validate, &mut session).unwrap();
    assert_eq!(
        actual_calls.get(),
        expected_calls.get(),
        "constructor priming"
    );
    let mut index = 0usize;
    loop {
        let expected = expected.next();
        let actual = actual.next(&mut session).unwrap();
        assert_eq!(actual, expected, "output {index}, validate={validate}");
        assert_eq!(
            actual_calls.get(),
            expected_calls.get(),
            "delegate at output {index}, validate={validate}"
        );
        if actual.is_none() {
            break;
        }
        index += 1;
    }
    for _ in 0..2 {
        assert_eq!(actual.next(&mut session).unwrap(), expected.next());
        assert_eq!(actual_calls.get(), expected_calls.get());
    }
    session.finish(Ok(())).unwrap();
}

fn compare<I: Iterator<Item = char> + Clone>(input: I) {
    let mapper = Uts46MapperBorrowed::new();
    for validate in [false, true] {
        let expected_calls = Cell::new(0);
        let actual_calls = Cell::new(0);
        let expected = Tapped {
            iter: input.clone(),
            count: &expected_calls,
        };
        let actual = Tapped {
            iter: input.clone(),
            count: &actual_calls,
        };
        if validate {
            paired(
                actual,
                mapper.normalize_validate(expected),
                validate,
                &expected_calls,
                &actual_calls,
            );
        } else {
            paired(
                actual,
                mapper.map_normalize(expected),
                validate,
                &expected_calls,
                &actual_calls,
            );
        }
    }
}

#[test]
fn complete_scalar_stream_matches_both_pinned_modes_without_fixture_collection() {
    // Separators prevent neighboring scalar mappings from hiding one another
    // through composition. Neither source nor expected output is collected.
    compare(
        (0..=0x10ffff)
            .filter_map(char::from_u32)
            .flat_map(|c| [c, ' ']),
    );
}

#[test]
fn pinned_contextual_cases_preserve_output_and_incremental_consumption() {
    for input in [
        "",
        "ASCII",
        "1234",
        "\u{ad}",
        "\u{ad}\u{ad}a",
        "a\u{ad}\u{301}",
        "\u{301}\u{300}\u{323}x",
        "\u{300}\u{301}\u{323}",
        "A\u{301}\u{323}\u{300}",
        "a\u{305}\u{30a}\u{301}",
        "\u{212b}\u{323}\u{301}",
        "\u{1e0a}\u{323}",
        "\u{1e0c}\u{307}",
        "\u{1100}\u{1161}\u{11a8}",
        "\u{ac00}\u{11a8}",
        "\u{ac01}\u{301}",
        "\u{3131}\u{314f}\u{3131}",
        "\u{320e}\u{301}",
        "\u{9c7}\u{9be}\u{301}",
        "\u{11099}\u{110ba}",
        "\u{340}\u{341}\u{343}\u{344}",
        "\u{f73}\u{f75}\u{f81}",
        "\u{ff76}\u{ff9e}\u{ff9f}",
        "\u{ff9e}\u{ff9f}a",
        "\u{fdfa}\u{fdfb}",
        "\u{1d160}\u{1d165}\u{1d16e}",
        "ßς\u{200c}\u{200d}",
        "ＡＢＣ\u{3002}ＤＥＦ",
        "\0_\u{ffff}\u{10ffff}",
        "\u{fffd}\u{301}x",
        "é\u{fffd}\u{ad}\u{301}",
    ] {
        compare(input.chars());
    }
    // Both equal-class stability and discontiguous composition are exercised
    // beyond upstream inline storage and ordinary stable-sort scratch sizes.
    compare(
        std::iter::once('A')
            .chain((0..4096).flat_map(|_| ['\u{301}', '\u{300}', '\u{323}', '\u{315}', '\u{316}'])),
    );
    compare(
        (0..4096)
            .flat_map(|_| ['\u{301}', '\u{300}', '\u{323}'])
            .chain(std::iter::once('x')),
    );
    compare((0..1024).flat_map(|_| ['\u{fdfa}', '\u{344}', '\u{f73}', '\u{ad}']));
}
