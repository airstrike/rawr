use super::*;
use crate::input::membership::manifest::{reference::Account, Stop};
use rawr_toml::document::Request;
use std::{alloc::Layout, num::NonZeroUsize};

#[derive(Default)]
struct Count {
    calls: usize,
    refuse: Option<usize>,
    allocations: Vec<(usize, usize)>,
}
impl Admission for Count {
    type Error = ();
    fn admit(&mut self, request: Request) -> Result<(), ()> {
        let call = self.calls;
        self.calls += 1;
        if self.refuse == Some(call) {
            return Err(());
        }
        if let Request::Allocation { bytes, alignment } = request {
            self.allocations.push((bytes, alignment));
        }
        Ok(())
    }
}

fn translated(globs: &[&str]) -> Vec<super::super::super::translate::Expression> {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    globs
        .iter()
        .map(|glob| {
            let regex = super::super::super::super::parse(glob, &mut session)
                .unwrap()
                .ok()
                .unwrap()
                .regex(&mut session)
                .unwrap();
            let ast = regex.tree(&mut session).unwrap().ok().unwrap();
            super::super::super::translate::tree(&regex, &ast, &mut session)
                .unwrap()
                .ok()
                .unwrap()
        })
        .collect()
}

fn prepared(globs: &[&str], kind: info::Kind) -> (info::Info, finalize::Nfa) {
    let expressions = translated(globs);
    info::Info::with(
        &expressions,
        kind,
        &mut Session::new(&mut Account),
        |info, session| {
            let mut compiler = Compiler::new(false, session)?;
            let nfa = compiler.build(&expressions, None, session)?.ok().unwrap();
            Ok((info, nfa))
        },
    )
    .unwrap()
}

fn engine(globs: &[&str]) -> Pike {
    let (info, nfa) = prepared(globs, info::Kind::All);
    Pike::new(&info, &nfa, &mut Session::new(&mut Account)).unwrap()
}

fn extents(engine: &Pike) -> (usize, usize) {
    let states = engine.nfa.0.states.len();
    let slots = engine.nfa.0.groups.slot_len();
    (
        states,
        states * slots + slots.max(engine.nfa.0.starts.len() * 2),
    )
}

fn layout<T>(length: usize) -> (usize, usize) {
    let layout = Layout::array::<T>(length).unwrap();
    (layout.size(), layout.align())
}

#[test]
fn info_owns_collected_properties_and_pike_shares_only_the_nfa() {
    for kind in [info::Kind::All, info::Kind::LeftmostFirst] {
        let expressions = translated(&["a*", "{é,中}"]);
        let mut count = Count::default();
        let (info, engine, weak) = info::Info::with(
            &expressions,
            kind,
            &mut Session::new(&mut count),
            |info, session| {
                assert_eq!(info.kind(), kind);
                assert_eq!(info.pattern_len(), 2);
                assert_eq!(Arc::strong_count(&info.0), 1);
                let mut compiler = Compiler::new(false, session)?;
                let nfa = compiler.build(&expressions, None, session)?.ok().unwrap();
                let weak = Arc::downgrade(&nfa.0);
                let engine = Pike::new(&info, &nfa, session)?;
                assert_eq!(Arc::strong_count(&nfa.0), 2);
                assert_eq!(engine.kind, kind);
                Ok((info, engine, weak))
            },
        )
        .unwrap();
        let weak_info = Arc::downgrade(&info.0);
        drop(expressions);
        assert_eq!(info.pattern_len(), 2);
        drop(info);
        assert!(weak_info.upgrade().is_none());
        assert!(weak.upgrade().is_some());
        drop(engine);
        assert!(weak.upgrade().is_none());

        let mut info_only = Count::default();
        let expressions = translated(&["a*"]);
        info::Info::with(
            &expressions,
            kind,
            &mut Session::new(&mut info_only),
            |info, _| {
                assert_eq!(info.pattern_len(), 1);
                Ok(())
            },
        )
        .unwrap();
        let (shared, _) = Layout::new::<[usize; 2]>()
            .extend(Layout::new::<info::Owned>())
            .unwrap();
        assert_eq!(
            info_only.allocations.last().copied(),
            Some((shared.pad_to_align().size(), shared.align()))
        );
    }
}

