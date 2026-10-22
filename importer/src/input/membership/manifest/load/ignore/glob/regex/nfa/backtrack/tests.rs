use super::*;
use crate::input::membership::manifest::{reference::Account, Stop};
use rawr_toml::document::Request;
use std::{alloc::Layout, num::NonZeroUsize};

#[derive(Default)]
pub(super) struct Count {
    pub(super) calls: usize,
    pub(super) refuse: Option<usize>,
    pub(super) allocations: Vec<(usize, usize)>,
    pub(super) items: usize,
}

impl Admission for Count {
    type Error = ();
    fn admit(&mut self, request: Request) -> Result<(), ()> {
        let call = self.calls;
        self.calls += 1;
        if self.refuse == Some(call) {
            return Err(());
        }
        match request {
            Request::Allocation { bytes, alignment } => {
                self.allocations.push((bytes, alignment));
            }
            Request::Items(items) => self.items += items,
            _ => {}
        }
        Ok(())
    }
}

fn prepared(globs: &[&str], kind: info::Kind) -> (info::Info, finalize::Nfa) {
    use super::super::super::translate;
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let expressions: Vec<_> = globs
        .iter()
        .map(|glob| {
            let regex = super::super::super::super::parse(glob, &mut session)
                .unwrap()
                .ok()
                .unwrap()
                .regex(&mut session)
                .unwrap();
            let ast = regex.tree(&mut session).unwrap().ok().unwrap();
            translate::tree(&regex, &ast, &mut session)
                .unwrap()
                .ok()
                .unwrap()
        })
        .collect();
    info::Info::with(&expressions, kind, &mut session, |info, session| {
        let mut compiler = Compiler::new(false, session)?;
        let nfa = compiler.build(&expressions, None, session)?.ok().unwrap();
        Ok((info, nfa))
    })
    .unwrap()
}

pub(super) fn wrapper(globs: &[&str], kind: info::Kind) -> Backtrack {
    let (info, nfa) = prepared(globs, kind);
    Backtrack::new(&info, &nfa, &mut Session::new(&mut Account)).unwrap()
}

fn blocks(engine: &Engine, span_len: usize) -> usize {
    (engine.nfa.0.states.len() * (span_len + 1)).div_ceil(cache::Visited::BLOCK_SIZE)
}

fn layout<T>(length: usize) -> (usize, usize) {
    let layout = Layout::array::<T>(length).unwrap();
    (layout.size(), layout.align())
}

fn seeded(engine: &Engine) -> cache::Lazy {
    let mut lazy = cache::Lazy::none();
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let cache = lazy.get(engine, &mut session).unwrap();
    cache.setup(engine, 17, &mut session).unwrap().unwrap();
    cache.visited.bitset.values.fill(usize::MAX);
    cache
        .stack
        .resize(
            2,
            cache::Frame::Step {
                sid: super::super::builder::Id(1),
                at: 8,
            },
            &mut session,
        )
        .unwrap();
    lazy
}

#[test]
fn all_skips_builder_ownership_but_leftmost_constructs_before_sharing() {
    let (info, nfa) = prepared(&["*.rs"], info::Kind::All);
    let mut count = Count::default();
    let wrapper = Backtrack::new(&info, &nfa, &mut Session::new(&mut count)).unwrap();
    assert!(wrapper.0.is_none());
    assert_eq!(count.items, 1);
    assert!(count.allocations.is_empty());
    assert_eq!(Arc::strong_count(&nfa.0), 1);

    let (info, nfa) = prepared(&["src/**/a?.rs"], info::Kind::LeftmostFirst);
    let mut compiler_count = Count::default();
    let compiler = Compiler::new(false, &mut Session::new(&mut compiler_count)).unwrap();
    drop(compiler);
    assert!(!compiler_count.allocations.is_empty());
    let mut count = Count::default();
    let wrapper = Backtrack::new(&info, &nfa, &mut Session::new(&mut count)).unwrap();
    assert_eq!(count.allocations, compiler_count.allocations);
    assert_eq!(Arc::strong_count(&nfa.0), 2);
    assert!(matches!(wrapper.0.as_ref().unwrap().config.pre, Some(None)));
    assert_eq!(wrapper.0.as_ref().unwrap().config.visited_capacity, None);
    assert_eq!(
        wrapper.0.as_ref().unwrap().visited_capacity(),
        256 * (1 << 10)
    );
    let weak_info = Arc::downgrade(&info.0);
    drop(info);
    assert!(weak_info.upgrade().is_none());
    drop(wrapper);
    assert_eq!(Arc::strong_count(&nfa.0), 1);
}

