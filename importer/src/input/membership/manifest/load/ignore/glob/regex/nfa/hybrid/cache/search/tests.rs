use super::super::super::super::{builder, finalize};
use super::super::super::{info, machine, Attempt, Hybrid};
use super::*;
use crate::input::membership::manifest::{reference::Account, storage::Buffer, Stop};
use rawr_toml::document::Request;

mod epochs;

#[derive(Default)]
struct Count {
    requests: Vec<Request>,
    calls: usize,
    refuse: Option<usize>,
}
impl Admission for Count {
    type Error = usize;
    fn admit(&mut self, request: Request) -> Result<(), usize> {
        let call = self.calls;
        self.calls += 1;
        if self.refuse == Some(call) {
            return Err(call);
        }
        self.requests.push(request);
        Ok(())
    }
}

fn hybrid(globs: &[&str], kind: info::Kind) -> Hybrid {
    let (info, nfa, reverse) = machine::tests::prepared(globs, kind);
    Hybrid::new(&info, &nfa, &reverse, &mut Session::new(&mut Account)).unwrap()
}

fn machine(globs: &[&str], kind: info::Kind, minimum: bool) -> Machine {
    let (_, nfa, _) = machine::tests::prepared(globs, kind);
    build(&nfa, kind, minimum)
}

fn build(nfa: &finalize::Nfa, kind: info::Kind, minimum: bool) -> Machine {
    let capacity = if minimum {
        match Machine::build(nfa, kind, 0, &mut Session::new(&mut Account)).unwrap() {
            Err(machine::Failure::CacheCapacity { minimum, .. }) => minimum,
            _ => panic!("expected the pinned capacity floor"),
        }
    } else {
        10 * (1 << 20)
    };
    Machine::build(nfa, kind, capacity, &mut Session::new(&mut Account))
        .unwrap()
        .unwrap()
}

// A tiny Thompson producer for testing delayed non-EOI matches and branch
// priorities. Public query producers remain the emitted whole-path globs.
fn prefix(patterns: &[&[u8]], reversed: bool, kind: info::Kind) -> Machine {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut builder = builder::Builder::new();
    let mut starts = Buffer::new();
    for pattern in patterns {
        builder.start_pattern(&mut session).unwrap().unwrap();
        let mut next = builder.add_match(&mut session).unwrap().unwrap();
        for &byte in pattern.iter().rev() {
            next = builder
                .add(
                    builder::State::ByteRange {
                        trans: builder::Transition {
                            start: byte,
                            end: byte,
                            next,
                        },
                    },
                    &mut session,
                )
                .unwrap()
                .unwrap();
        }
        let start = builder.capture_start(next, &mut session).unwrap().unwrap();
        builder.finish_pattern(start, &mut session).unwrap();
        starts.push(start, &mut session).unwrap();
    }
    let state = if reversed {
        builder::State::UnionReverse { alternates: starts }
    } else {
        builder::State::Union { alternates: starts }
    };
    let start = builder.add(state, &mut session).unwrap().unwrap();
    let nfa = builder
        .finalize(start, start, false, &mut session)
        .unwrap()
        .unwrap();
    build(&nfa, kind, false)
}

fn ids(set: &pike::set::Set) -> Vec<u32> {
    let mut output = Vec::new();
    let mut cursor = 0;
    while let Some(id) = set
        .next(&mut cursor, &mut Session::new(&mut Account))
        .unwrap()
    {
        output.push(id);
    }
    output
}

#[test]
fn emitted_glob_language_keeps_bytes_unicode_empty_and_eoi_with_retained_pair_caches() {
    let globs = ["a*", "a", "?", "", "{é,中}", "[!a]", "src/**/x.rs"];
    let re = hybrid(&globs, info::Kind::All);
    let mut cache = re.create_cache(&mut Session::new(&mut Account)).unwrap();
    let mut set = pike::set::Set::new(globs.len(), &mut Session::new(&mut Account)).unwrap();
    for (input, expected) in [
        (&b"a"[..], vec![0, 1, 2]),
        (&b"ab"[..], vec![0]),
        (&b""[..], vec![3]),
        ("é".as_bytes(), vec![4]),
        ("中".as_bytes(), vec![4]),
        (&b"\xff"[..], vec![2, 5]),
        (&b"\n"[..], vec![2, 5]),
        (&b"/"[..], vec![5]),
        (&b"src/x.rs"[..], vec![6]),
        (&b"src/a/b/x.rs"[..], vec![6]),
        (&b"src/x.rs.backup"[..], vec![]),
    ] {
        set.clear(&mut Session::new(&mut Account)).unwrap();
        re.overlapping(&mut cache, input, &mut set, &mut Session::new(&mut Account))
            .unwrap()
            .unwrap();
        assert_eq!(ids(&set), expected, "input={input:?}");
        assert_eq!(
            re.is_match(&mut cache, input, &mut Session::new(&mut Account))
                .unwrap(),
            Ok(!expected.is_empty())
        );
    }
    let pair = cache.0.as_ref().unwrap();
    assert_eq!(pair.forward.clear_count, 0);
    assert!(pair.forward.progress.is_none());
    assert_eq!(pair.reverse.states.values.len(), 3);
    assert_eq!(pair.reverse.bytes_searched, 0);
    assert!(pair.reverse.starts.values.iter().all(|&id| id == UNKNOWN));
    let none = hybrid(&[], info::Kind::All);
    let mut empty = none.create_cache(&mut Session::new(&mut Account)).unwrap();
    assert_eq!(
        none.is_match(&mut empty, b"", &mut Session::new(&mut Account))
            .unwrap(),
        Ok(false)
    );
}