#[test]
fn pike_constructor_reaches_temporary_compiler_allocation_before_sharing_nfa() {
    let (info, nfa) = prepared(&["a*"], info::Kind::All);
    let mut compiler_count = Count::default();
    let temporary = Compiler::new(false, &mut Session::new(&mut compiler_count)).unwrap();
    drop(temporary);
    assert!(!compiler_count.allocations.is_empty());
    let mut count = Count::default();
    let pike = Pike::new(&info, &nfa, &mut Session::new(&mut count)).unwrap();
    assert_eq!(count.allocations, compiler_count.allocations);
    assert_eq!(Arc::strong_count(&nfa.0), 2);
    drop(pike);
    assert_eq!(Arc::strong_count(&nfa.0), 1);
    for refuse in 0..count.calls {
        let mut account = Count {
            refuse: Some(refuse),
            ..Count::default()
        };
        let mut session = Session::new(&mut account);
        assert!(matches!(
            Pike::new(&info, &nfa, &mut session),
            Err(Stop::Refused)
        ));
        assert_eq!(Arc::strong_count(&nfa.0), 1);
        assert!(matches!(session.check(), Err(Stop::Refused)));
        assert!(matches!(
            Pike::new(&info, &nfa, &mut session),
            Err(Stop::Refused)
        ));
        assert_eq!(account.calls, refuse + 1);
    }
}

#[test]
fn cache_first_get_allocates_full_tables_and_repeated_get_allocates_nothing() {
    let engine = engine(&["a*", "b?", ""]);
    let (states, slots) = extents(&engine);
    let mut count = Count::default();
    let mut lazy = engine.create_cache(&mut Session::new(&mut count)).unwrap();
    assert!(lazy.0.is_none());
    assert!(count.allocations.is_empty());
    let pointer;
    {
        let cache = lazy.get(&engine, &mut Session::new(&mut count)).unwrap();
        assert!(cache.stack.values.is_empty());
        assert_eq!(cache.stack.admitted, 0);
        for active in [&cache.curr, &cache.next] {
            assert_eq!(active.length, 0);
            assert_eq!(active.dense.values.len(), states);
            assert_eq!(active.sparse.values.len(), states);
            assert_eq!(active.slots.table.values.len(), slots);
            assert_eq!(active.slots.per_state, 6);
            assert_eq!(active.slots.for_captures, 6);
            assert!(active.slots.table.values.iter().all(Option::is_none));
        }
        pointer = cache.curr.slots.table.values.as_ptr();
        cache.curr.slots.table.values[0] = NonZeroUsize::new(8);
    }
    let one = [
        layout::<super::super::builder::Id>(states.max(4)),
        layout::<super::super::builder::Id>(states.max(4)),
        layout::<Option<NonZeroUsize>>(slots.max(4)),
    ];
    assert_eq!(count.allocations, one.repeat(2));
    let allocations = count.allocations.len();
    let cache = lazy.get(&engine, &mut Session::new(&mut count)).unwrap();
    assert_eq!(cache.curr.slots.table.values.as_ptr(), pointer);
    assert_eq!(cache.curr.slots.table.values[0], NonZeroUsize::new(8));
    assert_eq!(count.allocations.len(), allocations);
}

