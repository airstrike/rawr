use std::sync::Arc;

use rawr_toml::document::Request;

use super::super::{
    noncontiguous::Nfa, prefilter::Prefilter, prepare, tests::Count, Abort, Failure, MAX,
};
use super::{geometry, Dfa};
use crate::input::membership::manifest::{reference::Account, session::Session, Error, Stop};

fn nfa(patterns: &[&str]) -> Nfa {
    let patterns = patterns.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    prepare(&patterns, &mut Session::new(&mut Account))
        .unwrap()
        .ok()
        .unwrap()
}

fn convert(nfa: &Nfa) -> Dfa {
    Dfa::build(nfa, &mut Session::new(&mut Account))
        .ok()
        .unwrap()
}

fn patterns(dfa: &Dfa, state: u32) -> Vec<u32> {
    if !dfa.is_match(state) {
        return Vec::new();
    }
    (0..dfa.match_len(state))
        .map(|index| dfa.match_pattern(state, index))
        .collect()
}

#[test]
fn table_compiles_failure_walks_and_preserves_class_padding_and_sentinels() {
    let nfa = nfa(&["abcdef", "bc", "bchij", "\0é\u{7f}", "z"]);
    let dfa = convert(&nfa);
    assert_eq!(dfa.classes, *nfa.classes());
    assert_eq!(dfa.state_len, nfa.states().len());
    assert_eq!(dfa.start(), nfa.special().start << dfa.stride2);
    assert_eq!(dfa.special.anchored, 0);
    assert!(dfa.is_start(dfa.start()));
    assert_eq!(dfa.is_special(dfa.start()), dfa.prefilter().is_some());
    assert!(dfa.is_dead(0));
    assert!(!dfa.is_match(0));
    assert_eq!((dfa.minimum(), dfa.maximum()), (1, 6));
    assert_eq!(dfa.patterns_len(), 5);
    assert_eq!(dfa.pattern_len(3), 4);
    let stride = 1 << dfa.stride2;
    assert!(dfa.alphabet_len < stride);
    for old in 0..nfa.states().len() {
        let new = (old << dfa.stride2) as u32;
        for byte in 0..=255 {
            let actual = dfa
                .next_state(new, byte, &mut Session::new(&mut Account))
                .unwrap();
            // DEAD loops to itself and unused FAIL resolves to DEAD. The
            // retained anchored row uses the source direct-to-DEAD shortcut.
            let expected = if old <= 1 {
                0
            } else if old as u32 == nfa.special().anchored {
                let mut transitions = nfa.transitions(old as u32);
                let mut next = 0;
                while let Some(transition) =
                    transitions.next(&mut Session::new(&mut Account)).unwrap()
                {
                    if transition.byte() == byte {
                        next = if transition.next() == 1 {
                            0
                        } else {
                            transition.next()
                        };
                        break;
                    }
                }
                next
            } else {
                nfa.next_state(old as u32, byte, &mut Session::new(&mut Account))
                    .unwrap()
            };
            assert_eq!(actual, expected << dfa.stride2, "state {old}, byte {byte}");
            assert_eq!(actual as usize % stride, 0);
        }
        assert!(
            dfa.trans.slice()[new as usize + dfa.alphabet_len..new as usize + stride]
                .iter()
                .all(|&next| next == 0)
        );
    }
}

#[test]
fn converted_match_rows_keep_duplicates_suffix_order_and_empty_outputs() {
    let dfa = convert(&nfa(&["he", "she", "his", "hers", "he"]));
    let mut state = dfa.start();
    let mut found = Vec::new();
    for (offset, byte) in b"ushers".iter().copied().enumerate() {
        state = dfa
            .next_state(state, byte, &mut Session::new(&mut Account))
            .unwrap();
        for pattern in patterns(&dfa, state) {
            found.push((pattern, offset + 1 - dfa.pattern_len(pattern), offset + 1));
        }
    }
    assert_eq!(found, [(1, 1, 4), (0, 2, 4), (4, 2, 4), (3, 2, 6)]);
    let inner_bytes = dfa.matches.0.iter().map(|row| row.len() * 4).sum::<usize>();
    assert_eq!(dfa.matches_memory_usage, inner_bytes);
    assert_eq!(
        dfa.memory_usage(),
        dfa.trans.len() * 4
            + dfa.matches.0.len() * std::mem::size_of::<Vec<u32>>()
            + inner_bytes
            + dfa.patterns_len() * 4
            + dfa.prefilter().map_or(0, |p| p.memory_usage())
    );

    let dfa = convert(&nfa(&["", "a", "aa", ""]));
    assert!(dfa.prefilter().is_none());
    assert_eq!(patterns(&dfa, dfa.start()), [0, 3]);
    let mut state = dfa.start();
    for (byte, expected) in [
        (b'a', vec![1, 0, 3]),
        (b'a', vec![2, 1, 0, 3]),
        (b'z', vec![0, 3]),
    ] {
        state = dfa
            .next_state(state, byte, &mut Session::new(&mut Account))
            .unwrap();
        assert_eq!(patterns(&dfa, state), expected);
    }
    assert_eq!((dfa.minimum(), dfa.maximum()), (0, 2));

    let dfa = convert(&nfa(&[]));
    assert_eq!(dfa.patterns_len(), 0);
    assert_eq!((dfa.minimum(), dfa.maximum()), (usize::MAX, 0));
    assert!(!dfa.is_match(dfa.start()));
    assert!(patterns(&dfa, dfa.start()).is_empty());
}

