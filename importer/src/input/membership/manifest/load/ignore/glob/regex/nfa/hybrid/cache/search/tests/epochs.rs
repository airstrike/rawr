use super::super::super::{key, Saver, Tag};
use super::*;

const INPUT: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

fn before(dfa: &Machine, stop: usize) -> (Cache, u32) {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut cache = Cache::new(dfa, &mut session).unwrap();
    let mut id = cache
        .start(dfa, Anchored::No, None, &mut session)
        .unwrap()
        .unwrap();
    cache.search_start(0, &mut session).unwrap();
    for (at, &byte) in INPUT[..stop].iter().enumerate() {
        cache.search_update(at, &mut session).unwrap();
        id = cache
            .next(dfa, id, Some(byte), &mut session)
            .unwrap()
            .unwrap();
    }
    cache.search_update(stop, &mut session).unwrap();
    (cache, id)
}

fn first_clear(dfa: &Machine) -> usize {
    let (mut cache, mut id) = before(dfa, 0);
    let mut account = Account;
    let mut session = Session::new(&mut account);
    for (at, &byte) in INPUT.iter().enumerate() {
        cache.search_update(at, &mut session).unwrap();
        id = cache
            .next(dfa, id, Some(byte), &mut session)
            .unwrap()
            .unwrap();
        if cache.clear_count != 0 {
            return at;
        }
    }
    panic!("minimum-capacity literal must exercise automatic clear");
}