#[test]
fn reset_grows_then_reuses_retained_arrays_and_preserves_the_epsilon_stack() {
    let small = engine(&["a"]);
    let large = engine(&["a*", "b?", "src/**/long-name.rs"]);
    let (small_states, small_slots) = extents(&small);
    let (large_states, large_slots) = extents(&large);
    assert!(large_states > small_states && large_slots > small_slots);
    let mut count = Count::default();
    let mut lazy = cache::Lazy::none();
    {
        let cache = lazy.get(&small, &mut Session::new(&mut count)).unwrap();
        cache.curr.length = 1;
        cache.curr.dense.values[0] = super::super::builder::Id(1);
        cache.curr.slots.table.values[0] = NonZeroUsize::new(9);
        cache
            .stack
            .resize(
                1,
                cache::Frame::Explore(super::super::builder::Id(1)),
                &mut Session::new(&mut count),
            )
            .unwrap();
    }
    let before = count.allocations.len();
    lazy.reset(&large, &mut Session::new(&mut count)).unwrap();
    assert_eq!(count.allocations.len() - before, 6);
    let grown = [
        layout::<super::super::builder::Id>((small_states.max(4) * 2).max(large_states)),
        layout::<super::super::builder::Id>((small_states.max(4) * 2).max(large_states)),
        layout::<Option<NonZeroUsize>>((small_slots.max(4) * 2).max(large_slots)),
    ];
    assert_eq!(count.allocations[before..], grown.repeat(2));
    let cache = lazy.0.as_ref().unwrap();
    assert_eq!(cache.curr.length, 0);
    assert_eq!(cache.curr.dense.values[0].index(), 1);
    assert_eq!(cache.curr.slots.table.values[0], NonZeroUsize::new(9));
    assert!(cache.curr.slots.table.values[small_slots..]
        .iter()
        .all(Option::is_none));
    assert!(matches!(cache.stack.values[0], cache::Frame::Explore(id) if id.index() == 1));
    let pointers = (
        cache.curr.dense.values.as_ptr(),
        cache.curr.slots.table.values.as_ptr(),
    );
    let capacity = (cache.curr.dense.admitted, cache.curr.slots.table.admitted);
    let before = count.allocations.len();
    lazy.reset(&small, &mut Session::new(&mut count)).unwrap();
    let cache = lazy.0.as_ref().unwrap();
    assert_eq!(
        (
            cache.curr.dense.values.len(),
            cache.curr.slots.table.values.len()
        ),
        (small_states, small_slots)
    );
    assert_eq!(
        (cache.curr.dense.admitted, cache.curr.slots.table.admitted),
        capacity
    );
    assert_eq!(
        (
            cache.curr.dense.values.as_ptr(),
            cache.curr.slots.table.values.as_ptr()
        ),
        pointers
    );
    lazy.reset(&large, &mut Session::new(&mut count)).unwrap();
    assert_eq!(count.allocations.len(), before);
    let cache = lazy.0.as_ref().unwrap();
    assert_eq!(
        (
            cache.curr.dense.values.len(),
            cache.curr.slots.table.values.len()
        ),
        (large_states, large_slots)
    );
    assert!(cache.curr.dense.values[small_states..]
        .iter()
        .all(|id| id.index() == 0));
    assert!(cache.curr.slots.table.values[small_slots..]
        .iter()
        .all(Option::is_none));
}

#[test]
fn reset_of_an_empty_wrapper_initializes_once_and_empty_patterns_have_no_slots() {
    for globs in [&[][..], &["a*"][..]] {
        let engine = engine(globs);
        let mut first = Count::default();
        let mut lazy = cache::Lazy::none();
        lazy.get(&engine, &mut Session::new(&mut first)).unwrap();
        let mut reset = Count::default();
        let mut lazy = cache::Lazy::none();
        lazy.reset(&engine, &mut Session::new(&mut reset)).unwrap();
        assert_eq!(reset.allocations, first.allocations);
        assert!(reset.calls > first.calls);
        if globs.is_empty() {
            let cache = lazy.0.as_ref().unwrap();
            assert_eq!(reset.allocations.len(), 4);
            assert_eq!(cache.curr.slots.per_state, 0);
            assert!(cache.curr.slots.table.values.is_empty());
        }
    }
}

