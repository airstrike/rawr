use super::super::{info, machine, Attempt, Hybrid};
use super::*;
use crate::input::membership::manifest::{reference::Account, Stop};
use rawr_toml::document::Request;

#[derive(Default)]
struct Count {
    requests: Vec<Request>,
    refuse: Option<usize>,
    calls: usize,
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

fn engine(glob: &str) -> Machine {
    let (_, nfa, _) = machine::tests::prepared(&[glob], info::Kind::LeftmostFirst);
    Machine::build(
        &nfa,
        info::Kind::LeftmostFirst,
        10 * (1 << 20),
        &mut Session::new(&mut Account),
    )
    .unwrap()
    .unwrap()
}

fn builder(id: u32, matching: bool) -> key::Nfa {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut matches = key::Empty::new().matches(&mut session).unwrap();
    if matching {
        matches.pattern(0, &mut session).unwrap();
    }
    let mut builder = matches.nfa(&mut session).unwrap();
    builder.id(id, &mut session).unwrap();
    builder
}

fn sentinel_assertions(cache: &Cache, dfa: &Machine) {
    let stride = dfa.stride();
    assert_eq!(cache.trans.values.len(), 3 * stride);
    assert_eq!(cache.states.values.len(), 3);
    assert_eq!(cache.memory_states, 27);
    assert_eq!(cache.interner.len(), 1);
    assert_eq!(
        cache.starts.values,
        vec![UNKNOWN; 12 + 6 * dfa.pattern_len()]
    );
    let dead = key::State::dead(&mut Session::new(&mut Account)).unwrap();
    assert_eq!(
        cache
            .interner
            .get(dead.bytes(), &mut Session::new(&mut Account))
            .unwrap(),
        Some(DEAD | stride as u32)
    );
    assert!(cache
        .states
        .values
        .iter()
        .all(|state| state.bytes() == dead.bytes()));
    assert_eq!(cache.states.values[0].weak().strong_count(), 4);
    let alphabet = usize::from(dfa.classes()[255]) + 2;
    for (index, tag) in [UNKNOWN, DEAD, QUIT].into_iter().enumerate() {
        let row = &cache.trans.values[index * stride..(index + 1) * stride];
        assert!(row[..alphabet]
            .iter()
            .all(|&id| id == tag | (index * stride) as u32));
        assert!(row[alphabet..].iter().all(|&id| id == UNKNOWN));
    }
}

#[test]
fn initialization_has_shared_sentinels_canonical_dead_lookup_and_exact_semantic_memory() {
    for glob in ["", "a", "[a-z]*.rs"] {
        let dfa = engine(glob);
        let mut count = Count::default();
        let cache = Cache::new(&dfa, &mut Session::new(&mut count)).unwrap();
        sentinel_assertions(&cache, &dfa);
        let allocations: Vec<_> = count
            .requests
            .iter()
            .filter_map(|request| match request {
                Request::Allocation { bytes, alignment } => Some((*bytes, *alignment)),
                _ => None,
            })
            .collect();
        assert_eq!(
            &allocations[..4],
            &[(dfa.state_len().max(4) * 4, std::mem::align_of::<u32>()); 4]
        );
        let state_size = std::mem::size_of::<key::State>();
        let expected = (3 * dfa.stride() + 12 + 6 * dfa.pattern_len()) * 4
            + 3 * state_size
            + state_size
            + 4
            + 4 * dfa.state_len() * 4
            + 27;
        assert_eq!(
            cache.memory_usage(&mut Session::new(&mut Account)).unwrap(),
            expected
        );
        assert_eq!((cache.clear_count, cache.bytes_searched), (0, 0));
        assert!(cache.progress.is_none());
        assert!(!count
            .requests
            .iter()
            .any(|request| matches!(request, Request::String(_))));
    }
}

#[test]
fn builder_hit_reuses_id_and_restores_scratch_without_allocating_another_state() {
    let dfa = engine("abcdef");
    let mut cache = Cache::new(&dfa, &mut Session::new(&mut Account)).unwrap();
    let id = cache
        .insert_builder(
            &dfa,
            builder(2, true),
            Tag::Start,
            &mut Session::new(&mut Account),
        )
        .unwrap()
        .unwrap();
    assert_eq!(id, (3 * dfa.stride()) as u32 | START | MATCH);
    let mut count = Count::default();
    let duplicate = cache
        .insert_builder(
            &dfa,
            builder(2, true),
            Tag::None,
            &mut Session::new(&mut count),
        )
        .unwrap()
        .unwrap();
    assert_eq!(duplicate, id);
    assert_eq!(cache.states.values.len(), 4);
    assert_eq!(cache.interner.len(), 2);
    assert!(count
        .requests
        .iter()
        .all(|request| matches!(request, Request::Items(_))));
    assert!(cache.builder.capacity() >= 10);
    let scratch = cache.take_builder(&mut Session::new(&mut Account)).unwrap();
    assert!(scratch.capacity() >= 10);
    assert_eq!(cache.builder.capacity(), 0);
}

#[test]
fn saved_state_survives_clear_restores_tags_and_reset_drops_the_obligation() {
    let dfa = engine("abcdef");
    let mut cache = Cache::new(&dfa, &mut Session::new(&mut Account)).unwrap();
    cache
        .insert_builder(
            &dfa,
            builder(1, false),
            Tag::None,
            &mut Session::new(&mut Account),
        )
        .unwrap()
        .unwrap();
    let state = builder(2, true)
        .shared(&mut Session::new(&mut Account))
        .unwrap();
    let weak = state.weak();
    let id = cache
        .add_state(&dfa, state, Tag::Start, &mut Session::new(&mut Account))
        .unwrap()
        .unwrap();
    assert_eq!(weak.strong_count(), 2);
    cache
        .save(&dfa, id, &mut Session::new(&mut Account))
        .unwrap();
    assert_eq!(weak.strong_count(), 3);
    cache
        .try_clear(&dfa, &mut Session::new(&mut Account))
        .unwrap()
        .unwrap();
    let restored = cache.saved_id(&mut Session::new(&mut Account)).unwrap();
    assert_ne!(restored, id);
    assert_eq!(restored, (3 * dfa.stride()) as u32 | START | MATCH);
    assert_eq!(weak.strong_count(), 2);
    cache
        .save(&dfa, restored, &mut Session::new(&mut Account))
        .unwrap();
    assert_eq!(
        cache.saved_id(&mut Session::new(&mut Account)).unwrap(),
        restored
    );
    assert_eq!(weak.strong_count(), 2);
    cache
        .save(&dfa, restored, &mut Session::new(&mut Account))
        .unwrap();
    cache.reset(&dfa, &mut Session::new(&mut Account)).unwrap();
    assert!(weak.upgrade().is_none());
    assert_eq!(cache.clear_count, 0);
    sentinel_assertions(&cache, &dfa);
}

#[test]
fn efficiency_thresholds_and_reverse_progress_are_semantic_and_reset_in_source_order() {
    let dfa = engine("a");
    let mut cache = Cache::new(&dfa, &mut Session::new(&mut Account)).unwrap();
    for _ in 0..3 {
        cache
            .try_clear(&dfa, &mut Session::new(&mut Account))
            .unwrap()
            .unwrap();
    }
    let mut count = Count::default();
    let mut session = Session::new(&mut count);
    assert_eq!(
        cache.try_clear(&dfa, &mut session).unwrap(),
        Err(Failure::BadEfficiency)
    );
    assert!(session.check().is_ok());
    cache.search_start(100, &mut session).unwrap();
    cache.search_update(71, &mut session).unwrap();
    assert_eq!(
        cache.try_clear(&dfa, &mut session).unwrap(),
        Err(Failure::BadEfficiency)
    );
    cache.search_update(70, &mut session).unwrap();
    cache.try_clear(&dfa, &mut session).unwrap().unwrap();
    assert_eq!(cache.clear_count, 4);
    assert_eq!(cache.total_searched(&mut session).unwrap(), 0);
    cache.search_start(50, &mut session).unwrap();
    cache.search_update(40, &mut session).unwrap();
    cache.search_start(30, &mut session).unwrap();
    cache.search_finish(10, &mut session).unwrap();
    assert_eq!(cache.total_searched(&mut session).unwrap(), 30);
    cache.reset(&dfa, &mut session).unwrap();
    assert_eq!((cache.clear_count, cache.bytes_searched), (0, 0));
    assert!(cache.progress.is_none());
}

#[test]
fn retained_clear_and_reset_reuse_storage_but_still_admit_each_new_dead_state() {
    let dfa = engine("abcdefghijklmnop");
    let mut cache = Cache::new(&dfa, &mut Session::new(&mut Account)).unwrap();
    for id in 0..10 {
        cache
            .insert_builder(
                &dfa,
                builder(id, false),
                Tag::None,
                &mut Session::new(&mut Account),
            )
            .unwrap()
            .unwrap();
    }
    let trans_capacity = cache.trans.admitted;
    let starts_capacity = cache.starts.admitted;
    let mut count = Count::default();
    cache.reset(&dfa, &mut Session::new(&mut count)).unwrap();
    assert_eq!(cache.trans.admitted, trans_capacity);
    assert_eq!(cache.starts.admitted, starts_capacity);
    assert_eq!(
        count
            .requests
            .iter()
            .filter(|r| matches!(r, Request::Allocation { .. }))
            .count(),
        2
    );
    let smaller = engine("a");
    cache
        .reset(&smaller, &mut Session::new(&mut Account))
        .unwrap();
    assert_eq!(cache.sparses.memory_usage(), 4 * smaller.state_len() * 4);
    sentinel_assertions(&cache, &smaller);
}

#[test]
fn paired_cache_creation_is_forward_then_reverse_and_absent_engine_allocates_nothing() {
    let (info, nfa, reverse) = machine::tests::prepared(&["a"], info::Kind::LeftmostFirst);
    let hybrid = Hybrid::new(&info, &nfa, &reverse, &mut Session::new(&mut Account)).unwrap();
    let Attempt::Built(pair) = &hybrid.0 else {
        panic!("expected engine");
    };
    let mut forward = Count::default();
    Cache::new(&pair.forward, &mut Session::new(&mut forward)).unwrap();
    let mut backward = Count::default();
    Cache::new(&pair.reverse, &mut Session::new(&mut backward)).unwrap();
    let mut count = Count::default();
    hybrid.create_cache(&mut Session::new(&mut count)).unwrap();
    let mut expected = vec![Request::Items(1)];
    expected.extend(forward.requests);
    expected.extend(backward.requests);
    assert_eq!(count.requests, expected);
    let mut absent_count = Count::default();
    let absent = Hybrid::none()
        .create_cache(&mut Session::new(&mut absent_count))
        .unwrap();
    assert!(absent.0.is_none());
    assert_eq!(absent_count.requests, [Request::Items(1)]);
}

#[test]
fn every_initialization_refusal_is_terminal_and_does_not_retry() {
    let dfa = engine("a");
    let mut count = Count::default();
    Cache::new(&dfa, &mut Session::new(&mut count)).unwrap();
    for refusal in 0..count.calls {
        let mut count = Count {
            refuse: Some(refusal),
            ..Count::default()
        };
        let mut session = Session::new(&mut count);
        assert!(matches!(Cache::new(&dfa, &mut session), Err(Stop::Refused)));
        assert!(matches!(Cache::new(&dfa, &mut session), Err(Stop::Refused)));
        assert_eq!(count.calls, refusal + 1);
    }
}

#[test]
fn clear_saved_restore_and_reset_refusals_are_terminal_with_flat_teardown() {
    fn setup(dfa: &Machine) -> (Cache, std::sync::Weak<[u8]>) {
        let mut cache = Cache::new(dfa, &mut Session::new(&mut Account)).unwrap();
        let state = builder(1, false)
            .shared(&mut Session::new(&mut Account))
            .unwrap();
        let weak = state.weak();
        let id = cache
            .add_state(dfa, state, Tag::None, &mut Session::new(&mut Account))
            .unwrap()
            .unwrap();
        cache
            .save(dfa, id, &mut Session::new(&mut Account))
            .unwrap();
        (cache, weak)
    }
    let dfa = engine("abcdef");
    for reset in [false, true] {
        let (mut cache, _) = setup(&dfa);
        let mut count = Count::default();
        if reset {
            cache.reset(&dfa, &mut Session::new(&mut count)).unwrap();
        } else {
            cache
                .try_clear(&dfa, &mut Session::new(&mut count))
                .unwrap()
                .unwrap();
        }
        let calls = count.calls;
        for refusal in 0..calls {
            let (mut cache, weak) = setup(&dfa);
            let mut count = Count {
                refuse: Some(refusal),
                ..Count::default()
            };
            let mut session = Session::new(&mut count);
            let result = if reset {
                cache.reset(&dfa, &mut session)
            } else {
                cache
                    .try_clear(&dfa, &mut session)
                    .map(|result| result.unwrap())
            };
            assert!(matches!(result, Err(Stop::Refused)));
            assert!(matches!(
                cache.try_clear(&dfa, &mut session),
                Err(Stop::Refused)
            ));
            assert_eq!(count.calls, refusal + 1);
            drop(cache);
            assert!(weak.upgrade().is_none());
        }
    }
}

#[test]
fn semantic_fit_drives_retained_epochs_then_efficiency_failure_without_session_refusal() {
    let (_, nfa, _) = machine::tests::prepared(&["abcdefghijklmnop"], info::Kind::LeftmostFirst);
    let minimum = match Machine::build(
        &nfa,
        info::Kind::LeftmostFirst,
        0,
        &mut Session::new(&mut Account),
    )
    .unwrap()
    {
        Err(machine::Failure::CacheCapacity { minimum, .. }) => minimum,
        _ => panic!("zero capacity must expose the source floor"),
    };
    let dfa = Machine::build(
        &nfa,
        info::Kind::LeftmostFirst,
        minimum,
        &mut Session::new(&mut Account),
    )
    .unwrap()
    .unwrap();
    let mut cache = Cache::new(&dfa, &mut Session::new(&mut Account)).unwrap();
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut failed = false;
    for id in 0..nfa.0.states.len() as u32 {
        match cache
            .insert_builder(&dfa, builder(id, false), Tag::None, &mut session)
            .unwrap()
        {
            Ok(_) => {}
            Err(Failure::BadEfficiency) => {
                failed = true;
                break;
            }
            Err(other) => panic!("unexpected source failure: {other:?}"),
        }
    }
    assert!(failed);
    assert_eq!(cache.clear_count, 3);
    assert!(session.check().is_ok());
    assert!(cache.builder.capacity() >= 10);
    cache.reset(&dfa, &mut session).unwrap();
    assert_eq!(cache.clear_count, 0);
}

#[test]
fn complete_lifecycle_preserves_original_cumulative_floors_after_clear_and_reset() {
    struct Budget {
        items: usize,
        bytes: usize,
        item_limit: usize,
        byte_limit: usize,
    }
    impl Admission for Budget {
        type Error = ();
        fn admit(&mut self, request: Request) -> Result<(), ()> {
            match request {
                Request::Items(amount) => {
                    if self.items + amount > self.item_limit {
                        return Err(());
                    }
                    self.items += amount;
                }
                Request::Allocation { bytes, .. } => {
                    if self.bytes + bytes > self.byte_limit {
                        return Err(());
                    }
                    self.bytes += bytes;
                }
                _ => {}
            }
            Ok(())
        }
    }
    fn operation<A: Admission>(dfa: &Machine, session: &mut Session<'_, A>) -> Step<()> {
        let mut prior = Array::new();
        prior.resize(17, 9_usize, session)?;
        session.items(29)?;
        let mut cache = Cache::new(dfa, session)?;
        let mut builder = cache
            .take_builder(session)?
            .matches(session)?
            .nfa(session)?;
        builder.id(1, session)?;
        let id = cache
            .insert_builder(dfa, builder, Tag::None, session)?
            .unwrap();
        cache.save(dfa, id, session)?;
        cache.try_clear(dfa, session)?.unwrap();
        cache.reset(dfa, session)?;
        drop(prior);
        Ok(())
    }
    let dfa = engine("abcdef");
    let mut count = Count::default();
    operation(&dfa, &mut Session::new(&mut count)).unwrap();
    let items: usize = count
        .requests
        .iter()
        .map(|r| if let Request::Items(n) = r { *n } else { 0 })
        .sum();
    let bytes: usize = count
        .requests
        .iter()
        .map(|r| {
            if let Request::Allocation { bytes, .. } = r {
                *bytes
            } else {
                0
            }
        })
        .sum();
    for (item_limit, byte_limit, success) in [
        (items, bytes, true),
        (items - 1, bytes, false),
        (items, bytes - 1, false),
    ] {
        let mut budget = Budget {
            items: 0,
            bytes: 0,
            item_limit,
            byte_limit,
        };
        let mut session = Session::new(&mut budget);
        assert_eq!(operation(&dfa, &mut session).is_ok(), success);
        if !success {
            assert!(matches!(operation(&dfa, &mut session), Err(Stop::Refused)));
        }
        if success {
            assert_eq!((budget.items, budget.bytes), (items, bytes));
        }
    }
}

#[test]
fn fit_triggered_clear_restores_saved_state_before_incoming_builder_publication() {
    fn setup(dfa: &Machine) -> (Cache, key::Nfa, std::sync::Weak<[u8]>, u32, usize) {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut cache = Cache::new(dfa, &mut session).unwrap();
        cache
            .insert_builder(dfa, builder(0, false), Tag::None, &mut session)
            .unwrap()
            .unwrap();
        let state = builder(1, true).shared(&mut session).unwrap();
        let weak = state.weak();
        let saved = cache
            .add_state(dfa, state, Tag::Start, &mut session)
            .unwrap()
            .unwrap();
        assert_eq!(saved, (4 * dfa.stride()) as u32 | START | MATCH);
        for id in 2..dfa.state_len() as u32 {
            let scratch = cache.take_builder(&mut session).unwrap();
            let capacity = scratch.capacity();
            let mut incoming = scratch
                .matches(&mut session)
                .unwrap()
                .nfa(&mut session)
                .unwrap();
            incoming.id(id, &mut session).unwrap();
            if !cache
                .fits(dfa, incoming.bytes().len(), &mut session)
                .unwrap()
            {
                assert_eq!(cache.clear_count, 0);
                assert_eq!(cache.builder.capacity(), 0);
                assert!(capacity >= incoming.bytes().len());
                cache.save(dfa, saved, &mut session).unwrap();
                assert_eq!(weak.strong_count(), 3);
                return (cache, incoming, weak, saved, capacity);
            }
            cache
                .insert_builder(dfa, incoming, Tag::None, &mut session)
                .unwrap()
                .unwrap();
        }
        panic!("the source minimum-capacity fixture must reach an automatic clear");
    }

    let (_, nfa, _) = machine::tests::prepared(&["abcdefghijklmnop"], info::Kind::LeftmostFirst);
    let minimum = match Machine::build(
        &nfa,
        info::Kind::LeftmostFirst,
        0,
        &mut Session::new(&mut Account),
    )
    .unwrap()
    {
        Err(machine::Failure::CacheCapacity { minimum, .. }) => minimum,
        _ => panic!("zero capacity must expose the source floor"),
    };
    let dfa = Machine::build(
        &nfa,
        info::Kind::LeftmostFirst,
        minimum,
        &mut Session::new(&mut Account),
    )
    .unwrap()
    .unwrap();
    let (mut cache, incoming, saved, old_id, scratch_capacity) = setup(&dfa);
    let incoming_bytes = incoming.bytes().to_vec();
    let mut count = Count::default();
    let incoming_id = cache
        .insert_builder(&dfa, incoming, Tag::None, &mut Session::new(&mut count))
        .unwrap()
        .unwrap();
    let restored = cache.saved_id(&mut Session::new(&mut Account)).unwrap();
    assert_ne!(restored, old_id);
    assert_eq!(restored, (3 * dfa.stride()) as u32 | START | MATCH);
    assert_eq!(incoming_id, (4 * dfa.stride()) as u32);
    assert_eq!(cache.clear_count, 1);
    assert_eq!(cache.states.values.len(), 5);
    assert_eq!(cache.interner.len(), 3);
    assert_eq!(cache.trans.values.len(), 5 * dfa.stride());
    assert!(cache.states.values[3].weak().ptr_eq(&saved));
    assert_eq!(cache.states.values[4].bytes(), incoming_bytes);
    assert_eq!(
        cache
            .interner
            .get(&incoming_bytes, &mut Session::new(&mut Account))
            .unwrap(),
        Some(incoming_id)
    );
    assert_eq!(cache.builder.capacity(), scratch_capacity);
    assert_eq!(saved.strong_count(), 2);
    let published = cache.states.values[4].weak();
    drop(cache);
    assert!(saved.upgrade().is_none());
    assert!(published.upgrade().is_none());

    // Refuse every step of the joined path, including incoming Arc creation,
    // clear, saved-state restoration, publication and scratch restoration.
    for refusal in 0..count.calls {
        let (mut cache, incoming, saved, _, _) = setup(&dfa);
        let mut refused = Count {
            refuse: Some(refusal),
            ..Count::default()
        };
        let mut session = Session::new(&mut refused);
        assert!(matches!(
            cache.insert_builder(&dfa, incoming, Tag::None, &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(
            cache.try_clear(&dfa, &mut session),
            Err(Stop::Refused)
        ));
        assert!(
            matches!(session.finish::<()>(Err(Stop::Refused)), Err(Error::Admission(cause)) if cause == refusal)
        );
        assert_eq!(refused.calls, refusal + 1);
        assert_eq!(refused.requests, count.requests[..refusal]);
        let published: Vec<_> = cache.states.values.iter().map(key::State::weak).collect();
        drop(cache);
        assert!(saved.upgrade().is_none());
        assert!(published.iter().all(|state| state.upgrade().is_none()));
    }
}

#[test]
fn growing_reset_replaces_all_four_sparse_arrays_after_reinitialization() {
    fn setup(dfa: &Machine) -> (Cache, std::sync::Weak<[u8]>) {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut cache = Cache::new(dfa, &mut session).unwrap();
        let state = builder(1, false).shared(&mut session).unwrap();
        let weak = state.weak();
        let id = cache
            .add_state(dfa, state, Tag::None, &mut session)
            .unwrap()
            .unwrap();
        cache.save(dfa, id, &mut session).unwrap();
        cache.search_start(0, &mut session).unwrap();
        cache.search_update(7, &mut session).unwrap();
        (cache, weak)
    }

    let small = engine("a");
    let large = engine("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789");
    assert!(large.state_len() > small.state_len().max(4) * 2);
    let (mut cache, saved) = setup(&small);
    let mut count = Count::default();
    cache.reset(&large, &mut Session::new(&mut count)).unwrap();
    assert!(saved.upgrade().is_none());
    sentinel_assertions(&cache, &large);
    assert_eq!(cache.sparses.memory_usage(), 4 * large.state_len() * 4);
    assert_eq!((cache.clear_count, cache.bytes_searched), (0, 0));
    assert!(cache.progress.is_none());
    assert!(matches!(cache.saver, Saver::None));
    let allocations: Vec<_> = count
        .requests
        .iter()
        .enumerate()
        .filter_map(|(index, request)| {
            matches!(request, Request::Allocation { .. }).then_some(index)
        })
        .collect();
    // Sparse resizing is the final allocating operation, dense then sparse
    // for the first set, followed by dense then sparse for the second set.
    let replacements = &allocations[allocations.len() - 4..];
    for &index in replacements {
        assert_eq!(
            count.requests[index],
            Request::Allocation {
                bytes: large.state_len() * 4,
                alignment: std::mem::align_of::<u32>(),
            }
        );
    }
    drop(cache);

    for (completed, &refusal) in replacements.iter().enumerate() {
        let (mut cache, saved) = setup(&small);
        let mut refused = Count {
            refuse: Some(refusal),
            ..Count::default()
        };
        let mut session = Session::new(&mut refused);
        assert!(matches!(
            cache.reset(&large, &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(
            cache.reset(&large, &mut session),
            Err(Stop::Refused)
        ));
        assert!(
            matches!(session.finish::<()>(Err(Stop::Refused)), Err(Error::Admission(cause)) if cause == refusal)
        );
        assert_eq!(refused.calls, refusal + 1);
        assert_eq!(refused.requests, count.requests[..refusal]);
        assert!(saved.upgrade().is_none());
        assert!(matches!(cache.saver, Saver::None));
        assert_eq!(cache.clear_count, 1);
        assert_eq!(cache.states.values.len(), 3);
        assert_eq!(cache.starts.values.len(), 12 + 6 * large.pattern_len());
        assert_eq!(
            cache.sparses.memory_usage(),
            (completed * large.state_len() + (4 - completed) * small.state_len()) * 4
        );
        let published: Vec<_> = cache.states.values.iter().map(key::State::weak).collect();
        drop(cache);
        assert!(published.iter().all(|state| state.upgrade().is_none()));
    }
}
