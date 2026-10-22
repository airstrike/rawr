use super::super::{
    info,
    tests::{prepared, Count},
    Prepared,
};
use super::{table, Dfa, Error, Failure, Half, Overlapping, Session, Set};
use crate::input::membership::manifest::{reference::Account, Stop};

pub(super) fn engine(globs: &[&str], kind: info::Kind) -> Dfa {
    let (expressions, info, nfa) = prepared(globs, kind);
    let (_, dfa) = Prepared::begin(&info, &nfa, &expressions, &mut Session::new(&mut Account))
        .unwrap()
        .unwrap()
        .complete(&mut Session::new(&mut Account))
        .unwrap();
    assert!(
        dfa.has_engine(),
        "fixture must actually finish both dense DFAs"
    );
    dfa
}

fn ids(patterns: &Set) -> Vec<u32> {
    let mut cursor = 0;
    let mut ids = vec![];
    while let Some(id) = patterns
        .next(&mut cursor, &mut Session::new(&mut Account))
        .unwrap()
    {
        ids.push(id);
    }
    ids
}

fn patterns(dfa: &Dfa, bytes: &[u8], capacity: usize) -> Vec<u32> {
    let mut set = Set::new(capacity, &mut Session::new(&mut Account)).unwrap();
    dfa.overlapping(bytes, &mut set, &mut Session::new(&mut Account))
        .unwrap()
        .unwrap();
    ids(&set)
}

#[test]
fn completed_queries_match_whole_paths_empty_eoi_and_arbitrary_bytes() {
    let cases: &[(&str, &[&[u8]], &[&[u8]])] = &[
        ("", &[b""], &[b"a", b"\xff"]),
        (
            "*",
            &[b"", b"ab\n\x00\xff", "é".as_bytes()],
            &[b"a/b", b"/"],
        ),
        (
            "?",
            &[b"a", b"\xff", b"\n", b"\x00"],
            &[b"", b"ab", b"/", "é".as_bytes()],
        ),
        ("a", &[b"a"], &[b"", b"aa", b"ba", b"a\n"]),
        ("a*", &[b"a", b"ab\xff"], &[b"", b"ba", b"a/b\xff"]),
        ("*a", &[b"a", b"\xffa", b"ba"], &[b"", b"ab", b"a\n"]),
        ("**/x", &[b"x", b"a/x", b"a/b/x"], &[b"ax", b"x/a"]),
        ("*.rs", &[b"lib.rs"], &[b"rs", b"lib.rs.bak", b"a/b.rs"]),
        (
            "{é,中}",
            &["é".as_bytes(), "中".as_bytes()],
            &[b"", b"\xc3", b"\xff"],
        ),
    ];
    for &(glob, yes, no) in cases {
        let dfa = engine(&[glob], info::Kind::LeftmostFirst);
        for (&bytes, expected) in yes
            .iter()
            .map(|bytes| (bytes, true))
            .chain(no.iter().map(|bytes| (bytes, false)))
        {
            assert_eq!(
                dfa.is_match(bytes, &mut Session::new(&mut Account))
                    .unwrap(),
                Ok(expected),
                "glob={glob:?}, bytes={bytes:?}"
            );
            assert_eq!(!patterns(&dfa, bytes, 1).is_empty(), expected);
        }
    }
}

#[test]
fn all_byte_values_follow_byte_classes_and_eoi_in_both_query_shapes() {
    let dfa = engine(&["[a-c]", "?"], info::Kind::All);
    assert_eq!(patterns(&dfa, b"", 2), Vec::<u32>::new());
    for byte in 0..=255u8 {
        assert_eq!(
            dfa.is_match(&[byte], &mut Session::new(&mut Account))
                .unwrap(),
            Ok(byte != b'/')
        );
        assert_eq!(
            patterns(&dfa, &[byte], 2),
            if (b'a'..=b'c').contains(&byte) {
                vec![0, 1]
            } else if byte != b'/' {
                vec![1]
            } else {
                vec![]
            }
        );
        assert_eq!(patterns(&dfa, &[byte, byte], 2), Vec::<u32>::new());
    }
}