#[test]
fn cache_initialization_and_reset_refusals_are_terminal() {
    let engine = engine(&["a*", "b?"]);
    let mut count = Count::default();
    cache::Lazy::none()
        .get(&engine, &mut Session::new(&mut count))
        .unwrap();
    for refuse in 0..count.calls {
        let mut account = Count {
            refuse: Some(refuse),
            ..Count::default()
        };
        let mut session = Session::new(&mut account);
        let mut lazy = cache::Lazy::none();
        assert!(matches!(
            lazy.get(&engine, &mut session),
            Err(Stop::Refused)
        ));
        assert!(lazy.0.is_none());
        assert!(matches!(
            lazy.reset(&engine, &mut session),
            Err(Stop::Refused)
        ));
        assert_eq!(account.calls, refuse + 1);
    }
    let small = super::tests::engine(&["a"]);
    let mut baseline = Count::default();
    let mut lazy = cache::Lazy::none();
    lazy.get(&small, &mut Session::new(&mut baseline)).unwrap();
    let before = baseline.calls;
    lazy.reset(&engine, &mut Session::new(&mut baseline))
        .unwrap();
    for refuse in before..baseline.calls {
        let mut account = Count {
            refuse: Some(refuse),
            ..Count::default()
        };
        let mut session = Session::new(&mut account);
        let mut lazy = cache::Lazy::none();
        lazy.get(&small, &mut session).unwrap();
        assert!(matches!(
            lazy.reset(&engine, &mut session),
            Err(Stop::Refused)
        ));
        assert!(lazy.0.is_some());
        assert!(matches!(lazy.get(&small, &mut session), Err(Stop::Refused)));
        assert!(matches!(session.finish(Ok(())), Err(Error::Admission(()))));
        assert_eq!(account.calls, refuse + 1);
    }
    // Allocator bonus capacity cannot bypass the next prospective request.
    let mut values = Vec::with_capacity(64);
    values.extend([7_u64; 3]);
    let mut array = storage::Array {
        values,
        admitted: 4,
    };
    let mut account = Count::default();
    array.resize(5, 9, &mut Session::new(&mut account)).unwrap();
    assert_eq!(account.allocations, [layout::<u64>(8)]);
    assert_eq!(array.values, [7, 7, 7, 9, 9]);
}

#[test]
fn replacement_refusal_retains_old_extent_through_allocation_copy_and_initialization() {
    let mut baseline = Count::default();
    let mut array = storage::Array::new();
    array
        .resize(3, 7_u64, &mut Session::new(&mut baseline))
        .unwrap();
    let before = baseline.calls;
    array
        .resize(12, 9, &mut Session::new(&mut baseline))
        .unwrap();
    assert_eq!(baseline.allocations, [layout::<u64>(4), layout::<u64>(12)]);
    for refuse in before..baseline.calls {
        let mut account = Count {
            refuse: Some(refuse),
            ..Count::default()
        };
        let mut session = Session::new(&mut account);
        let mut array = storage::Array::new();
        array.resize(3, 7_u64, &mut session).unwrap();
        let old = array.values.as_ptr();
        assert!(matches!(
            array.resize(12, 9, &mut session),
            Err(Stop::Refused)
        ));
        assert_eq!(array.values.as_ptr(), old);
        assert_eq!(array.values, [7, 7, 7]);
        assert_eq!(array.admitted, 4);
        assert!(matches!(
            array.resize(4, 0, &mut session),
            Err(Stop::Refused)
        ));
        assert_eq!(account.calls, refuse + 1);
    }
}

#[test]
fn info_refusal_prevents_downstream_publication() {
    let expressions = translated(&["a*", "b?"]);
    let mut baseline = Count::default();
    info::Info::with(
        &expressions,
        info::Kind::All,
        &mut Session::new(&mut baseline),
        |_, _| Ok(()),
    )
    .unwrap();
    for refuse in 0..baseline.calls {
        let mut account = Count {
            refuse: Some(refuse),
            ..Count::default()
        };
        let mut session = Session::new(&mut account);
        let mut entered = false;
        assert!(matches!(
            info::Info::with(&expressions, info::Kind::All, &mut session, |_, _| {
                entered = true;
                Ok(())
            }),
            Err(Stop::Refused)
        ));
        assert!(!entered);
        assert!(matches!(session.check(), Err(Stop::Refused)));
        assert_eq!(account.calls, refuse + 1);
    }
}
