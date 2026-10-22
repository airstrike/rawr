use super::super::{
    tests::{prepared, Count},
    Config, Forward, Pending, Prepared,
};
use super::*;
use crate::input::membership::manifest::{reference::Account, Stop};
use rawr_toml::document::Request;
use std::sync::Arc;

fn construct(globs: &[&str], kind: info::Kind) -> (initial::Initial, Complete) {
    let (_, _, nfa) = prepared(globs, kind);
    build(&nfa, kind, false)
}

fn build(
    nfa: &finalize::Nfa,
    kind: info::Kind,
    anchored_only: bool,
) -> (initial::Initial, Complete) {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut dense = initial::Initial::new(nfa, anchored_only, &mut session)
        .unwrap()
        .unwrap();
    let subsets = run(nfa, &mut dense, kind, None, None, &mut session)
        .unwrap()
        .ok()
        .unwrap();
    (dense, subsets)
}

// Test-only inspection of raw rows, not an engine search/fallback API.
fn walk(dense: &initial::Initial, subsets: &Complete, group: usize, bytes: &[u8]) -> Vec<u32> {
    let mut state = dense.starts.table.values[group * 6 + initial::Start::Text as usize];
    for &byte in bytes {
        state = dense.transitions.values
            [state as usize + usize::from(dense.classes[usize::from(byte)])];
    }
    state = dense.transitions.values[state as usize + usize::from(dense.classes[255]) + 1];
    subsets
        .matches
        .as_slice()
        .iter()
        .find(|(id, _)| *id == state)
        .map_or_else(Vec::new, |(_, patterns)| patterns.values.clone())
}

#[test]
fn exact_literal_subset_rows_delay_matches_and_keep_dead_and_quit_distinct() {
    use super::super::super::builder::Id;
    let (_, _, mut nfa) = prepared(&["a"], info::Kind::LeftmostFirst);
    let graph = Arc::get_mut(&mut nfa.0).unwrap();
    // Preserve the emitted literal's byte classes; remove its anchors to make
    // the source subset IDs and delayed-match transition rows explicit.
    graph.states = vec![
        finalize::State::ByteRange {
            trans: super::super::super::builder::Transition {
                start: b'a',
                end: b'a',
                next: Id(1),
            },
        },
        finalize::State::Match { pattern: 0 },
    ];
    graph.starts = vec![Id(0)];
    graph.anchored = Id(0);
    graph.unanchored = Id(0);
    graph.looks = 0;
    graph.prefix_looks = 0;
    let (dense, subsets) = build(&nfa, info::Kind::LeftmostFirst, false);
    assert_eq!(dense.stride(), 4);
    assert_eq!(
        dense.transitions.values,
        [
            0, 0, 0, 0, // dead
            0, 0, 0, 0, // quit: same empty key, never interned
            0, 12, 0, 0, // literal start
            16, 16, 16, 16, // previous NFA match becomes a delayed DFA match
            0, 0, 0, 0,
        ]
    );
    assert!(dense.starts.table.values.iter().all(|&id| id == 8));
    assert_eq!(subsets.matches.as_slice().len(), 1);
    assert_eq!(subsets.matches.as_slice()[0].0, 16);
    assert_eq!(subsets.matches.as_slice()[0].1.values, [0]);
    assert_eq!(subsets.scratch.cache.len(), 0);
    assert!(subsets.scratch.uncompiled.values.is_empty());
    assert!(subsets.scratch.uncompiled.admitted > 0);
    assert_eq!(subsets.scratch.representatives.values.len(), 4);
    assert_eq!(dense.special, [0; 8]);
    assert!(dense.matches.patterns.values.is_empty());
}

#[test]
fn emitted_anchors_bytes_unicode_and_pattern_priority_survive_determinization() {
    for kind in [info::Kind::All, info::Kind::LeftmostFirst] {
        let (dense, subsets) =
            construct(&["a*", "a", "?", "", "{é,中}", "[!a]", "src/**/x.rs"], kind);
        for (bytes, mut expected) in [
            (b"a".as_slice(), vec![0, 1, 2]),
            (b"ab".as_slice(), vec![0]),
            (b"".as_slice(), vec![3]),
            ("é".as_bytes(), vec![4]),
            ("中".as_bytes(), vec![4]),
            (b"\xff".as_slice(), vec![2, 5]),
            (b"\n".as_slice(), vec![2, 5]),
            (b"/".as_slice(), vec![5]),
            (b"src/x.rs".as_slice(), vec![6]),
            (b"src/a/b/x.rs".as_slice(), vec![6]),
            (b"src/x.rs.backup".as_slice(), vec![]),
        ] {
            if kind == info::Kind::LeftmostFirst {
                expected.truncate(1);
            }
            assert_eq!(
                walk(&dense, &subsets, 0, bytes),
                expected,
                "{kind:?} {bytes:?}"
            );
            assert_eq!(
                walk(&dense, &subsets, 1, bytes),
                expected,
                "anchored {bytes:?}"
            );
        }
        assert!(subsets
            .matches
            .as_slice()
            .windows(2)
            .all(|pair| pair[0].0 < pair[1].0));
    }
}