#[test]
fn overlapping_preserves_pattern_order_duplicate_ids_and_eoi_resumption() {
    let dfa = engine(&["a", "a", "*"], info::Kind::All);
    let dense = &dfa.completed.as_ref().unwrap().forward;
    let mut state = Overlapping::default();
    for pattern in 0..3 {
        assert_eq!(
            state
                .next(dense, b"a", &mut Session::new(&mut Account))
                .unwrap(),
            Ok(Some(Half { pattern, offset: 1 }))
        );
    }
    assert_eq!(
        state
            .next(dense, b"a", &mut Session::new(&mut Account))
            .unwrap(),
        Ok(None)
    );
    assert_eq!(patterns(&dfa, b"a", 3), [0, 1, 2]);
    assert_eq!(patterns(&dfa, b"", 3), [2]);
    let leftmost = engine(&["a", "a", "*"], info::Kind::LeftmostFirst);
    assert_eq!(patterns(&leftmost, b"a", 3), [0]);
}

#[test]
fn pattern_set_fullness_is_checked_after_each_match_and_existing_ids_survive() {
    let dfa = engine(&["a", "a", "*"], info::Kind::All);
    assert_eq!(patterns(&dfa, b"a", 1), [0]);
    assert_eq!(patterns(&dfa, b"a", 2), [0, 1]);
    let mut set = Set::new(4, &mut Session::new(&mut Account)).unwrap();
    set.insert(3, &mut Session::new(&mut Account))
        .unwrap()
        .unwrap();
    dfa.overlapping(b"a", &mut set, &mut Session::new(&mut Account))
        .unwrap()
        .unwrap();
    assert_eq!(ids(&set), [0, 1, 2, 3]);
}

