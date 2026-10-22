use std::sync::Arc;

use rawr_toml::document::Request;

use super::super::{noncontiguous, prefilter::Prefilter, tests::Count, Abort};
use super::{words, Nfa, DENSE, FAIL, ONE};
use crate::input::membership::manifest::{reference::Account, session::Session, Error, Stop};

fn predecessor(patterns: &[&str]) -> noncontiguous::Nfa {
    let patterns = patterns
        .iter()
        .map(|pattern| pattern.to_string())
        .collect::<Vec<_>>();
    match noncontiguous::Nfa::build(&patterns, &mut Session::new(&mut Account)) {
        Ok(nfa) => nfa,
        Err(_) => panic!("noncontiguous fixture must build"),
    }
}

fn convert(predecessor: &noncontiguous::Nfa) -> Nfa {
    match Nfa::build(predecessor, &mut Session::new(&mut Account)) {
        Ok(nfa) => nfa,
        Err(_) => panic!("contiguous fixture must build"),
    }
}

#[test]
fn empty_and_single_literal_have_the_exact_source_word_layout() {
    let empty = convert(&predecessor(&[]));
    assert_eq!(empty.repr(), &[DENSE, 0, 0, DENSE, 0, 3, DENSE, 0, FAIL]);
    assert_eq!(empty.state_len(), 4); // FAIL occupies no encoded state.
    assert_eq!((empty.special().start, empty.special().anchored), (3, 6));
    assert_eq!(
        (empty.special().max_match, empty.special().max_special),
        (FAIL, FAIL)
    );
    assert_eq!((empty.minimum(), empty.maximum()), (usize::MAX, 0));
    assert!(empty.prefilter().is_none());

    let single = convert(&predecessor(&["a"]));
    assert_eq!(single.alphabet_len(), 3);
    assert_eq!(
        single.repr(),
        &[
            DENSE,
            0,
            0,
            0,
            0,
            DENSE,
            11,
            FAIL,
            FAIL,
            FAIL,
            1 << 31,
            DENSE,
            0,
            11,
            5,
            11,
            DENSE,
            0,
            FAIL,
            5,
            FAIL,
        ]
    );
    assert_eq!(single.pattern_lens(), &[1]);
    assert_eq!(
        (single.special().start, single.special().anchored),
        (11, 16)
    );
    assert_eq!(
        (single.special().max_match, single.special().max_special),
        (5, 16)
    );
}

// Inspect the wire layout using each predecessor state's match flag. This is
// constructor verification, not a production search or overlapping iterator.
fn offsets(old: &noncontiguous::Nfa, new: &Nfa) -> Vec<u32> {
    let mut offsets = Vec::new();
    let mut at = 0;
    for (index, state) in old.states().iter().enumerate() {
        if index == FAIL as usize {
            offsets.push(FAIL);
            continue;
        }
        offsets.push(at as u32);
        let kind = new.repr()[at] & 0xFF;
        let transition_words = match kind {
            DENSE => new.alphabet_len(),
            ONE => 1,
            count => words(count as usize) + count as usize,
        };
        at += 2 + transition_words;
        if state.is_match() {
            let packed = new.repr()[at];
            at += if packed & (1 << 31) != 0 {
                1
            } else {
                1 + packed as usize
            };
        }
    }
    assert_eq!(at, new.repr().len());
    offsets
}