#[test]
fn every_constructor_refusal_unshares_nfa_and_prevents_fallback() {
    let (info, nfa) = prepared(&["*.rs"], info::Kind::LeftmostFirst);
    let mut count = Count::default();
    drop(Backtrack::new(&info, &nfa, &mut Session::new(&mut count)).unwrap());
    for refuse in 0..count.calls {
        let mut account = Count {
            refuse: Some(refuse),
            ..Count::default()
        };
        let mut session = Session::new(&mut account);
        assert!(matches!(
            Backtrack::new(&info, &nfa, &mut session),
            Err(Stop::Refused)
        ));
        assert_eq!(Arc::strong_count(&nfa.0), 1);
        assert!(matches!(session.check(), Err(Stop::Refused)));
        assert!(matches!(
            Backtrack::new(&info, &nfa, &mut session),
            Err(Stop::Refused)
        ));
        assert_eq!(account.calls, refuse + 1);
    }
}

#[test]
fn wrapper_eligibility_uses_total_length_for_earliest_and_span_for_capacity() {
    let all = wrapper(&["*.rs"], info::Kind::All);
    let mut account = Account;
    let mut session = Session::new(&mut account);
    assert!(all.get(0, 0, false, &mut session).unwrap().is_none());
    let wrapper = wrapper(&["*.rs"], info::Kind::LeftmostFirst);
    let engine = wrapper.0.as_ref().unwrap();
    let maximum = engine.max_haystack_len(&mut session).unwrap();
    assert_eq!(
        maximum,
        ((256 * (1 << 10) * 8) / engine.nfa.0.states.len()) - 1
    );
    for (total, span, earliest, expected) in [
        (128, 128, true, true),
        (129, 1, true, false),
        (129, 1, false, true),
        (maximum, maximum, false, true),
        (maximum + 1, maximum + 1, false, false),
        (maximum + 1, maximum, false, true),
    ] {
        assert_eq!(
            wrapper
                .get(total, span, earliest, &mut session)
                .unwrap()
                .is_some(),
            expected
        );
    }
    // Source saturates the maximum toward zero if the bit budget is smaller
    // than the NFA, rather than underflowing. This is an internal source fixture.
    let mut wrapper = wrapper;
    wrapper.0.as_mut().unwrap().config.visited_capacity = Some(0);
    assert_eq!(
        wrapper
            .0
            .as_ref()
            .unwrap()
            .max_haystack_len(&mut session)
            .unwrap(),
        0
    );
}

#[test]
fn cache_initialization_is_lazy_allocation_free_and_published_only_on_success() {
    let wrapper = wrapper(&["*.rs"], info::Kind::LeftmostFirst);
    let engine = wrapper.0.as_ref().unwrap();
    let mut count = Count::default();
    let mut lazy = wrapper.create_cache(&mut Session::new(&mut count)).unwrap();
    assert!(lazy.0.is_none());
    assert!(count.allocations.is_empty());
    let cache = lazy.get(engine, &mut Session::new(&mut count)).unwrap();
    assert!(cache.stack.values.is_empty());
    assert!(cache.visited.bitset.values.is_empty());
    assert_eq!(cache.visited.stride, 0);
    assert!(count.allocations.is_empty());

    let mut count = Count::default();
    let mut initialized = cache::Lazy::none();
    initialized
        .get(engine, &mut Session::new(&mut count))
        .unwrap();
    for refuse in 0..count.calls {
        let mut account = Count {
            refuse: Some(refuse),
            ..Count::default()
        };
        let mut session = Session::new(&mut account);
        let mut lazy = cache::Lazy::none();
        assert!(matches!(lazy.get(engine, &mut session), Err(Stop::Refused)));
        assert!(lazy.0.is_none());
        assert!(matches!(lazy.get(engine, &mut session), Err(Stop::Refused)));
        assert_eq!(account.calls, refuse + 1);
    }
}

