use super::super::{
    finalize, info,
    tests::{expressions, info, Case, Count},
};
use super::*;
use crate::input::membership::manifest::{reference::Account, Error, Stop};
use rawr_toml::document::Request;
use std::{num::NonZeroUsize, sync::Arc};

fn core(globs: &[&str], kind: info::Kind, case: Case) -> Core {
    let expressions = expressions(globs);
    Core::new(
        info(&expressions, kind, case),
        &expressions,
        &mut Session::new(&mut Account),
    )
    .unwrap()
    .unwrap()
}

// Assemble the pinned fields independently. In particular, the expected trace
// spells out GroupInfo cloning and all-slot allocation before either lazy cache,
// instead of calling the new Captures::all composition under test.
fn source_cache(core: &Core, session: &mut Session<'_, Count>) -> Cache {
    session.items(1).unwrap();
    let group_info = Arc::clone(&core.nfa.0.groups);
    session.items(1).unwrap();
    let slots = finalize::storage::repeat(None, group_info.slot_len(), session).unwrap();
    let capmatches = captures::Captures {
        group_info,
        pid: None,
        slots,
    };
    let pike = core.pike.create_cache(session).unwrap();
    let backtrack = core.backtrack.create_cache(session).unwrap();
    session.items(1).unwrap();
    assert!(matches!(
        core.onepass,
        super::super::Onepass::Disabled | super::super::Onepass::NotUseful
    ));
    let onepass = Onepass::None;
    let hybrid = core.hybrid.create_cache(session).unwrap();
    session.items(1).unwrap();
    Cache {
        capmatches,
        pike,
        backtrack,
        onepass,
        hybrid,
        revhybrid: ReverseHybrid::None,
    }
}

fn source_reset(cache: &mut Cache, core: &Core, session: &mut Session<'_, Count>) {
    cache.pike.reset(&core.pike, session).unwrap();
    cache.backtrack.reset(&core.backtrack, session).unwrap();
    session.items(1).unwrap();
    cache.hybrid.reset(&core.hybrid, session).unwrap();
}

#[test]
fn cache_creation_orders_all_capture_slots_lazy_wrappers_and_actual_hybrid_pair() {
    for globs in [vec![], vec!["a"], vec!["a", "b"]] {
        for kind in [info::Kind::All, info::Kind::LeftmostFirst] {
            for case in [
                Case::Default,
                Case::OnlyHybrid,
                Case::BothDecline,
                Case::Neither,
            ] {
                let core = core(&globs, kind, case);
                let mut expected = Count::default();
                let source = source_cache(&core, &mut Session::new(&mut expected));
                drop(source);
                let owners = Arc::strong_count(&core.nfa.0.groups);
                let mut count = Count::default();
                let cache = core.create_cache(&mut Session::new(&mut count)).unwrap();
                assert_eq!(*count.requests.borrow(), *expected.requests.borrow());
                assert_eq!(cache.capmatches.slots.len(), 2 * globs.len());
                assert!(cache.capmatches.slots.iter().all(Option::is_none));
                assert_eq!(cache.capmatches.pid, None);
                assert!(Arc::ptr_eq(
                    &cache.capmatches.group_info,
                    &core.nfa.0.groups
                ));
                assert_eq!(Arc::strong_count(&core.nfa.0.groups), owners + 1);
                assert!(matches!(cache.onepass, Onepass::None));
                assert!(matches!(cache.revhybrid, ReverseHybrid::None));
                if !core.hybrid.has_engine() {
                    // Even a dense engine still creates Captures::all; neither
                    // lazy NFA wrapper may eagerly allocate its engine cache.
                    let allocations: Vec<_> = count
                        .requests
                        .borrow()
                        .iter()
                        .copied()
                        .filter(|r| matches!(r, Request::Allocation { .. }))
                        .collect();
                    let expected = if globs.is_empty() {
                        vec![]
                    } else {
                        vec![Request::Allocation {
                            bytes: globs.len() * 2 * std::mem::size_of::<Option<NonZeroUsize>>(),
                            alignment: std::mem::align_of::<Option<NonZeroUsize>>(),
                        }]
                    };
                    assert_eq!(allocations, expected);
                }
                drop(cache);
                assert_eq!(Arc::strong_count(&core.nfa.0.groups), owners);
            }
        }
    }
}

