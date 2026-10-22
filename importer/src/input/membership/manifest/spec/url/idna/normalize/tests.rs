use std::cell::Cell;

use rawr_toml::document;

use super::{map_normalize, normalize_validate, Character, Normalizer};
use crate::input::membership::manifest::{session, Error, Step, Stop};

mod reference;

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

struct Tapped<'a, I> {
    iter: I,
    count: &'a Cell<usize>,
}

impl<I: Iterator<Item = char>> Iterator for Tapped<'_, I> {
    type Item = char;

    fn next(&mut self) -> Option<char> {
        self.count.set(self.count.get() + 1);
        self.iter.next()
    }
}

fn start<I: Iterator<Item = char>, A: document::Admission>(
    iter: I,
    validate: bool,
    session: &mut session::Session<'_, A>,
) -> Step<Normalizer<I>> {
    if validate {
        normalize_validate(iter, session)
    } else {
        map_normalize(iter, session)
    }
}

fn discard<I: Iterator<Item = char>, A: document::Admission>(
    iter: I,
    validate: bool,
    session: &mut session::Session<'_, A>,
) -> Step<()> {
    let mut normalizer = start(iter, validate, session)?;
    while normalizer.next(session)?.is_some() {}
    Ok(())
}

fn normalized(input: &str, validate: bool) -> String {
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let mut normalizer = start(input.chars(), validate, &mut session).unwrap();
    let mut output = String::new();
    while let Some(c) = normalizer.next(&mut session).unwrap() {
        output.push(c);
    }
    session.finish(Ok(())).unwrap();
    output
}

#[test]
fn modes_preserve_ignorables_mapping_replacement_and_composition() {
    for (input, mapped, validated) in [
        ("", "", ""),
        ("ABC", "abc", "abc"),
        ("a\u{ad}B", "ab", "a\u{fffd}b"),
        ("e\u{301}", "é", "é"),
        ("\u{1100}\u{1161}\u{11a8}", "각", "각"),
        ("ＡＢＣ", "abc", "abc"),
        (
            "ßς\u{200c}\u{200d}",
            "ßς\u{200c}\u{200d}",
            "ßς\u{200c}\u{200d}",
        ),
        ("\u{ffff}", "\u{fffd}", "\u{fffd}"),
        ("\0_", "\0_", "\0_"),
    ] {
        assert_eq!(normalized(input, false), mapped, "map {input:?}");
        assert_eq!(normalized(input, true), validated, "validate {input:?}");
    }
}

#[test]
fn stable_ccc_order_preserves_equal_class_sequence_and_special_expansions() {
    assert_eq!(
        normalized("\u{301}\u{300}\u{323}", false),
        "\u{323}\u{301}\u{300}"
    );
    assert_eq!(
        normalized("\u{300}\u{301}\u{323}", false),
        "\u{323}\u{300}\u{301}"
    );
    assert_eq!(
        normalized("\u{f73}\u{f75}\u{f81}", false),
        "\u{f71}\u{f71}\u{f71}\u{f72}\u{f80}\u{f74}"
    );
    assert_eq!(normalized("\u{ff76}\u{ff9e}", false), "ガ");
}