#[test]
fn reset_preserves_stack_stride_capacity_and_disabled_wrapper_keeps_cache() {
    let small = wrapper(&["a"], info::Kind::LeftmostFirst);
    let large = wrapper(&["src/**/a?.rs"], info::Kind::LeftmostFirst);
    let disabled = wrapper(&["a"], info::Kind::All);
    let mut lazy = seeded(small.0.as_ref().unwrap());
    let cache = lazy.0.as_mut().unwrap();
    cache.stack.values[1] = cache::Frame::RestoreCapture {
        slot: 1,
        offset: NonZeroUsize::new(7),
    };
    let pointer = cache.visited.bitset.values.as_ptr();
    let capacity = cache.visited.bitset.admitted;
    let length = cache.visited.bitset.values.len();
    let mut count = Count::default();
    lazy.reset(&disabled, &mut Session::new(&mut count))
        .unwrap();
    assert_eq!(lazy.0.as_ref().unwrap().visited.bitset.values.len(), length);
    assert!(lazy
        .0
        .as_ref()
        .unwrap()
        .visited
        .bitset
        .values
        .iter()
        .all(|&word| word == usize::MAX));
    lazy.reset(&large, &mut Session::new(&mut count)).unwrap();
    let cache = lazy
        .get(large.0.as_ref().unwrap(), &mut Session::new(&mut count))
        .unwrap();
    assert!(cache.visited.bitset.values.is_empty());
    assert_eq!(cache.visited.bitset.values.as_ptr(), pointer);
    assert_eq!(cache.visited.bitset.admitted, capacity);
    assert_eq!(cache.visited.stride, 18);
    assert!(matches!(cache.stack.values[0], cache::Frame::Step { sid, at: 8 } if sid.index() == 1));
    assert!(
        matches!(cache.stack.values[1], cache::Frame::RestoreCapture { slot: 1, offset } if offset == NonZeroUsize::new(7))
    );
    assert!(count.allocations.is_empty());
    let mut none = cache::Lazy::none();
    none.reset(&disabled, &mut Session::new(&mut count))
        .unwrap();
    assert!(none.0.is_none());
    none.reset(&large, &mut Session::new(&mut count)).unwrap();
    assert!(none.0.is_some());
    assert!(count.allocations.is_empty());
}

#[test]
fn setup_sizes_the_real_bitset_and_reuses_growth_after_truncation() {
    let wrapper = wrapper(&["*.rs"], info::Kind::LeftmostFirst);
    let engine = wrapper.0.as_ref().unwrap();
    let mut count = Count::default();
    let mut lazy = wrapper.create_cache(&mut Session::new(&mut count)).unwrap();
    let cache = lazy.get(engine, &mut Session::new(&mut count)).unwrap();
    let first = blocks(engine, 17);
    cache
        .setup(engine, 17, &mut Session::new(&mut count))
        .unwrap()
        .unwrap();
    assert_eq!(cache.visited.bitset.values.len(), first);
    assert_eq!(count.allocations, [layout::<usize>(first.max(4))]);
    let capacity = cache.visited.bitset.admitted;
    let grown = blocks(engine, 513);
    cache.visited.bitset.values.fill(usize::MAX);
    cache
        .setup(engine, 513, &mut Session::new(&mut count))
        .unwrap()
        .unwrap();
    assert_eq!(cache.visited.bitset.values.len(), grown);
    assert_eq!(
        count.allocations[1],
        layout::<usize>((capacity * 2).max(grown))
    );
    assert!(cache.visited.bitset.values.iter().all(|&word| word == 0));
    let pointer = cache.visited.bitset.values.as_ptr();
    let capacity = cache.visited.bitset.admitted;
    cache.visited.bitset.values.fill(usize::MAX);
    cache
        .setup(engine, 0, &mut Session::new(&mut count))
        .unwrap()
        .unwrap();
    assert_eq!(cache.visited.bitset.values.len(), blocks(engine, 0));
    assert_eq!(cache.visited.stride, 1);
    cache
        .setup(engine, 513, &mut Session::new(&mut count))
        .unwrap()
        .unwrap();
    assert_eq!(cache.visited.bitset.values.as_ptr(), pointer);
    assert_eq!(cache.visited.bitset.admitted, capacity);
    assert_eq!(count.allocations.len(), 2);
    assert!(cache.visited.bitset.values.iter().all(|&word| word == 0));
}

