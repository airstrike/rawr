use super::super::super::super::builder;
use super::{cache, engine, Count, Kind, Pike, Session, State};
use crate::input::membership::manifest::{reference::Account, storage::Buffer, Error, Stop};
use std::{num::NonZeroUsize, sync::Arc};

fn matches(engine: &Pike, bytes: &[u8]) -> bool {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut cache = engine.create_cache(&mut session).unwrap();
    engine.is_match(&mut cache, bytes, &mut session).unwrap()
}

// Low-level source-builder fixtures expose match timing that whole-path emitted
// globs intentionally hide behind End assertions. They add no regex input API.
fn raw(patterns: &[Option<u8>], kind: Kind, anchored: bool) -> Pike {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut builder = builder::Builder::new();
    let mut starts = Buffer::new();
    for &byte in patterns {
        builder.start_pattern(&mut session).unwrap().unwrap();
        let matched = builder.add_match(&mut session).unwrap().unwrap();
        let next = match byte {
            None => matched,
            Some(byte) => builder
                .add(
                    builder::State::ByteRange {
                        trans: builder::Transition {
                            start: byte,
                            end: byte,
                            next: matched,
                        },
                    },
                    &mut session,
                )
                .unwrap()
                .unwrap(),
        };
        let captured = builder.capture_start(next, &mut session).unwrap().unwrap();
        builder.finish_pattern(captured, &mut session).unwrap();
        starts.push(captured, &mut session).unwrap();
    }
    let start = builder
        .add(builder::State::Union { alternates: starts }, &mut session)
        .unwrap()
        .unwrap();
    let unanchored = if anchored {
        start
    } else {
        let prefix = builder
            .add(
                builder::State::Union {
                    alternates: Buffer::new(),
                },
                &mut session,
            )
            .unwrap()
            .unwrap();
        let any = builder
            .add(
                builder::State::ByteRange {
                    trans: builder::Transition {
                        start: 0,
                        end: 255,
                        next: prefix,
                    },
                },
                &mut session,
            )
            .unwrap()
            .unwrap();
        builder.patch(prefix, start, &mut session).unwrap().unwrap();
        builder.patch(prefix, any, &mut session).unwrap().unwrap();
        prefix
    };
    let nfa = builder
        .finalize(start, unanchored, false, &mut session)
        .unwrap()
        .ok()
        .unwrap();
    Pike { kind, nfa }
}

#[test]
fn boolean_queries_preserve_bytes_empty_input_and_delayed_eoi_matches() {
    for kind in [Kind::All, Kind::LeftmostFirst] {
        let re = engine(&["a", "{é,中}", "src/**/x.rs", ""], kind);
        for (bytes, expected) in [
            (&b""[..], true),
            (b"a", true),
            (b"aa", false),
            (b"za", false),
            ("é".as_bytes(), true),
            ("中".as_bytes(), true),
            (b"src/x.rs", true),
            (b"src/a/b/x.rs", true),
            (b"src/a/b/x.rs\n", false),
            (b"\xff", false),
            (b"/", false),
        ] {
            assert_eq!(matches(&re, bytes), expected, "{kind:?}: {bytes:?}");
        }
        let wildcard = engine(&["?"], kind);
        for byte in 0..=255 {
            assert_eq!(
                matches(&wildcard, &[byte]),
                byte != b'/',
                "{kind:?}: {byte}"
            );
        }
        assert!(!matches(&wildcard, b""));
        assert!(!matches(&wildcard, b"aa"));
        assert!(!matches(&engine(&[], kind), b""));
        assert!(!matches(&engine(&[], kind), b"anything"));
    }
}

#[test]
fn earliest_all_finishes_the_batch_and_leftmost_first_stops_at_its_first_match() {
    for (kind, expected) in [(Kind::All, 2), (Kind::LeftmostFirst, 0)] {
        let re = raw(&[None, Some(b'a'), None], kind, true);
        let mut account = Count::default();
        let mut cache = cache::Lazy::none();
        let matched = re
            .earliest(&mut cache, b"a", &mut Session::new(&mut account))
            .unwrap();
        assert_eq!(matched, Some(expected));
        let next = &cache.0.as_ref().unwrap().next;
        let next_matches: Vec<_> = next.dense.values[..next.length]
            .iter()
            .filter_map(|id| match re.nfa.0.states[id.index()] {
                State::Match { pattern } => Some(pattern),
                _ => None,
            })
            .collect();
        assert_eq!(
            next_matches,
            if kind == Kind::All { vec![1] } else { vec![] }
        );

        // Earliest stops before swapping/clearing the batch or visiting any
        // subsequent byte, even when All has populated the next active set.
        let mut longer = Count::default();
        let mut longer_cache = cache::Lazy::none();
        assert!(re
            .is_match(
                &mut longer_cache,
                b"a later bytes",
                &mut Session::new(&mut longer)
            )
            .unwrap());
        assert_eq!(longer.calls, account.calls);
        assert_eq!(longer.allocations, account.allocations);
    }
}