#[test]
fn start_groups_and_lookbehind_classes_are_cached_separately_without_start_tags() {
    let dfa = machine(&["a", "b"], info::Kind::All, false);
    let mut cache = Cache::new(&dfa, &mut Session::new(&mut Account)).unwrap();
    for (mode, group) in [
        (Anchored::No, 0),
        (Anchored::Yes, 6),
        (Anchored::Pattern(0), 12),
        (Anchored::Pattern(1), 18),
    ] {
        for (behind, index) in [
            (Some(b'!'), 0),
            (Some(b'a'), 1),
            (None, 2),
            (Some(b'\n'), 3),
            (Some(b'\r'), 4),
        ] {
            let id = cache
                .start(&dfa, mode, behind, &mut Session::new(&mut Account))
                .unwrap()
                .unwrap();
            assert_eq!(id & (START | MATCH | UNKNOWN), 0);
            assert_eq!(cache.starts.values[group + index], id);
            let mut count = Count::default();
            assert_eq!(
                cache
                    .start(&dfa, mode, behind, &mut Session::new(&mut count))
                    .unwrap(),
                Ok(id)
            );
            assert_eq!(count.requests, [Request::Items(1)]);
        }
        assert_eq!(cache.starts.values[group + 5], UNKNOWN);
    }
    assert_eq!(
        cache
            .start(
                &dfa,
                Anchored::Pattern(2),
                None,
                &mut Session::new(&mut Account)
            )
            .unwrap(),
        Ok(DEAD | dfa.stride() as u32)
    );
    let id = cache
        .start(
            &dfa,
            Anchored::Pattern(1),
            None,
            &mut Session::new(&mut Account),
        )
        .unwrap()
        .unwrap();
    let id = cache
        .next(&dfa, id, Some(b'b'), &mut Session::new(&mut Account))
        .unwrap()
        .unwrap();
    let id = cache
        .next(&dfa, id, None, &mut Session::new(&mut Account))
        .unwrap()
        .unwrap();
    assert_eq!(
        cache
            .match_pattern(&dfa, id, 0, &mut Session::new(&mut Account))
            .unwrap(),
        1
    );
    let id = cache
        .start(
            &dfa,
            Anchored::No,
            Some(b'\n'),
            &mut Session::new(&mut Account),
        )
        .unwrap()
        .unwrap();
    let id = cache
        .next(&dfa, id, Some(b'a'), &mut Session::new(&mut Account))
        .unwrap()
        .unwrap();
    assert_ne!(id & DEAD, 0); // glob ^ is a text anchor, not a line anchor.
}

#[test]
fn lazy_eoi_delays_matches_and_repeated_queries_reuse_all_transition_storage() {
    let dfa = machine(&["a", "a"], info::Kind::All, false);
    let mut cache = Cache::new(&dfa, &mut Session::new(&mut Account)).unwrap();
    let start = cache
        .start(&dfa, Anchored::No, None, &mut Session::new(&mut Account))
        .unwrap()
        .unwrap();
    let after = cache
        .next(&dfa, start, Some(b'a'), &mut Session::new(&mut Account))
        .unwrap()
        .unwrap();
    assert_eq!(after & MATCH, 0);
    let end = cache
        .next(&dfa, after, None, &mut Session::new(&mut Account))
        .unwrap()
        .unwrap();
    assert_ne!(end & MATCH, 0);
    assert_eq!(
        cache
            .match_len(&dfa, end, &mut Session::new(&mut Account))
            .unwrap(),
        2
    );
    assert_eq!(
        cache
            .match_pattern(&dfa, end, 1, &mut Session::new(&mut Account))
            .unwrap(),
        1
    );
    let states = cache.states.values.len();
    let mut count = Count::default();
    for _ in 0..3 {
        assert_eq!(
            cache
                .is_match(&dfa, b"a", &mut Session::new(&mut count))
                .unwrap(),
            Ok(true)
        );
    }
    assert!(count
        .requests
        .iter()
        .all(|r| matches!(r, Request::Items(_))));
    assert_eq!(cache.states.values.len(), states);
    assert_eq!(cache.bytes_searched, 3);
    assert!(cache.progress.is_none());
}