#[test]
fn visited_pairs_distinguish_states_offsets_and_block_edges_then_clear() {
    use super::super::builder::Id;
    let wrapper = wrapper(&["*.rs"], info::Kind::LeftmostFirst);
    let engine = wrapper.0.as_ref().unwrap();
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut lazy = cache::Lazy::none();
    let cache = lazy.get(engine, &mut session).unwrap();
    cache.setup(engine, 129, &mut session).unwrap().unwrap();
    let last = Id((engine.nfa.0.states.len() - 1) as u32);
    for (id, at) in [
        (Id(0), 0),
        (Id(0), 63),
        (Id(0), 64),
        (Id(1), 0),
        (last, 129),
    ] {
        assert!(cache.visited.insert(id, at, &mut session).unwrap());
        assert!(!cache.visited.insert(id, at, &mut session).unwrap());
    }
    cache.setup(engine, 129, &mut session).unwrap().unwrap();
    assert!(cache.visited.insert(last, 129, &mut session).unwrap());
    let previous = cache.visited.bitset.values.clone();
    let mut refused = Count {
        refuse: Some(0),
        ..Count::default()
    };
    let mut session = Session::new(&mut refused);
    assert!(matches!(
        cache.visited.insert(Id(0), 0, &mut session),
        Err(Stop::Refused)
    ));
    assert_eq!(cache.visited.bitset.values, previous);
    assert!(matches!(
        cache.visited.insert(Id(0), 0, &mut session),
        Err(Stop::Refused)
    ));
    assert_eq!(refused.calls, 1);
}

#[test]
fn too_long_clears_stack_and_records_stride_before_returning_semantic_failure() {
    let wrapper = wrapper(&["*.rs"], info::Kind::LeftmostFirst);
    let engine = wrapper.0.as_ref().unwrap();
    let maximum = engine
        .max_haystack_len(&mut Session::new(&mut Account))
        .unwrap();
    for length in [maximum + 1, usize::MAX / 2] {
        let mut lazy = seeded(engine);
        let cache = lazy.0.as_mut().unwrap();
        let before = cache.visited.bitset.values.clone();
        let capacity = cache.visited.bitset.admitted;
        let mut count = Count::default();
        let mut session = Session::new(&mut count);
        assert_eq!(
            cache.setup(engine, length, &mut session).unwrap(),
            Err(cache::TooLong { length })
        );
        assert!(cache.stack.values.is_empty());
        assert_eq!(cache.visited.stride, length + 1);
        assert_eq!(cache.visited.bitset.values, before);
        assert_eq!(cache.visited.bitset.admitted, capacity);
        assert!(session.check().is_ok());
        cache.setup(engine, 0, &mut session).unwrap().unwrap();
        assert!(count.allocations.is_empty());
    }
}

