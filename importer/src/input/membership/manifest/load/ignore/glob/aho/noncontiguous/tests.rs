use super::super::{prepare, Abort, Failure, MAX};
use super::{pattern_fields, Nfa, Transition, FAIL};
use crate::input::membership::manifest::{reference::Account, session::Session};

fn compile(patterns: &[&str]) -> Nfa {
    let patterns = patterns.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    prepare(&patterns, &mut Session::new(&mut Account))
        .unwrap()
        .ok()
        .unwrap()
}

fn matches(nfa: &Nfa, state: u32) -> Vec<usize> {
    let mut found = Vec::new();
    let mut link = nfa.states.slice()[state as usize].matches;
    while link != 0 {
        let matched = nfa.matches.slice()[link as usize];
        found.push(matched.pattern as usize);
        link = matched.link;
    }
    found
}

// Constructor inspection only: this does not implement the production
// overlapping iterator or its prefilter scheduling.
fn step(nfa: &Nfa, mut state: u32, byte: u8) -> u32 {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    loop {
        let next = nfa.follow(state, byte, &mut session).unwrap();
        if next != FAIL {
            return next;
        }
        state = nfa.states.slice()[state as usize].fail;
    }
}

#[test]
fn conversion_borrows_walk_exact_order_and_refuse_before_advancing() {
    use super::super::tests::Count;
    use crate::input::membership::manifest::Stop;
    let nfa = compile(&["ab", "b", "ab"]);
    let a = nfa
        .next_state(nfa.special().start, b'a', &mut Session::new(&mut Account))
        .unwrap();
    let ab = nfa
        .next_state(a, b'b', &mut Session::new(&mut Account))
        .unwrap();
    let mut output = nfa.matches(ab);
    let mut found = Vec::new();
    while let Some(pattern) = output.next(&mut Session::new(&mut Account)).unwrap() {
        found.push(pattern);
    }
    assert_eq!(found, [0, 2, 1]);
    let mut transitions = nfa.transitions(nfa.special().start);
    let initial = transitions.link;
    let mut refused = Count {
        refuse: Some(0),
        ..Count::default()
    };
    let mut session = Session::new(&mut refused);
    assert!(matches!(transitions.next(&mut session), Err(Stop::Refused)));
    assert!(matches!(transitions.next(&mut session), Err(Stop::Refused)));
    assert_eq!(transitions.link, initial);
    assert!(session.finish(Ok(())).is_err());
    let mut transitions = nfa.transitions(nfa.special().start);
    let mut bytes = Vec::new();
    while let Some(transition) = transitions.next(&mut Session::new(&mut Account)).unwrap() {
        bytes.push(transition.byte());
    }
    assert_eq!(bytes, (0..=255).collect::<Vec<u8>>());
    assert!(nfa.states()[ab as usize].is_match());
}

#[test]
fn failure_links_append_suffix_matches_after_direct_and_duplicate_pattern_ids() {
    let nfa = compile(&["he", "she", "his", "hers", "he"]);
    let mut state = nfa.special.start;
    let mut found = Vec::new();
    for (offset, byte) in b"ushers".iter().copied().enumerate() {
        state = step(&nfa, state, byte);
        for pattern in matches(&nfa, state) {
            found.push((
                pattern,
                offset + 1 - nfa.pattern_lens.slice()[pattern] as usize,
                offset + 1,
            ));
        }
    }
    assert_eq!(found, [(1, 1, 4), (0, 2, 4), (4, 2, 4), (3, 2, 6)]);
    assert_eq!((nfa.minimum, nfa.maximum), (2, 4));
}

#[test]
fn empty_matches_follow_nonempty_outputs_once_at_every_trie_depth() {
    let nfa = compile(&["", "a", "aa", ""]);
    assert_eq!(matches(&nfa, nfa.special.start), [0, 3]);
    assert_eq!(matches(&nfa, nfa.special.anchored), [0, 3]);
    assert!(nfa.prefilter.is_none());
    assert_eq!(nfa.special.max_match, nfa.special.anchored);
    let a = step(&nfa, nfa.special.start, b'a');
    assert_eq!(matches(&nfa, a), [1, 0, 3]);
    let aa = step(&nfa, a, b'a');
    assert_eq!(matches(&nfa, aa), [2, 1, 0, 3]);
    let reset = step(&nfa, aa, b'z');
    assert_eq!(reset, nfa.special.start);
    assert_eq!(matches(&nfa, reset), [0, 3]);
}

#[test]
fn shuffled_dense_rows_keep_sparse_correspondence_and_the_source_depth_threshold() {
    let nfa = compile(&["abcdef", "abchij", "\0é\u{7f}", "z"]);
    assert_eq!(std::mem::size_of::<Transition>(), 9);
    assert_eq!(nfa.special.anchored, nfa.special.start + 1);
    for (index, state) in nfa.states.slice().iter().enumerate() {
        if index <= 1 {
            continue;
        }
        assert_eq!(state.dense != 0, state.depth < 3);
        assert_eq!(state.matches != 0, index <= nfa.special.max_match as usize);
        for byte in 0..=255 {
            let mut expected = FAIL;
            let mut link = state.sparse;
            while link != 0 {
                let transition = nfa.sparse.slice()[link as usize];
                if byte <= transition.byte {
                    if byte == transition.byte {
                        expected = transition.next;
                    }
                    break;
                }
                link = transition.link;
            }
            assert_eq!(
                nfa.follow(index as u32, byte, &mut Session::new(&mut Account))
                    .unwrap(),
                expected
            );
        }
    }
    assert_eq!(nfa.states.slice()[nfa.special.anchored as usize].fail, 0);
    assert_eq!(
        nfa.follow(nfa.special.anchored, b'!', &mut Session::new(&mut Account))
            .unwrap(),
        FAIL
    );
    assert_eq!(step(&nfa, nfa.special.start, b'!'), nfa.special.start);
}

#[test]
fn empty_pattern_set_keeps_sentinels_and_pattern_overflow_precedes_length_overflow() {
    let nfa = compile(&[]);
    assert_eq!(nfa.states.len(), 4);
    assert_eq!((nfa.minimum, nfa.maximum), (usize::MAX, 0));
    assert!(nfa.prefilter.is_none());
    assert_eq!(nfa.special.max_match, FAIL);
    assert_eq!(step(&nfa, nfa.special.start, 255), nfa.special.start);
    assert!(
        matches!(pattern_fields(MAX + 1, MAX + 1), Err(Abort::Semantic(Failure::PatternId { attempted })) if attempted == MAX + 1)
    );
    assert!(
        matches!(pattern_fields(7, MAX + 1), Err(Abort::Semantic(Failure::PatternLength { pattern: 7, length })) if length == MAX + 1)
    );
    assert!(
        matches!(super::super::state_id(MAX + 1), Err(Abort::Semantic(Failure::StateId { attempted })) if attempted == MAX + 1)
    );
}