#[test]
fn all_byte_classes_eoi_and_individual_pattern_starts_match_exact_languages() {
    let (dense, subsets) = construct(&["?", "[!a]", "a", ""], info::Kind::All);
    for byte in u8::MIN..=u8::MAX {
        let expected: Vec<_> = [(0, byte != b'/'), (1, byte != b'a'), (2, byte == b'a')]
            .into_iter()
            .filter_map(|(id, yes)| yes.then_some(id))
            .collect();
        assert_eq!(walk(&dense, &subsets, 0, &[byte]), expected);
        for pattern in 0..4 {
            assert_eq!(
                walk(&dense, &subsets, 2 + pattern, &[byte]),
                expected
                    .iter()
                    .copied()
                    .filter(|&id| id == pattern as u32)
                    .collect::<Vec<_>>()
            );
        }
    }
    assert_eq!(walk(&dense, &subsets, 0, b""), [3]);
    assert!(walk(&dense, &subsets, 0, "é".as_bytes()).is_empty());
}

#[test]
fn repeated_greedy_alternatives_preserve_exhaustive_short_string_language() {
    let (dense, subsets) = construct(&["a*a*a*b"], info::Kind::LeftmostFirst);
    for length in 0..=8 {
        for bits in 0..(1 << length) {
            let bytes: Vec<_> = (0..length)
                .map(|bit| if bits & (1 << bit) == 0 { b'a' } else { b'b' })
                .collect();
            let expected = bytes.first() == Some(&b'a')
                && bytes.last() == Some(&b'b')
                && bytes.iter().filter(|&&byte| byte == b'a').count() >= 3;
            assert_eq!(
                !walk(&dense, &subsets, 0, &bytes).is_empty(),
                expected,
                "{bytes:?}"
            );
        }
    }
    assert!(walk(&dense, &subsets, 0, b"aaa/b").is_empty());
}

#[test]
fn prepared_advances_to_unfinished_subsets_and_anchored_only_keeps_unused_group_zero() {
    let (expressions, info, nfa) = prepared(&["a"], info::Kind::LeftmostFirst);
    let stage = Prepared::begin(&info, &nfa, &expressions, &mut Session::new(&mut Account))
        .unwrap()
        .ok()
        .unwrap()
        .determinize(&mut Session::new(&mut Account))
        .unwrap();
    let Forward::Unshuffled(stage) = stage.forward else {
        panic!("expected unfinished subset graph");
    };
    assert_eq!(walk(&stage.pending.dense, &stage.subsets, 0, b"a"), [0]);
    let (dense, subsets) = build(&nfa, info::Kind::LeftmostFirst, true);
    assert_eq!(&dense.starts.table.values[..6], &[0; 6]);
    assert_eq!(walk(&dense, &subsets, 1, b"a"), [0]);
    assert_eq!(walk(&dense, &subsets, 2, b"a"), [0]);
}

#[test]
fn semantic_limits_run_after_state_publication_with_dfa_before_auxiliary() {
    let (_, _, nfa) = prepared(&["a"], info::Kind::LeftmostFirst);
    let mut traces = Vec::new();
    for (dfa_limit, aux_limit, failure) in [
        (Some(0), Some(0), initial::Failure::DfaSize(0)),
        (None, Some(0), initial::Failure::DeterminizeSize(0)),
    ] {
        let mut dense = initial::Initial::new(&nfa, false, &mut Session::new(&mut Account))
            .unwrap()
            .unwrap();
        let mut count = Count::default();
        let outcome = run(
            &nfa,
            &mut dense,
            info::Kind::LeftmostFirst,
            dfa_limit,
            aux_limit,
            &mut Session::new(&mut count),
        )
        .unwrap();
        assert!(matches!(outcome, Err(error) if error == failure));
        assert_eq!(dense.transitions.values.len(), 3 * dense.stride());
        assert!(dense.starts.table.values.iter().all(|&id| id == 0));
        // Two dead/quit keys, map, canonical vector, both sparse sets,
        // representatives, and the first new state's row/key/canonical growth
        // are already reached before either semantic limit can decline.
        assert!(count.allocations.len() >= 13);
        traces.push(count.allocations);
    }
    assert_eq!(traces[0], traces[1]);
    let (dense, _) = build(&nfa, info::Kind::LeftmostFirst, false);
    let exact = dense.memory_usage();
    for limit in [exact, exact - 1] {
        let mut dense = initial::Initial::new(&nfa, false, &mut Session::new(&mut Account))
            .unwrap()
            .unwrap();
        let outcome = run(
            &nfa,
            &mut dense,
            info::Kind::LeftmostFirst,
            Some(limit),
            None,
            &mut Session::new(&mut Account),
        )
        .unwrap();
        assert_eq!(outcome.is_ok(), limit == exact);
        if limit != exact {
            assert!(matches!(outcome, Err(initial::Failure::DfaSize(value)) if value == limit));
        }
    }
}