#[test]
fn sparse_padding_one_edges_and_all_remapped_matches_preserve_the_predecessor() {
    let old = predecessor(&["abcx", "abcy", "abczz", "bcx", "abcx", "", "éx"]);
    let new = convert(&old);
    let remap = offsets(&old, &new);
    let mut saw_sparse_padding = false;
    let mut saw_one = false;
    let mut saw_multiple_matches = false;
    for (index, state) in old.states().iter().enumerate() {
        if index == FAIL as usize {
            continue;
        }
        let raw = &new.repr()[remap[index] as usize..];
        let kind = raw[0] & 0xFF;
        assert_eq!(raw[1], remap[state.fail() as usize]);
        let mut transitions = Vec::new();
        let mut cursor = old.transitions(index as u32);
        while let Some(transition) = cursor.next(&mut Session::new(&mut Account)).unwrap() {
            transitions.push((
                new.classes()[transition.byte() as usize],
                remap[transition.next() as usize],
            ));
        }
        let match_start = if kind == DENSE {
            let mut expected = vec![FAIL; new.alphabet_len()];
            for (class, next) in transitions {
                expected[class as usize] = next;
            }
            assert_eq!(&raw[2..][..new.alphabet_len()], expected);
            2 + new.alphabet_len()
        } else if kind == ONE {
            saw_one = true;
            assert!(!state.is_match());
            assert_eq!(transitions, [(((raw[0] >> 8) & 0xFF) as u8, raw[2])]);
            3
        } else {
            assert_eq!(kind as usize, transitions.len());
            let classes = raw[2..][..words(transitions.len())]
                .iter()
                .flat_map(|chunk| chunk.to_ne_bytes())
                .collect::<Vec<_>>();
            for (at, &(class, next)) in transitions.iter().enumerate() {
                assert_eq!(classes[at], class);
                assert_eq!(raw[2 + words(transitions.len()) + at], next);
            }
            if transitions.len() % 4 != 0 {
                saw_sparse_padding = true;
                let last = transitions.last().unwrap().0;
                assert!(classes[transitions.len()..]
                    .iter()
                    .all(|&class| class == last));
            }
            2 + words(transitions.len()) + transitions.len()
        };
        if state.is_match() {
            let mut expected = Vec::new();
            let mut cursor = old.matches(index as u32);
            while let Some(pattern) = cursor.next(&mut Session::new(&mut Account)).unwrap() {
                expected.push(pattern);
            }
            let packed = raw[match_start];
            if packed & (1 << 31) != 0 {
                assert_eq!(expected, [packed & !(1 << 31)]);
            } else {
                saw_multiple_matches = true;
                assert_eq!(packed as usize, expected.len());
                assert_eq!(&raw[match_start + 1..][..expected.len()], expected);
            }
        }
    }
    assert!(saw_sparse_padding);
    // Empty patterns make every trie state a match, suppressing ONE.
    assert!(!saw_one);
    assert!(saw_multiple_matches);

    let old = predecessor(&["abcdef"]);
    let new = convert(&old);
    assert!(offsets(&old, &new)
        .iter()
        .enumerate()
        .any(|(index, &at)| { index != FAIL as usize && new.repr()[at as usize] & 0xFF == ONE }));
}

#[test]
fn a_deep_state_with_128_edges_uses_dense_encoding_at_the_source_threshold() {
    let patterns = (0..128u8)
        .map(|byte| format!("abc{}", char::from(byte)))
        .collect::<Vec<_>>();
    let old = match noncontiguous::Nfa::build(&patterns, &mut Session::new(&mut Account)) {
        Ok(nfa) => nfa,
        Err(_) => panic!("wide trie fixture"),
    };
    let mut state = old.special().start;
    for &byte in b"abc" {
        state = old
            .next_state(state, byte, &mut Session::new(&mut Account))
            .unwrap();
    }
    assert_eq!(old.states()[state as usize].depth(), 2);
    let new = convert(&old);
    let remap = offsets(&old, &new);
    assert_eq!(new.repr()[remap[state as usize] as usize], DENSE);
}

#[test]
fn every_conversion_refusal_preserves_original_cause_and_shared_prefilter_ownership() {
    let old = predecessor(&["abcdef"]);
    let Prefilter::Memmem(finder) = old.prefilter().unwrap() else {
        panic!("memmem fixture");
    };
    assert_eq!(Arc::strong_count(finder), 1);
    let mut baseline = Count::default();
    let converted = match Nfa::build(&old, &mut Session::new(&mut baseline)) {
        Ok(nfa) => nfa,
        Err(_) => panic!("conversion baseline"),
    };
    let Prefilter::Memmem(shared) = converted.prefilter().unwrap() else {
        panic!("retained memmem");
    };
    assert!(Arc::ptr_eq(finder, shared));
    assert_eq!(Arc::strong_count(finder), 2);
    drop(converted);
    assert_eq!(Arc::strong_count(finder), 1);
    assert!(
        matches!(baseline.requests[1], Request::Allocation { bytes, .. } if bytes == old.states().len() * 4)
    );
    for index in 0..baseline.requests.len() {
        let mut account = Count {
            refuse: Some(index),
            ..Count::default()
        };
        let mut session = Session::new(&mut account);
        assert!(matches!(
            Nfa::build(&old, &mut session),
            Err(Abort::Stop(Stop::Refused))
        ));
        assert_eq!(Arc::strong_count(finder), 1);
        assert!(matches!(
            Nfa::build(&old, &mut session),
            Err(Abort::Stop(Stop::Refused))
        ));
        assert!(matches!(session.finish(Ok(())), Err(Error::Admission(at)) if at == index));
        assert_eq!(account.requests, baseline.requests[..index]);
        assert_eq!(account.calls, index + 1);
    }
    let converted = convert(&old);
    drop(old);
    let Prefilter::Memmem(retained) = converted.prefilter().unwrap() else {
        panic!("owned memmem");
    };
    assert_eq!(Arc::strong_count(retained), 1);
    assert_eq!(retained.needle(), b"abcdef");
}