#[test]
fn an_actual_lazy_transition_remaps_the_saved_row_and_caches_its_edge_after_clear() {
    let dfa = machine(
        &[std::str::from_utf8(INPUT).unwrap()],
        info::Kind::LeftmostFirst,
        true,
    );
    let at = first_clear(&dfa);
    let (mut cache, current) = before(&dfa, at);
    let state = &cache.states.values[(current & MAX_ID) as usize >> dfa.stride2()];
    let saved = state.weak();
    let key = state.bytes().to_vec();
    let mut count = Count::default();
    let next = cache
        .next(
            &dfa,
            current,
            Some(INPUT[at]),
            &mut Session::new(&mut count),
        )
        .unwrap()
        .unwrap();
    assert_eq!(cache.clear_count, 1);
    assert!(matches!(cache.saver, Saver::None));
    let restored = cache
        .interner
        .get(&key, &mut Session::new(&mut Account))
        .unwrap()
        .unwrap();
    assert_ne!(restored, current);
    assert!(
        cache.states.values[(restored & MAX_ID) as usize >> dfa.stride2()]
            .weak()
            .ptr_eq(&saved)
    );
    assert_eq!(
        cache
            .cached(
                &dfa,
                restored,
                Some(INPUT[at]),
                &mut Session::new(&mut Account)
            )
            .unwrap(),
        next
    );
    let mut warm = Count::default();
    assert_eq!(
        cache
            .next(
                &dfa,
                restored,
                Some(INPUT[at]),
                &mut Session::new(&mut warm)
            )
            .unwrap(),
        Ok(next)
    );
    assert_eq!(warm.requests, [Request::Items(1)]);
    assert_eq!(
        cache.progress.as_ref().map(|p| (p.start, p.at)),
        Some((at, at))
    );
    assert_eq!(cache.bytes_searched, 0);
    drop(cache);
    assert!(saved.upgrade().is_none());

    for refusal in 0..count.calls {
        let (mut cache, current) = before(&dfa, at);
        let saved = cache.states.values[(current & MAX_ID) as usize >> dfa.stride2()].weak();
        let mut refused = Count {
            refuse: Some(refusal),
            ..Count::default()
        };
        let mut session = Session::new(&mut refused);
        assert!(matches!(
            cache.next(&dfa, current, Some(INPUT[at]), &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(
            cache.is_match(&dfa, INPUT, &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(session.finish(Ok(())), Err(Error::Admission(cause)) if cause == refusal));
        assert_eq!(refused.requests, count.requests[..refusal]);
        assert_eq!(refused.calls, refusal + 1);
        let published: Vec<_> = cache.states.values.iter().map(|s| s.weak()).collect();
        drop(cache);
        assert!(saved.upgrade().is_none());
        assert!(published.iter().all(|s| s.upgrade().is_none()));
    }
}

#[test]
fn source_efficiency_gave_up_keeps_exact_byte_offset_and_original_session_usable() {
    let dfa = machine(
        &[std::str::from_utf8(INPUT).unwrap()],
        info::Kind::LeftmostFirst,
        true,
    );
    let (mut manual, mut id) = before(&dfa, 0);
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let expected = INPUT
        .iter()
        .enumerate()
        .find_map(|(at, &byte)| {
            manual.search_update(at, &mut session).unwrap();
            match manual.next(&dfa, id, Some(byte), &mut session).unwrap() {
                Ok(next) => {
                    id = next;
                    None
                }
                Err(_) => Some(at),
            }
        })
        .expect("the literal must reach the fourth inefficient epoch");
    assert_eq!(manual.clear_count, 3);
    assert!(expected > 0 && expected < INPUT.len());
    for overlap in [false, true] {
        let mut cache = Cache::new(&dfa, &mut session).unwrap();
        let mut set = pike::set::Set::new(1, &mut session).unwrap();
        let result = if overlap {
            cache.overlapping(&dfa, INPUT, &mut set, &mut session)
        } else {
            cache
                .is_match(&dfa, INPUT, &mut session)
                .map(|r| r.map(|_| ()))
        }
        .unwrap();
        assert_eq!(result, Err(Failure::GaveUp { offset: expected }));
        assert_eq!(cache.clear_count, 3);
        assert_eq!(cache.progress.as_ref().map(|p| p.at), Some(expected));
        assert!(matches!(cache.saver, Saver::ToSave { .. }));
        assert!(set.is_empty());
        session.check().unwrap();
    }
}

// Fill remaining room with encoded subsets, retaining every ordinary cache
// extent. All keys have one valid NFA ID; no fake memory counter is installed.
fn crowded(dfa: &Machine, input: &[u8]) -> (Cache, u32) {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut cache = Cache::new(dfa, &mut session).unwrap();
    let mut id = cache
        .start(dfa, Anchored::No, None, &mut session)
        .unwrap()
        .unwrap();
    for &byte in input {
        id = cache
            .next(dfa, id, Some(byte), &mut session)
            .unwrap()
            .unwrap();
    }
    let scratch = cache.take_builder(&mut session).unwrap();
    for candidate in 0..dfa.state_len() as u32 {
        let mut builder = key::Empty::new()
            .matches(&mut session)
            .unwrap()
            .nfa(&mut session)
            .unwrap();
        builder.id(candidate, &mut session).unwrap();
        if !cache
            .fits(dfa, builder.bytes().len(), &mut session)
            .unwrap()
        {
            assert!(
                !cache.fits(dfa, 9, &mut session).unwrap(),
                "fixture needs room below the smallest EOI state"
            );
            cache.builder = scratch;
            cache.clear_count = 3;
            cache.bytes_searched = 0;
            return (cache, id);
        }
        let state = builder.shared(&mut session).unwrap();
        cache
            .add_state(dfa, state, Tag::None, &mut session)
            .unwrap()
            .unwrap();
    }
    panic!("fixture must fill the source minimum capacity");
}

fn crowded_machine(kind: info::Kind) -> Machine {
    let (_, nfa, _) = machine::tests::prepared(&["abcdefghijklmnop"], kind);
    let minimum = build(&nfa, kind, true).capacity();
    // Retain the full literal path, then fill its remaining table room. This
    // config remains above the pinned source minimum in both directions.
    Machine::build(&nfa, kind, minimum * 3, &mut Session::new(&mut Account))
        .unwrap()
        .unwrap()
}

#[test]
fn eoi_gave_up_finishes_overlapping_progress_but_preserves_earliest_incomplete_progress() {
    let dfa = crowded_machine(info::Kind::All);
    let input = b"abcdefghijklmnop";
    for overlap in [false, true] {
        let (mut cache, _) = crowded(&dfa, input);
        let mut account = Account;
        let mut session = Session::new(&mut account);
        if overlap {
            let mut set = pike::set::Set::new(1, &mut session).unwrap();
            assert_eq!(
                cache
                    .overlapping(&dfa, input, &mut set, &mut session)
                    .unwrap(),
                Err(Failure::GaveUp {
                    offset: input.len()
                })
            );
            assert!(cache.progress.is_none());
            assert_eq!(cache.bytes_searched, input.len());
            assert!(set.is_empty());
        } else {
            assert_eq!(
                cache.is_match(&dfa, input, &mut session).unwrap(),
                Err(Failure::GaveUp {
                    offset: input.len()
                })
            );
            // All byte edges are warm/untagged, so the earliest loop never
            // updates progress before EOI and returns before search_finish.
            assert_eq!(
                cache.progress.as_ref().map(|p| (p.start, p.at)),
                Some((0, 0))
            );
            assert_eq!(cache.bytes_searched, 0);
        }
        assert_eq!(cache.clear_count, 3);
        session.check().unwrap();
    }
}

#[test]
fn predicted_clear_with_existing_dead_key_releases_saver_without_clearing() {
    let dfa = crowded_machine(info::Kind::LeftmostFirst);
    let (mut cache, id) = crowded(&dfa, b"a");
    let count = cache.states.values.len();
    let clears = cache.clear_count;
    let next = cache
        .next(&dfa, id, Some(b'!'), &mut Session::new(&mut Account))
        .unwrap()
        .unwrap();
    assert_ne!(next & DEAD, 0);
    assert_eq!(cache.clear_count, clears);
    assert_eq!(cache.states.values.len(), count);
    assert!(matches!(cache.saver, Saver::None));
    assert_eq!(
        cache
            .cached(&dfa, id, Some(b'!'), &mut Session::new(&mut Account))
            .unwrap(),
        next
    );
}

#[test]
fn warmed_untagged_runs_preserve_progress_and_allocate_no_boolean_pattern_set() {
    let re = hybrid(&["*a"], info::Kind::LeftmostFirst);
    let input = b"bbbbbbbbbbbbbbbbba";
    let mut cache = re.create_cache(&mut Session::new(&mut Account)).unwrap();
    assert_eq!(
        re.is_match(&mut cache, input, &mut Session::new(&mut Account))
            .unwrap(),
        Ok(true)
    );
    let before = cache.0.as_ref().unwrap().forward.bytes_searched;
    let mut count = Count::default();
    assert_eq!(
        re.is_match(&mut cache, input, &mut Session::new(&mut count))
            .unwrap(),
        Ok(true)
    );
    assert!(count
        .requests
        .iter()
        .all(|r| matches!(r, Request::Items(_))));
    let forward = &cache.0.as_ref().unwrap().forward;
    assert_eq!(forward.bytes_searched, before + input.len());
    assert!(forward.progress.is_none());
}