#[test]
fn geometry_checks_the_last_premultiplied_id_before_output_allocation() {
    let states = MAX / 4 + 1;
    let (trans_len, shift) = geometry(states, 3).ok().unwrap();
    assert_eq!(shift, 2);
    assert_eq!(trans_len, states * 4);
    assert!(trans_len > MAX);
    assert!(trans_len - 4 <= MAX);
    assert!(matches!(
        geometry(states + 1, 3),
        Err(Abort::Semantic(Failure::StateId { attempted })) if attempted == states * 4
    ));
    assert_eq!(geometry(4, 1).ok(), Some((4, 0)));
    assert_eq!(geometry(4, 256).ok(), Some((1024, 8)));
}

#[test]
fn conversion_shares_the_prefilter_until_the_last_engine_is_dropped() {
    let nfa = nfa(&["needle"]);
    let Some(Prefilter::Memmem(original)) = nfa.prefilter() else {
        panic!("single literal retains its owned Finder");
    };
    assert_eq!(Arc::strong_count(original), 1);
    let dfa = convert(&nfa);
    let Some(Prefilter::Memmem(converted)) = dfa.prefilter() else {
        panic!("DFA retains the source prefilter variant");
    };
    assert!(Arc::ptr_eq(original, converted));
    assert_eq!(Arc::strong_count(converted), 2);
    drop(nfa);
    assert_eq!(Arc::strong_count(converted), 1);
    assert_eq!(converted.needle(), b"needle");
}

#[test]
fn conversion_refusal_unwinds_shared_ownership_and_keeps_the_original_terminal_prefix() {
    let nfa = nfa(&["needle"]);
    let Some(Prefilter::Memmem(original)) = nfa.prefilter() else {
        panic!("single literal retains its owned Finder");
    };
    let mut baseline = Count::default();
    drop(
        Dfa::build(&nfa, &mut Session::new(&mut baseline))
            .ok()
            .unwrap(),
    );
    let allocations: Vec<_> = baseline
        .requests
        .iter()
        .enumerate()
        .filter_map(|(index, request)| {
            matches!(request, Request::Allocation { .. }).then_some(index)
        })
        .collect();
    assert_eq!(allocations.len(), 4); // table, outer rows, lengths, inner row
    assert_eq!(baseline.requests[0], Request::Items(256));
    let row_bytes = nfa.special().max_match as usize - 1;
    assert_eq!(
        baseline.requests[allocations[1]],
        Request::Allocation {
            bytes: row_bytes * std::mem::size_of::<super::Buffer<u32>>(),
            alignment: std::mem::align_of::<super::Buffer<u32>>(),
        }
    );
    for index in [
        allocations[0],
        allocations[1],
        allocations[3],
        baseline.requests.len() - 1,
    ] {
        let mut account = Count {
            refuse: Some(index),
            ..Count::default()
        };
        let mut session = Session::new(&mut account);
        assert!(matches!(
            Dfa::build(&nfa, &mut session),
            Err(Abort::Stop(Stop::Refused))
        ));
        assert_eq!(Arc::strong_count(original), 1);
        assert!(matches!(
            Dfa::build(&nfa, &mut session),
            Err(Abort::Stop(Stop::Refused))
        ));
        assert!(matches!(session.finish(Ok(())), Err(Error::Admission(at)) if at == index));
        assert_eq!(account.requests, baseline.requests[..index]);
        assert_eq!(account.calls, index + 1);
    }
    let dfa = convert(&nfa);
    let mut account = Count {
        refuse: Some(0),
        ..Count::default()
    };
    let mut session = Session::new(&mut account);
    assert!(matches!(
        dfa.next_state(dfa.start(), b'n', &mut session),
        Err(Stop::Refused)
    ));
    assert!(matches!(
        dfa.next_state(dfa.start(), b'n', &mut session),
        Err(Stop::Refused)
    ));
    assert!(matches!(session.finish(Ok(())), Err(Error::Admission(0))));
    assert_eq!(account.calls, 1);
}