#[test]
fn allocation_refusal_occurs_after_old_words_clear_and_before_any_replacement() {
    let wrapper = wrapper(&["*.rs"], info::Kind::LeftmostFirst);
    let engine = wrapper.0.as_ref().unwrap();
    let mut baseline = Count::default();
    let mut lazy = seeded(engine);
    lazy.0
        .as_mut()
        .unwrap()
        .setup(engine, 513, &mut Session::new(&mut baseline))
        .unwrap()
        .unwrap();
    for refuse in 0..baseline.calls {
        let mut lazy = seeded(engine);
        let cache = lazy.0.as_mut().unwrap();
        let capacity = cache.visited.bitset.admitted;
        let length = cache.visited.bitset.values.len();
        let pointer = cache.visited.bitset.values.as_ptr();
        let mut count = Count {
            refuse: Some(refuse),
            ..Count::default()
        };
        let mut session = Session::new(&mut count);
        assert!(matches!(
            cache.setup(engine, 513, &mut session),
            Err(Stop::Refused)
        ));
        assert_eq!(cache.visited.bitset.admitted, capacity);
        assert_eq!(cache.visited.bitset.values.len(), length);
        assert_eq!(cache.visited.bitset.values.as_ptr(), pointer);
        assert!(matches!(
            cache.setup(engine, 0, &mut session),
            Err(Stop::Refused)
        ));
        assert_eq!(count.calls, refuse + 1);
    }
    struct RefuseAllocation;
    impl Admission for RefuseAllocation {
        type Error = ();
        fn admit(&mut self, request: Request) -> Result<(), ()> {
            if matches!(request, Request::Allocation { .. }) {
                Err(())
            } else {
                Ok(())
            }
        }
    }
    let mut lazy = seeded(engine);
    let cache = lazy.0.as_mut().unwrap();
    let length = cache.visited.bitset.values.len();
    assert!(matches!(
        cache.setup(engine, 513, &mut Session::new(&mut RefuseAllocation)),
        Err(Stop::Refused)
    ));
    assert!(cache.stack.values.is_empty());
    assert_eq!(cache.visited.stride, 514);
    assert_eq!(cache.visited.bitset.values.len(), length);
    assert!(cache.visited.bitset.values.iter().all(|&word| word == 0));
}

#[test]
fn allocator_bonus_capacity_never_authorizes_visited_growth() {
    let wrapper = wrapper(&["*.rs"], info::Kind::LeftmostFirst);
    let engine = wrapper.0.as_ref().unwrap();
    let mut lazy = cache::Lazy::none();
    let cache = lazy.get(engine, &mut Session::new(&mut Account)).unwrap();
    let needed = blocks(engine, 513);
    cache.visited.bitset.values = Vec::with_capacity(needed * 4);
    cache.visited.bitset.values.resize(4, usize::MAX);
    cache.visited.bitset.admitted = 4;
    let mut count = Count::default();
    cache
        .setup(engine, 513, &mut Session::new(&mut count))
        .unwrap()
        .unwrap();
    assert_eq!(count.allocations, [layout::<usize>(needed.max(8))]);
    assert_eq!(cache.visited.bitset.admitted, needed.max(8));
}

#[test]
fn original_account_keeps_prior_work_through_constructor_cache_and_setup() {
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
    fn operation<A: Admission>(
        info: &info::Info,
        nfa: &finalize::Nfa,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        let mut prior = super::super::storage::Array::<usize>::new();
        prior.resize(13, 7, session)?;
        session.items(37)?;
        let wrapper = Backtrack::new(info, nfa, session)?;
        let engine = wrapper.0.as_ref().unwrap();
        let mut lazy = wrapper.create_cache(session)?;
        let cache = lazy.get(engine, session)?;
        assert!(cache.setup(engine, 513, session)?.is_ok());
        assert_eq!(wrapper.is_match(&mut lazy, b"foo.rs", session)?, Some(true));
        Ok(())
    }
    let (info, nfa) = prepared(&["*.rs"], info::Kind::LeftmostFirst);
    let mut count = Count::default();
    operation(&info, &nfa, &mut Session::new(&mut count)).unwrap();
    let items = count.items;
    let bytes = count
        .allocations
        .iter()
        .map(|&(bytes, _)| bytes)
        .sum::<usize>();
    for (item_limit, byte_limit, success) in [
        (items, bytes, true),
        (items - 1, bytes, false),
        (items, bytes - 1, false),
    ] {
        let mut account = Budget {
            items: 0,
            bytes: 0,
            item_limit,
            byte_limit,
        };
        let mut session = Session::new(&mut account);
        assert_eq!(operation(&info, &nfa, &mut session).is_ok(), success);
        if success {
            assert_eq!((account.items, account.bytes), (items, bytes));
        } else {
            assert!(matches!(session.check(), Err(Stop::Refused)));
            assert!(matches!(
                operation(&info, &nfa, &mut session),
                Err(Stop::Refused)
            ));
        }
        assert_eq!(Arc::strong_count(&nfa.0), 1);
    }
}