#[test]
fn every_cache_creation_refusal_discards_the_partial_group_owner_without_retry() {
    for case in [Case::Default, Case::OnlyHybrid] {
        let core = core(&["a", "b"], info::Kind::LeftmostFirst, case);
        let owners = Arc::strong_count(&core.nfa.0.groups);
        let mut count = Count::default();
        drop(core.create_cache(&mut Session::new(&mut count)).unwrap());
        let expected = count.requests.borrow().clone();
        for refuse in 0..expected.len() {
            let mut count = Count {
                refuse: Some(refuse),
                ..Count::default()
            };
            let mut session = Session::new(&mut count);
            assert!(matches!(
                core.create_cache(&mut session),
                Err(Stop::Refused)
            ));
            assert_eq!(Arc::strong_count(&core.nfa.0.groups), owners);
            assert!(matches!(
                core.create_cache(&mut session),
                Err(Stop::Refused)
            ));
            assert!(
                matches!(session.finish::<()>(Err(Stop::Refused)), Err(Error::Admission(cause)) if cause == refuse)
            );
            assert_eq!(count.requests.borrow().as_slice(), &expected[..=refuse]);
        }
    }
}

#[test]
fn reset_initializes_lazy_engines_then_reuses_storage_in_source_order() {
    for kind in [info::Kind::All, info::Kind::LeftmostFirst] {
        for case in [Case::Default, Case::OnlyHybrid] {
            let core = core(&["ab"], kind, case);
            let mut cache = core.create_cache(&mut Session::new(&mut Account)).unwrap();
            let mut source = core.create_cache(&mut Session::new(&mut Account)).unwrap();
            let mut first_allocations = 0;
            for reset in 0..2 {
                let mut expected = Count::default();
                source_reset(&mut source, &core, &mut Session::new(&mut expected));
                let mut count = Count::default();
                cache.reset(&core, &mut Session::new(&mut count)).unwrap();
                assert_eq!(*count.requests.borrow(), *expected.requests.borrow());
                let allocations = count
                    .requests
                    .borrow()
                    .iter()
                    .filter(|r| matches!(r, Request::Allocation { .. }))
                    .count();
                if reset == 0 {
                    first_allocations = allocations;
                    assert!(allocations >= 6); // Pike's two sparse pairs and slot tables.
                } else {
                    assert!(allocations < first_allocations);
                    // A hybrid reset retains arrays/maps, but recreates each
                    // direction's dead-state byte vector and shared owner.
                    assert_eq!(allocations, if core.hybrid.has_engine() { 4 } else { 0 });
                }
            }
        }
    }
}

#[test]
fn cross_core_reset_preserves_original_capture_owner_values_and_capacity() {
    let original = core(&["a", "b"], info::Kind::LeftmostFirst, Case::OnlyHybrid);
    let target = core(&["abcdefghijklmnop"], info::Kind::All, Case::OnlyHybrid);
    let mut cache = original
        .create_cache(&mut Session::new(&mut Account))
        .unwrap();
    cache
        .reset(&original, &mut Session::new(&mut Account))
        .unwrap();
    cache.capmatches.pid = Some(1);
    cache.capmatches.slots[0] = NonZeroUsize::new(7);
    let pointer = cache.capmatches.slots.as_ptr();
    let capacity = cache.capmatches.slots.capacity();
    let groups = Arc::downgrade(&original.nfa.0.groups);
    let graph = Arc::downgrade(&original.nfa.0);
    let reverse = Arc::downgrade(&original.reverse.as_ref().unwrap().0);
    drop(original);
    assert!(graph.upgrade().is_none());
    assert!(reverse.upgrade().is_none());
    assert_eq!(groups.strong_count(), 1);
    cache
        .reset(&target, &mut Session::new(&mut Account))
        .unwrap();
    assert!(Arc::downgrade(&cache.capmatches.group_info).ptr_eq(&groups));
    assert!(!Arc::ptr_eq(
        &cache.capmatches.group_info,
        &target.nfa.0.groups
    ));
    assert_eq!(cache.capmatches.pid, Some(1));
    assert_eq!(cache.capmatches.slots.len(), 4);
    assert_eq!(cache.capmatches.slots[0], NonZeroUsize::new(7));
    assert_eq!(cache.capmatches.slots.as_ptr(), pointer);
    assert_eq!(cache.capmatches.slots.capacity(), capacity);
    assert!(matches!(cache.revhybrid, ReverseHybrid::None));
    drop(cache);
    assert!(groups.upgrade().is_none());
}