#[test]
fn priming_admits_leading_scratch_and_reuse_preserves_actual_capacity() {
    let count = Cell::new(0);
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let mut normalizer = map_normalize(
        Tapped {
            iter: "\u{301}\u{300}\u{323}x\u{301}\u{300}y".chars(),
            count: &count,
        },
        &mut session,
    )
    .unwrap();
    assert_eq!(
        count.get(),
        4,
        "priming consumes the leading sequence and pending starter"
    );
    assert_eq!(normalizer.buffer.len(), 3);
    while normalizer.next(&mut session).unwrap().is_some() {}
    session.finish(Ok(())).unwrap();
    let layouts = account
        .requests
        .iter()
        .filter_map(|request| match request {
            document::Request::Allocation { bytes, alignment } => Some((*bytes, *alignment)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let layout = std::alloc::Layout::array::<Character>(4).unwrap();
    assert_eq!(layouts, [(layout.size(), layout.align())]);
    assert_eq!(
        account
            .requests
            .iter()
            .filter(|request| matches!(request, document::Request::Items(1)))
            .count(),
        5
    );
    assert!(!account
        .requests
        .iter()
        .any(|request| matches!(request, document::Request::String(_))));
}

#[test]
fn every_constructor_and_next_refusal_stops_delegate_and_future_emissions() {
    for input in [
        "\u{301}\u{300}\u{323}\u{315}\u{316}x",
        "a\u{301}\u{300}\u{323}\u{315}\u{316}",
        "\u{fdfa}\u{344}\u{f73}",
    ] {
        for validate in [false, true] {
            let mut full = Account::default();
            let mut session = session::Session::new(&mut full);
            let result = discard(input.chars(), validate, &mut session);
            session.finish(result).unwrap();
            for refusal in 0..full.requests.len() {
                let count = Cell::new(0);
                let mut account = Account {
                    refusal: Some(refusal),
                    ..Account::default()
                };
                let mut session = session::Session::new(&mut account);
                let construction = start(
                    Tapped {
                        iter: input.chars(),
                        count: &count,
                    },
                    validate,
                    &mut session,
                );
                match construction {
                    Err(Stop::Refused) => {}
                    Err(_) => panic!("normalization has no semantic failure"),
                    Ok(mut normalizer) => {
                        loop {
                            match normalizer.next(&mut session) {
                                Ok(Some(_)) => {}
                                Err(Stop::Refused) => break,
                                _ => panic!("expected admission refusal"),
                            }
                        }
                        let stopped = count.get();
                        for _ in 0..3 {
                            assert!(matches!(normalizer.next(&mut session), Err(Stop::Refused)));
                            assert_eq!(count.get(), stopped);
                        }
                    }
                }
                let stopped = count.get();
                assert!(matches!(
                    map_normalize(
                        Tapped {
                            iter: "new".chars(),
                            count: &count
                        },
                        &mut session
                    ),
                    Err(Stop::Refused)
                ));
                assert_eq!(count.get(), stopped);
                assert!(
                    matches!(session.finish(Ok(())), Err(Error::Admission(index)) if index == refusal)
                );
                assert_eq!(account.requests, full.requests[..=refusal]);
            }
        }
    }
}

#[test]
fn original_account_preserves_prior_items_layouts_and_between_emission_refusal() {
    use crate::input;
    use crate::input::membership::manifest::tests::one_short;
    use crate::input::membership::Budget;
    use crate::node;

    let graph = node::GraphBudget::new();
    graph.paths(node::MAX_PATH_CHECKS_PER_GRAPH).unwrap();
    graph
        .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
        .unwrap();
    graph
        .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
        .unwrap();
    let exact_bytes = 19
        + std::alloc::Layout::array::<Character>(4).unwrap().size()
        + std::alloc::Layout::array::<Character>(8).unwrap().size();
    // One short of an item or byte target still admits every layout and
    // records only that axis at the exact total; nothing latches.
    for (items, bytes, exceeded) in [
        (12, exact_bytes, None),
        (11, exact_bytes, Some((input::INPUT_ITEMS_RESOURCE, 12))),
        (
            12,
            exact_bytes - 1,
            Some((input::INPUT_BYTES_RESOURCE, exact_bytes)),
        ),
    ] {
        let mut allocation = input::Allocation::with_limits(items, bytes, 0);
        allocation.items(7).unwrap();
        allocation.bytes(19).unwrap();
        let mut budget = Budget {
            graph: &graph,
            allocation: &mut allocation,
        };
        let mut session = session::Session::new(&mut budget);
        let result = discard(
            "\u{301}\u{300}\u{323}\u{315}\u{316}".chars(),
            false,
            &mut session,
        );
        session.finish(result).unwrap();
        assert_eq!(allocation.usage.items, 12);
        assert_eq!(allocation.usage.bytes, exact_bytes);
        assert!(allocation.check().is_ok(), "accounting targets never latch");
        match exceeded {
            Some((resource, total)) => {
                assert!(one_short(&allocation.exceedances(), resource, total))
            }
            None => assert!(!allocation.exceedances().is_exceeded()),
        }
    }

    // An exhausted target no longer refuses between emissions; a genuine
    // failure (checked-arithmetic overflow of the item account) does, and the
    // normalizer then pulls no further input.
    let count = Cell::new(0);
    let mut allocation = input::Allocation::with_limits(0, 0, 0);
    allocation.items(1).unwrap();
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut session = session::Session::new(&mut budget);
    let mut normalizer = map_normalize(
        Tapped {
            iter: "1234".chars(),
            count: &count,
        },
        &mut session,
    )
    .unwrap();
    assert_eq!(normalizer.next(&mut session).unwrap(), Some('1'));
    assert!(matches!(session.items(usize::MAX), Err(Stop::Refused)));
    let stopped = count.get();
    assert!(matches!(normalizer.next(&mut session), Err(Stop::Refused)));
    assert_eq!(count.get(), stopped);
    assert!(session.finish(Ok(())).is_err());
}