#[test]
fn reached_insufficient_pattern_capacity_is_terminal_and_preserves_prior_ids() {
    let dfa = engine(&["a", "a", "*"], info::Kind::All);
    for (capacity, bytes) in [(0, b"a".as_slice()), (2, b"".as_slice())] {
        let mut set = Set::new(capacity, &mut Session::new(&mut Account)).unwrap();
        if capacity > 0 {
            set.insert(1, &mut Session::new(&mut Account))
                .unwrap()
                .unwrap();
        }
        let before = ids(&set);
        let mut account = Account;
        let mut session = Session::new(&mut account);
        assert!(matches!(
            dfa.overlapping(bytes, &mut set, &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(
            dfa.is_match(bytes, &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(session.finish(Ok(())), Err(Error::State)));
        assert_eq!(ids(&set), before);
    }
    // Capacity is checked when a match is inserted, not as an eager new guard.
    assert_eq!(
        patterns(&engine(&["a"], info::Kind::All), b"b", 0),
        Vec::<u32>::new()
    );

    // The same search reaches ID 0 before its out-of-range ID 2. Keep the
    // already recorded result on both internal Full and refusal at insertion.
    let dfa = engine(&["a", "b", "a"], info::Kind::All);
    let mut set = Set::new(2, &mut Session::new(&mut Account)).unwrap();
    let mut count = Count::default();
    let mut session = Session::new(&mut count);
    let result = dfa.overlapping(b"a", &mut set, &mut session);
    assert!(matches!(session.finish(result), Err(Error::State)));
    assert_eq!(ids(&set), [0]);
    let refused = count.calls - 1;
    let mut count = Count {
        refuse: Some(refused),
        ..Count::default()
    };
    let mut set = Set::new(2, &mut Session::new(&mut Account)).unwrap();
    let mut session = Session::new(&mut count);
    let result = dfa.overlapping(b"a", &mut set, &mut session);
    assert!(matches!(session.finish(result), Err(Error::Admission(call)) if call == refused));
    assert_eq!(ids(&set), [0]);
}

#[test]
fn accelerator_hits_and_misses_keep_the_same_languages_and_take_eoi() {
    // Cargo's literal_separator=true makes slash an exit needle too.
    for (glob, suffixes, base_matches) in [
        ("*", b"".as_slice(), true),
        ("*a", b"a".as_slice(), false),
        ("*{a,b}", b"ab".as_slice(), false),
    ] {
        let dfa = engine(&[glob], info::Kind::LeftmostFirst);
        assert!(dfa.completed.as_ref().unwrap().forward.accelerators.values[0] > 0);
        let mut bytes = vec![b'x'; 127];
        for suffix in suffixes {
            bytes.push(*suffix);
            assert_eq!(
                dfa.is_match(&bytes, &mut Session::new(&mut Account))
                    .unwrap(),
                Ok(true)
            );
            assert_eq!(patterns(&dfa, &bytes, 1), [0]);
            bytes.pop();
        }
        assert_eq!(
            dfa.is_match(&bytes, &mut Session::new(&mut Account))
                .unwrap(),
            Ok(base_matches)
        );
        assert_eq!(!patterns(&dfa, &bytes, 1).is_empty(), base_matches);
        // A real accelerated row, with its EOI destination injected to the
        // existing match row, makes skipping EOI after an accelerator miss
        // observable. This is a table fault fixture, not another search loop.
        let mut dfa = dfa;
        let dense = &mut dfa.completed.as_mut().unwrap().forward;
        let mut id = dense.starts.table.values[2];
        id = dense.transitions.values[id as usize + usize::from(dense.classes[usize::from(b'x')])];
        assert!(table::accelerated(dense, id));
        let eoi = usize::from(dense.classes[255]) + 1;
        dense.transitions.values[id as usize + eoi] = dense.special[2];
        assert_eq!(
            dfa.is_match(&bytes, &mut Session::new(&mut Account))
                .unwrap(),
            Ok(true)
        );
        assert_eq!(patterns(&dfa, &bytes, 1), [0]);
    }
}

#[test]
fn slash_plus_three_literal_exits_is_not_an_accelerated_state() {
    let dfa = engine(&["*{a,b,c}"], info::Kind::LeftmostFirst);
    assert_eq!(
        dfa.completed.as_ref().unwrap().forward.accelerators.values[0],
        0
    );
    for bytes in [b"xxa".as_slice(), b"xxb", b"xxc"] {
        assert_eq!(
            dfa.is_match(bytes, &mut Session::new(&mut Account))
                .unwrap(),
            Ok(true)
        );
        assert_eq!(patterns(&dfa, bytes, 1), [0]);
    }
    assert_eq!(
        dfa.is_match(b"x/a", &mut Session::new(&mut Account))
            .unwrap(),
        Ok(false)
    );
}

#[test]
fn earliest_returns_before_later_quit_but_overlap_preserves_prior_ids_on_quit() {
    let mut dfa = engine(&["a"], info::Kind::All);
    let dense = &mut dfa.completed.as_mut().unwrap().forward;
    let start = dense.starts.table.values[2];
    let after_a =
        dense.transitions.values[start as usize + usize::from(dense.classes[usize::from(b'a')])];
    let matched = dense.special[2];
    let other = usize::from(dense.classes[usize::from(b'!')]);
    dense.transitions.values[after_a as usize + other] = matched;
    dense.transitions.values[matched as usize + other] = dense.special[1];
    assert_eq!(
        dfa.is_match(b"a!!", &mut Session::new(&mut Account))
            .unwrap(),
        Ok(true)
    );
    let mut set = Set::new(2, &mut Session::new(&mut Account)).unwrap();
    let mut account = Account;
    let mut session = Session::new(&mut account);
    assert_eq!(
        dfa.overlapping(b"a!!", &mut set, &mut session).unwrap(),
        Err(Failure::Quit {
            byte: b'!',
            offset: 2
        })
    );
    assert!(session.check().is_ok());
    assert_eq!(ids(&set), [0]);
    // Saturation returns after the first match; the later quit is not reached.
    assert_eq!(patterns(&dfa, b"a!!", 1), [0]);
}

#[test]
fn quit_and_unsupported_start_are_semantic_but_unavailable_engine_is_not() {
    let mut dfa = engine(&["a"], info::Kind::All);
    let dense = &mut dfa.completed.as_mut().unwrap().forward;
    let start = dense.starts.table.values[2];
    let class = usize::from(dense.classes[usize::from(b'a')]);
    dense.transitions.values[start as usize + class] = dense.special[1];
    let mut account = Account;
    let mut session = Session::new(&mut account);
    assert_eq!(
        dfa.is_match(b"a", &mut session).unwrap(),
        Err(Failure::Quit {
            byte: b'a',
            offset: 0
        })
    );
    assert!(session.check().is_ok());
    let mut full = Set::new(1, &mut session).unwrap();
    full.insert(0, &mut session).unwrap().unwrap();
    // Automaton::try_which_overlapping_matches does not bypass its first
    // search merely because the supplied PatternSet is already full.
    assert_eq!(
        dfa.overlapping(b"a", &mut full, &mut session).unwrap(),
        Err(Failure::Quit {
            byte: b'a',
            offset: 0
        })
    );
    assert_eq!(ids(&full), [0]);
    // Synthetic EOI quit is not an input-byte quit in the pinned API.
    let dense = &mut dfa.completed.as_mut().unwrap().forward;
    let eoi = usize::from(dense.classes[255]) + 1;
    dense.transitions.values[start as usize + eoi] = dense.special[1];
    assert_eq!(dfa.is_match(b"", &mut session).unwrap(), Ok(false));
    dfa.completed.as_mut().unwrap().forward.starts.anchored_only = true;
    assert_eq!(
        dfa.is_match(b"a", &mut session).unwrap(),
        Err(Failure::UnsupportedAnchored)
    );
    assert!(session.check().is_ok());
    assert!(matches!(
        Dfa::none().is_match(b"a", &mut session),
        Err(Stop::Refused)
    ));
    assert!(matches!(session.finish(Ok(())), Err(Error::State)));
}

#[test]
fn each_query_admission_refusal_is_terminal_without_allocating_or_fallback() {
    let dfa = engine(&["*a", "*a"], info::Kind::All);
    let bytes = b"bbbbbbbbba";
    for overlap in [false, true] {
        let mut full = Count::default();
        let mut set = Set::new(2, &mut Session::new(&mut Account)).unwrap();
        {
            let mut session = Session::new(&mut full);
            if overlap {
                dfa.overlapping(bytes, &mut set, &mut session)
                    .unwrap()
                    .unwrap();
            } else {
                assert_eq!(dfa.is_match(bytes, &mut session).unwrap(), Ok(true));
            }
        }
        assert!(full.allocations.is_empty());
        for refuse in 0..full.calls {
            let mut count = Count {
                refuse: Some(refuse),
                ..Count::default()
            };
            let mut set = Set::new(2, &mut Session::new(&mut Account)).unwrap();
            let mut session = Session::new(&mut count);
            if overlap {
                assert!(matches!(
                    dfa.overlapping(bytes, &mut set, &mut session),
                    Err(Stop::Refused)
                ));
            } else {
                assert!(matches!(
                    dfa.is_match(bytes, &mut session),
                    Err(Stop::Refused)
                ));
            }
            assert!(matches!(
                dfa.is_match(bytes, &mut session),
                Err(Stop::Refused)
            ));
            assert!(
                matches!(session.finish(Ok(())), Err(Error::Admission(call)) if call == refuse)
            );
            assert_eq!(count.calls, refuse + 1);
            assert!(count.allocations.is_empty());
        }
    }
}

#[test]
fn completed_constructor_distinguishes_decline_from_pending_and_releases_borrows() {
    for (enabled, limit) in [
        (true, Some(40 * (1 << 10))),
        (false, Some(40 * (1 << 10))),
        (true, Some(0)),
    ] {
        let (expressions, mut info, nfa) = prepared(&["a"], info::Kind::All);
        info.test_engines(enabled, true);
        info.test_dfa_limit(limit);
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let (reverse, dfa) = Prepared::begin(&info, &nfa, &expressions, &mut session)
            .unwrap()
            .unwrap()
            .complete(&mut session)
            .unwrap();
        assert_eq!(dfa.has_engine(), enabled && limit != Some(0));
        assert!(reverse.0.reverse);
        assert_eq!(std::sync::Arc::strong_count(&nfa.0), 1);
        assert!(session.check().is_ok());
    }
}