#[test]
fn every_cold_and_reused_reset_refusal_keeps_first_cause_and_capture_owner() {
    let core = core(&["ab"], info::Kind::LeftmostFirst, Case::OnlyHybrid);
    for warm in [false, true] {
        let prepare = || {
            let mut cache = core.create_cache(&mut Session::new(&mut Account)).unwrap();
            if warm {
                cache.reset(&core, &mut Session::new(&mut Account)).unwrap();
            }
            cache
        };
        let mut source = prepare();
        let mut count = Count::default();
        source.reset(&core, &mut Session::new(&mut count)).unwrap();
        let expected = count.requests.borrow().clone();
        drop(source);
        let owners = Arc::strong_count(&core.nfa.0.groups);
        for refuse in 0..expected.len() {
            let mut cache = prepare();
            let pointer = cache.capmatches.slots.as_ptr();
            let mut count = Count {
                refuse: Some(refuse),
                ..Count::default()
            };
            let mut session = Session::new(&mut count);
            assert!(matches!(
                cache.reset(&core, &mut session),
                Err(Stop::Refused)
            ));
            assert_eq!(cache.capmatches.slots.as_ptr(), pointer);
            assert_eq!(Arc::strong_count(&core.nfa.0.groups), owners + 1);
            assert!(matches!(
                cache.reset(&core, &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(
                core.create_cache(&mut session),
                Err(Stop::Refused)
            ));
            assert!(
                matches!(session.finish::<()>(Err(Stop::Refused)), Err(Error::Admission(cause)) if cause == refuse)
            );
            drop(cache);
            assert_eq!(Arc::strong_count(&core.nfa.0.groups), owners);
            assert_eq!(count.requests.borrow().as_slice(), &expected[..=refuse]);
        }
    }
}

#[test]
fn absent_hybrid_cache_is_not_fabricated_when_reset_reaches_a_present_engine() {
    let original = core(&["a"], info::Kind::All, Case::Default);
    let target = core(&["a"], info::Kind::All, Case::OnlyHybrid);
    assert!(!original.hybrid.has_engine());
    assert!(target.hybrid.has_engine());
    let mut cache = original
        .create_cache(&mut Session::new(&mut Account))
        .unwrap();
    let mut count = Count::default();
    let mut session = Session::new(&mut count);
    assert!(matches!(
        cache.reset(&target, &mut session),
        Err(Stop::Refused)
    ));
    assert!(matches!(
        session.finish::<()>(Err(Stop::Refused)),
        Err(Error::State)
    ));
    // The existing hybrid wrapper maps its reached source unwrap to terminal
    // State. Pike's preceding lazy initialization still happened in order.
    assert!(count
        .requests
        .borrow()
        .iter()
        .any(|r| matches!(r, Request::Allocation { .. })));
    assert!(Arc::ptr_eq(
        &cache.capmatches.group_info,
        &original.nfa.0.groups
    ));
}

#[test]
fn construction_cache_and_repeated_reset_keep_one_original_cumulative_account() {
    struct Budget {
        items: usize,
        bytes: usize,
    }
    impl Admission for Budget {
        type Error = ();
        fn admit(&mut self, request: Request) -> Result<(), ()> {
            let (remaining, amount) = match request {
                Request::Items(amount) => (&mut self.items, amount),
                Request::Allocation { bytes, .. } => (&mut self.bytes, bytes),
                Request::String(_) => return Ok(()),
            };
            *remaining = remaining.checked_sub(amount).ok_or(())?;
            Ok(())
        }
    }
    fn lifecycle<A: Admission>(
        expressions: &[super::super::Expression],
        case: Case,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        let prior = finalize::storage::repeat(9_u32, 17, session)?;
        info::Info::with(
            expressions,
            info::Kind::LeftmostFirst,
            session,
            |mut info, session| {
                // These are test-only source configuration branches, as in the
                // constructor tests. Production configuration is unchanged.
                match case {
                    Case::OnlyHybrid => info.test_engines(false, true),
                    Case::Default => {}
                    _ => unreachable!(),
                }
                let core = Core::new(info, expressions, session)?.unwrap();
                let mut cache = core.create_cache(session)?;
                cache.reset(&core, session)?;
                cache.reset(&core, session)
            },
        )?;
        drop(prior);
        Ok(())
    }
    let expressions = expressions(&["ab"]);
    for case in [Case::Default, Case::OnlyHybrid] {
        let mut count = Count::default();
        lifecycle(&expressions, case, &mut Session::new(&mut count)).unwrap();
        let (mut items, mut bytes) = (0, 0);
        for request in count.requests.borrow().iter() {
            match request {
                Request::Items(amount) => items += *amount,
                Request::Allocation { bytes: amount, .. } => bytes += *amount,
                Request::String(_) => {}
            }
        }
        for (item_floor, byte_floor, success) in [
            (items, bytes, true),
            (items - 1, bytes, false),
            (items, bytes - 1, false),
        ] {
            let mut budget = Budget {
                items: item_floor,
                bytes: byte_floor,
            };
            let mut session = Session::new(&mut budget);
            let result = lifecycle(&expressions, case, &mut session);
            if success {
                assert!(session.finish(result).is_ok());
                assert_eq!((budget.items, budget.bytes), (0, 0));
            } else {
                assert!(matches!(result, Err(Stop::Refused)));
                assert!(matches!(
                    lifecycle(&expressions, case, &mut session),
                    Err(Stop::Refused)
                ));
                assert!(matches!(session.finish(result), Err(Error::Admission(()))));
            }
        }
    }
}