#[test]
fn anchored_boolean_inserts_initial_closure_once_and_unanchored_retries_positions() {
    let re = raw(&[Some(b'a')], Kind::All, true);
    let mut cache = cache::Lazy::none();
    assert!(re
        .is_match(&mut cache, b"aa", &mut Session::new(&mut Account))
        .unwrap());
    let cache = cache.0.as_ref().unwrap();
    assert_eq!(cache.next.length, 0);
    assert_eq!(cache.curr.length, 1);
    assert!(matches!(
        re.nfa.0.states[cache.curr.dense.values[0].index()],
        State::Match { pattern: 0 }
    ));
    assert!(!matches(&re, b"za"));
    let unanchored = raw(&[Some(b'a')], Kind::All, false);
    assert!(matches(&unanchored, b"za"));
    assert!(!matches(&unanchored, b"zz"));
    assert!(!matches(&unanchored, b""));
}

#[test]
fn boolean_setup_reuses_full_slot_tables_and_clears_stale_search_state() {
    let re = engine(&["a", "b", ""], Kind::All);
    let mut account = Count::default();
    let mut cache = re.create_cache(&mut Session::new(&mut account)).unwrap();
    assert!(re
        .is_match(&mut cache, b"a", &mut Session::new(&mut account))
        .unwrap());
    let allocated = account.allocations.len();
    let (table, slots);
    {
        let active = cache.0.as_mut().unwrap();
        table = active.curr.slots.table.values.as_ptr();
        slots = active.curr.slots.table.values.len();
        assert!(slots > 0);
        active.curr.slots.table.values[0] = NonZeroUsize::new(7);
        active
            .stack
            .push(
                cache::Frame::Restore {
                    slot: 0,
                    offset: NonZeroUsize::new(9),
                },
                &mut Session::new(&mut account),
            )
            .unwrap();
    }
    for (bytes, expected) in [(&b"b"[..], true), (b"!", false), (b"", true), (b"a", true)] {
        assert_eq!(
            re.is_match(&mut cache, bytes, &mut Session::new(&mut account))
                .unwrap(),
            expected
        );
        let active = cache.0.as_ref().unwrap();
        assert!(active.stack.values.is_empty());
        assert_eq!(
            (
                active.curr.slots.for_captures,
                active.next.slots.for_captures
            ),
            (0, 0)
        );
        let retained = [&active.curr, &active.next]
            .into_iter()
            .find(|state| state.slots.table.values.as_ptr() == table)
            .unwrap();
        assert_eq!(retained.slots.table.values.len(), slots);
        assert_eq!(retained.slots.table.values[0], NonZeroUsize::new(7));
        assert_eq!(account.allocations.len(), allocated);
    }
}

#[test]
fn cold_and_reused_boolean_refusals_remain_terminal_after_matches_and_before_return() {
    let re = raw(&[None; 8], Kind::All, true);
    for warm in [false, true] {
        let mut baseline = Count::default();
        let mut cache = cache::Lazy::none();
        if warm {
            assert!(re
                .is_match(&mut cache, b"", &mut Session::new(&mut baseline))
                .unwrap());
        }
        let before = baseline.calls;
        assert!(re
            .is_match(&mut cache, b"", &mut Session::new(&mut baseline))
            .unwrap());
        for refuse in before..baseline.calls {
            let mut account = Count {
                refuse: Some(refuse),
                ..Count::default()
            };
            let mut session = Session::new(&mut account);
            let mut cache = cache::Lazy::none();
            if warm {
                assert!(re.is_match(&mut cache, b"", &mut session).unwrap());
            }
            assert!(matches!(
                re.is_match(&mut cache, b"", &mut session),
                Err(Stop::Refused)
            ));
            let reached = cache.0.as_ref().map(|cache| {
                (
                    cache.stack.values.len(),
                    cache.curr.length,
                    cache.next.length,
                )
            });
            assert!(matches!(
                re.is_match(&mut cache, b"unvisited", &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(cache.reset(&re, &mut session), Err(Stop::Refused)));
            assert_eq!(
                cache.0.as_ref().map(|cache| (
                    cache.stack.values.len(),
                    cache.curr.length,
                    cache.next.length
                )),
                reached
            );
            assert!(
                matches!(session.finish(Ok(())), Err(Error::Admission(cause)) if cause == refuse)
            );
            assert_eq!(account.calls, refuse + 1);
        }
    }
}

#[test]
fn changed_utf8_or_reverse_producer_is_terminal_instead_of_silently_skipping_required_work() {
    for reverse in [false, true] {
        let mut re = raw(&[None], Kind::All, true);
        let graph = Arc::get_mut(&mut re.nfa.0).unwrap();
        graph.utf8 = !reverse;
        graph.reverse = reverse;
        let mut account = Count::default();
        let mut session = Session::new(&mut account);
        let mut cache = cache::Lazy::none();
        assert!(matches!(
            re.is_match(&mut cache, b"", &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(
            re.is_match(&mut cache, b"", &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(session.finish(Ok(())), Err(Error::State)));
    }
}