#[test]
fn every_subset_refusal_is_terminal_before_semantic_fallback_and_preserves_first_cause() {
    let (_, _, nfa) = prepared(&["a"], info::Kind::LeftmostFirst);
    let initial = || {
        initial::Initial::new(&nfa, false, &mut Session::new(&mut Account))
            .unwrap()
            .unwrap()
    };
    let mut baseline = Count::default();
    assert!(run(
        &nfa,
        &mut initial(),
        info::Kind::LeftmostFirst,
        None,
        None,
        &mut Session::new(&mut baseline)
    )
    .unwrap()
    .is_ok());
    for refuse in 0..baseline.calls {
        let mut count = Count {
            refuse: Some(refuse),
            ..Count::default()
        };
        let mut session = Session::new(&mut count);
        let result = run(
            &nfa,
            &mut initial(),
            info::Kind::LeftmostFirst,
            None,
            None,
            &mut session,
        );
        assert!(matches!(result, Err(Stop::Refused)), "refusal {refuse}");
        assert!(matches!(session.items(1), Err(Stop::Refused)));
        assert!(matches!(session.finish(result), Err(Error::Admission(cause)) if cause == refuse));
        assert_eq!(count.calls, refuse + 1);
        assert_eq!(Arc::strong_count(&nfa.0), 1);
    }
    let mut semantic = Count::default();
    assert!(matches!(
        run(
            &nfa,
            &mut initial(),
            info::Kind::LeftmostFirst,
            Some(0),
            Some(0),
            &mut Session::new(&mut semantic)
        )
        .unwrap(),
        Err(initial::Failure::DfaSize(0))
    ));
    for refuse in 0..semantic.calls {
        let mut count = Count {
            refuse: Some(refuse),
            ..Count::default()
        };
        assert!(matches!(
            run(
                &nfa,
                &mut initial(),
                info::Kind::LeftmostFirst,
                Some(0),
                Some(0),
                &mut Session::new(&mut count)
            ),
            Err(Stop::Refused)
        ));
    }
}

#[test]
fn original_account_floor_spans_eager_dense_builder_and_subset_ownership() {
    struct Budget {
        bytes: usize,
        items: usize,
        byte_limit: usize,
        item_limit: usize,
    }
    impl Admission for Budget {
        type Error = ();
        fn admit(&mut self, request: Request) -> Result<(), ()> {
            match request {
                Request::Allocation { bytes, .. } => {
                    if self.bytes + bytes > self.byte_limit {
                        return Err(());
                    }
                    self.bytes += bytes;
                }
                Request::Items(items) => {
                    if self.items + items > self.item_limit {
                        return Err(());
                    }
                    self.items += items;
                }
                _ => {}
            }
            Ok(())
        }
    }
    fn attempt<A: Admission>(nfa: &finalize::Nfa, session: &mut Session<'_, A>) -> Step<()> {
        session.items(11)?;
        session.admit(Request::Allocation {
            bytes: 37,
            alignment: 1,
        })?;
        let mut pending = Pending::begin(
            nfa,
            Config {
                kind: info::Kind::All,
                anchored_only: false,
                size_limit: None,
            },
            session,
        )?
        .ok()
        .unwrap();
        assert!(run(
            nfa,
            &mut pending.dense,
            info::Kind::All,
            None,
            None,
            session
        )?
        .is_ok());
        Ok(())
    }
    let (_, _, nfa) = prepared(&["a", "a"], info::Kind::All);
    let mut baseline = Count::default();
    attempt(&nfa, &mut Session::new(&mut baseline)).unwrap();
    let bytes = baseline
        .allocations
        .iter()
        .map(|&(bytes, _)| bytes)
        .sum::<usize>();
    for (byte_limit, item_limit, success) in [
        (bytes, baseline.items, true),
        (bytes - 1, baseline.items, false),
        (bytes, baseline.items - 1, false),
    ] {
        let mut budget = Budget {
            bytes: 0,
            items: 0,
            byte_limit,
            item_limit,
        };
        assert_eq!(
            attempt(&nfa, &mut Session::new(&mut budget)).is_ok(),
            success
        );
    }
}