#[test]
fn ordered_subset_matches_keep_priority_and_overlapping_continuation_progress() {
    for kind in [info::Kind::All, info::Kind::LeftmostFirst] {
        let dfa = prefix(&[b"a", b"a", b"a"], true, kind);
        let mut cache = Cache::new(&dfa, &mut Session::new(&mut Account)).unwrap();
        let found = cache
            .earliest(&dfa, b"abcd", &mut Session::new(&mut Account))
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!((found.pattern, found.offset), (2, 1));
        assert_eq!(cache.bytes_searched, 1);
        let mut state = Overlapping::default();
        let mut reported = Vec::new();
        loop {
            cache
                .find_overlapping(&dfa, b"a", &mut state, &mut Session::new(&mut Account))
                .unwrap()
                .unwrap();
            let Some(found) = &state.found else {
                break;
            };
            reported.push((found.pattern, found.offset));
            assert_eq!(cache.bytes_searched, 2);
            assert!(cache.progress.is_none());
        }
        assert_eq!(
            reported,
            if kind == info::Kind::All {
                vec![(2, 1), (1, 1), (0, 1)]
            } else {
                vec![(2, 1)]
            }
        );
    }
}

#[test]
fn undersized_sets_ignore_full_in_match_order_and_never_clear_existing_ids() {
    let dfa = prefix(&[b"a", b"a", b"a"], true, info::Kind::All);
    for capacity in [0, 1, 2, 3, 4] {
        let mut cache = Cache::new(&dfa, &mut Session::new(&mut Account)).unwrap();
        let mut set = pike::set::Set::new(capacity, &mut Session::new(&mut Account)).unwrap();
        cache
            .overlapping(&dfa, b"a", &mut set, &mut Session::new(&mut Account))
            .unwrap()
            .unwrap();
        assert_eq!(ids(&set), (0..capacity.min(3) as u32).collect::<Vec<_>>());
        assert_eq!(cache.bytes_searched, 1);
    }
    let re = hybrid(&["a", "b", "?"], info::Kind::All);
    let mut cache = re.create_cache(&mut Session::new(&mut Account)).unwrap();
    let mut set = pike::set::Set::new(3, &mut Session::new(&mut Account)).unwrap();
    for input in [b"a", b"b"] {
        re.overlapping(&mut cache, input, &mut set, &mut Session::new(&mut Account))
            .unwrap()
            .unwrap();
    }
    assert_eq!(ids(&set), [0, 1, 2]);
}

#[test]
fn every_cold_search_refusal_is_terminal_and_retains_partial_pattern_ids() {
    let re = hybrid(&["a", "a", "a"], info::Kind::All);
    for overlapping in [false, true] {
        let mut cache = re.create_cache(&mut Session::new(&mut Account)).unwrap();
        let mut set = pike::set::Set::new(4, &mut Session::new(&mut Account)).unwrap();
        let mut baseline = Count::default();
        if overlapping {
            re.overlapping(&mut cache, b"a", &mut set, &mut Session::new(&mut baseline))
                .unwrap()
                .unwrap();
        } else {
            assert_eq!(
                re.is_match(&mut cache, b"a", &mut Session::new(&mut baseline))
                    .unwrap(),
                Ok(true)
            );
        }
        assert!(baseline
            .requests
            .iter()
            .any(|r| matches!(r, Request::Allocation { .. })));
        let mut partial = false;
        for refusal in 0..baseline.calls {
            let mut cache = re.create_cache(&mut Session::new(&mut Account)).unwrap();
            let mut set = pike::set::Set::new(4, &mut Session::new(&mut Account)).unwrap();
            let mut count = Count {
                refuse: Some(refusal),
                ..Count::default()
            };
            let mut session = Session::new(&mut count);
            let result = if overlapping {
                re.overlapping(&mut cache, b"a", &mut set, &mut session)
            } else {
                re.is_match(&mut cache, b"a", &mut session)
                    .map(|result| result.map(|_| ()))
            };
            assert!(matches!(result, Err(Stop::Refused)));
            partial |= !set.is_empty();
            assert!(matches!(
                re.is_match(&mut cache, b"a", &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(
                re.overlapping(&mut cache, b"a", &mut set, &mut session),
                Err(Stop::Refused)
            ));
            assert!(
                matches!(session.finish(Ok(())), Err(Error::Admission(cause)) if cause == refusal)
            );
            assert_eq!(count.calls, refusal + 1);
            assert_eq!(count.requests, baseline.requests[..refusal]);
            let states: Vec<_> = cache
                .0
                .as_ref()
                .unwrap()
                .forward
                .states
                .values
                .iter()
                .map(|s| s.weak())
                .collect();
            drop(cache);
            assert!(states.iter().all(|s| s.upgrade().is_none()));
        }
        assert_eq!(partial, overlapping);
    }
}

#[test]
fn absent_engine_query_is_an_internal_error_not_a_false_match_or_semantic_retry() {
    let re = Hybrid::none();
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut cache = re.create_cache(&mut session).unwrap();
    let result = re.is_match(&mut cache, b"a", &mut session);
    assert!(matches!(session.finish(result), Err(Error::State)));
    let re = hybrid(&["a"], info::Kind::LeftmostFirst);
    assert!(matches!(re.0, Attempt::Built(_)));
}
