use super::super::{
    dfa, hybrid, info,
    tests::{expressions, info, Case, Count},
};
use super::*;
use crate::input::membership::manifest::{reference::Account, Error, Stop};
use rawr_toml::document::Request;

mod fallback;
mod refusal;

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

fn ids(set: &Set) -> Vec<u32> {
    let mut cursor = 0;
    let mut ids = Vec::new();
    while let Some(id) = set
        .next(&mut cursor, &mut Session::new(&mut Account))
        .unwrap()
    {
        ids.push(id);
    }
    ids
}

fn set(capacity: usize, existing: &[u32]) -> Set {
    let mut set = Set::new(capacity, &mut Session::new(&mut Account)).unwrap();
    for &id in existing {
        set.insert(id, &mut Session::new(&mut Account))
            .unwrap()
            .unwrap();
    }
    set
}

fn allocations(count: &Count) -> Vec<Request> {
    count
        .requests
        .borrow()
        .iter()
        .copied()
        .filter(|request| matches!(request, Request::Allocation { .. }))
        .collect()
}

fn trace(count: &Count) -> Vec<Request> {
    count.requests.borrow().clone()
}

#[test]
fn real_dense_hybrid_and_nfa_dispatch_keep_boolean_and_overlap_language() {
    for kind in [info::Kind::All, info::Kind::LeftmostFirst] {
        for (case, dense, hybrid) in [
            (Case::Default, true, false),
            (Case::OnlyHybrid, false, true),
            (Case::Neither, false, false),
        ] {
            let core = core(&["a", "?", ""], kind, case);
            assert_eq!(
                (core.dfa.has_engine(), core.hybrid.has_engine()),
                (dense, hybrid)
            );
            let mut cache = core.create_cache(&mut Session::new(&mut Account)).unwrap();
            for (bytes, all) in [
                (b"a".as_slice(), vec![0, 1]),
                (b"b", vec![1]),
                (b"", vec![2]),
                (b"\xff", vec![1]),
                (b"aa", vec![]),
                (b"/", vec![]),
            ] {
                let expected = if kind == info::Kind::All {
                    all
                } else {
                    all.into_iter().take(1).collect()
                };
                assert_eq!(
                    core.is_match(&mut cache, bytes, &mut Session::new(&mut Account))
                        .unwrap(),
                    !expected.is_empty()
                );
                let mut found = set(3, &[]);
                core.overlapping(
                    &mut cache,
                    bytes,
                    &mut found,
                    &mut Session::new(&mut Account),
                )
                .unwrap();
                assert_eq!(ids(&found), expected);
            }
        }
    }
}

#[test]
fn completed_backtracker_false_returns_without_pike_and_long_earliest_declines() {
    let core = core(&["a"], info::Kind::LeftmostFirst, Case::Neither);
    let mut expected_cache = core.create_cache(&mut Session::new(&mut Account)).unwrap();
    let mut expected = Count::default();
    assert_eq!(
        core.backtrack
            .is_match(
                &mut expected_cache.backtrack,
                b"b",
                &mut Session::new(&mut expected)
            )
            .unwrap(),
        Some(false)
    );
    let mut cache = core.create_cache(&mut Session::new(&mut Account)).unwrap();
    let mut count = Count::default();
    assert!(!core
        .is_match(&mut cache, b"b", &mut Session::new(&mut count))
        .unwrap());
    let mut expected_trace = vec![Request::Items(1); 3];
    expected_trace.extend(trace(&expected));
    assert_eq!(trace(&count), expected_trace);

    let core = self::core(&["*"], info::Kind::LeftmostFirst, Case::Neither);
    for length in [128, 129] {
        let bytes = vec![b'a'; length];
        let mut expected_cache = core.create_cache(&mut Session::new(&mut Account)).unwrap();
        let mut expected = Count::default();
        let bb = core
            .backtrack
            .is_match(
                &mut expected_cache.backtrack,
                &bytes,
                &mut Session::new(&mut expected),
            )
            .unwrap();
        assert_eq!(bb, if length == 128 { Some(true) } else { None });
        if length == 129 {
            assert!(core
                .pike
                .is_match(
                    &mut expected_cache.pike,
                    &bytes,
                    &mut Session::new(&mut expected)
                )
                .unwrap());
        }
        let mut cache = core.create_cache(&mut Session::new(&mut Account)).unwrap();
        let mut count = Count::default();
        assert!(core
            .is_match(&mut cache, &bytes, &mut Session::new(&mut count))
            .unwrap());
        let mut expected_trace = vec![Request::Items(1); 3];
        expected_trace.extend(trace(&expected));
        assert_eq!(trace(&count), expected_trace);
    }
}

#[test]
fn repeated_dispatch_reuses_actual_engine_storage_without_resetting_caches() {
    for kind in [info::Kind::All, info::Kind::LeftmostFirst] {
        for case in [Case::Default, Case::OnlyHybrid, Case::Neither] {
            let core = core(&["a", "a"], kind, case);
            let mut cache = core.create_cache(&mut Session::new(&mut Account)).unwrap();
            let mut found = set(3, &[]);
            let mut account = Count::default();
            let requests = std::rc::Rc::clone(&account.requests);
            let mut session = Session::new(&mut account);
            assert!(core.is_match(&mut cache, b"a", &mut session).unwrap());
            core.overlapping(&mut cache, b"a", &mut found, &mut session)
                .unwrap();
            let cold_end = requests.borrow().len();
            for _ in 0..3 {
                found.clear(&mut session).unwrap();
                assert!(core.is_match(&mut cache, b"a", &mut session).unwrap());
                core.overlapping(&mut cache, b"a", &mut found, &mut session)
                    .unwrap();
            }
            session.finish(Ok(())).unwrap();
            assert!(requests.borrow()[cold_end..]
                .iter()
                .all(|request| !matches!(request, Request::Allocation { .. })));
            assert_eq!(
                ids(&found),
                if kind == info::Kind::All {
                    vec![0, 1]
                } else {
                    vec![0]
                }
            );
            assert_eq!(allocations(&account).is_empty(), core.dfa.has_engine());
        }
    }
}
