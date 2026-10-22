use super::*;

pub(super) fn both(kind: info::Kind) -> Core {
    let mut core = core(&["a", "a!!", "zz"], kind, Case::Default);
    assert!(core.dfa.has_engine());
    // Core normally suppresses hybrid after dense construction succeeds. This
    // test-only owner installs a real completed hybrid alongside a faulted
    // dense table, making an accidental second fast-engine dispatch observable.
    core.hybrid = hybrid::Hybrid::new(
        &core.info,
        &core.nfa,
        core.reverse.as_ref().unwrap(),
        &mut Session::new(&mut Account),
    )
    .unwrap();
    assert!(core.hybrid.has_engine());
    core
}

pub(super) fn minimum(kind: info::Kind) -> Core {
    let mut core = core(&[LONG], kind, Case::Default);
    assert!(!core.dfa.has_engine());
    let capacity = hybrid::Hybrid::test_minimum_capacity(
        &core.info,
        &core.nfa,
        core.reverse.as_ref().unwrap(),
    );
    core.info.test_hybrid_capacity(capacity);
    core.hybrid = hybrid::Hybrid::new(
        &core.info,
        &core.nfa,
        core.reverse.as_ref().unwrap(),
        &mut Session::new(&mut Account),
    )
    .unwrap();
    assert!(core.hybrid.has_engine());
    core
}

pub(super) const LONG: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

#[test]
fn dense_semantic_quit_and_unsupported_start_skip_present_hybrid() {
    for kind in [info::Kind::All, info::Kind::LeftmostFirst] {
        for unsupported in [false, true] {
            let mut core = both(kind);
            if unsupported {
                core.dfa.test_unsupported_start();
            } else {
                core.dfa.test_quit_at_start(b'a');
            }
            let mut source_cache = core.create_cache(&mut Session::new(&mut Account)).unwrap();
            let mut expected = Count::default();
            let mut session = Session::new(&mut expected);
            session.items(1).unwrap(); // Core's dense-wrapper selection.
            let failure = core.dfa.is_match(b"a", &mut session).unwrap();
            assert_eq!(
                failure,
                Err(if unsupported {
                    dfa::Failure::UnsupportedAnchored
                } else {
                    dfa::Failure::Quit {
                        byte: b'a',
                        offset: 0,
                    }
                })
            );
            session.check().unwrap();
            session.items(1).unwrap(); // Actual absent one-pass predicate.
            let bb = core
                .backtrack
                .is_match(&mut source_cache.backtrack, b"a", &mut session)
                .unwrap();
            assert_eq!(
                bb,
                if kind == info::Kind::LeftmostFirst {
                    Some(true)
                } else {
                    None
                }
            );
            if bb.is_none() {
                assert!(core
                    .pike
                    .is_match(&mut source_cache.pike, b"a", &mut session)
                    .unwrap());
            }
            session.finish(Ok(())).unwrap();
            let mut cache = core.create_cache(&mut Session::new(&mut Account)).unwrap();
            let mut count = Count::default();
            assert!(core
                .is_match(&mut cache, b"a", &mut Session::new(&mut count))
                .unwrap());
            assert_eq!(trace(&count), trace(&expected));
        }
    }
}

#[test]
fn dense_overlap_quit_goes_directly_to_pike_and_keeps_existing_and_partial_ids() {
    let mut core = both(info::Kind::All);
    core.dfa.test_match_then_quit(b"a", b'!', 0);
    let mut source_cache = core.create_cache(&mut Session::new(&mut Account)).unwrap();
    let mut source_set = set(3, &[2]);
    let mut expected = Count::default();
    let mut session = Session::new(&mut expected);
    session.items(1).unwrap();
    assert_eq!(
        core.dfa
            .overlapping(b"a!!", &mut source_set, &mut session)
            .unwrap(),
        Err(dfa::Failure::Quit {
            byte: b'!',
            offset: 2
        })
    );
    assert_eq!(ids(&source_set), [0, 2]);
    session.check().unwrap();
    core.pike
        .overlapping(
            &mut source_cache.pike,
            b"a!!",
            &mut source_set,
            &mut session,
        )
        .unwrap();
    session.finish(Ok(())).unwrap();
    assert_eq!(ids(&source_set), [0, 1, 2]);

    let mut cache = core.create_cache(&mut Session::new(&mut Account)).unwrap();
    let mut found = set(3, &[2]);
    let mut count = Count::default();
    core.overlapping(
        &mut cache,
        b"a!!",
        &mut found,
        &mut Session::new(&mut count),
    )
    .unwrap();
    assert_eq!(ids(&found), [0, 1, 2]);
    assert_eq!(trace(&count), trace(&expected));
}

#[test]
fn real_hybrid_efficiency_failure_reaches_boolean_and_direct_overlap_fallback() {
    for kind in [info::Kind::All, info::Kind::LeftmostFirst] {
        for overlap in [false, true] {
            let core = minimum(kind);
            let mut source_cache = core.create_cache(&mut Session::new(&mut Account)).unwrap();
            let mut source_set = set(2, &[]);
            let mut expected = Count::default();
            let mut session = Session::new(&mut expected);
            session.items(1).unwrap();
            session.items(1).unwrap();
            if overlap {
                assert!(matches!(
                    core.hybrid
                        .overlapping(
                            &mut source_cache.hybrid,
                            LONG.as_bytes(),
                            &mut source_set,
                            &mut session
                        )
                        .unwrap(),
                    Err(hybrid::Failure::GaveUp { .. })
                ));
                session.check().unwrap();
                core.pike
                    .overlapping(
                        &mut source_cache.pike,
                        LONG.as_bytes(),
                        &mut source_set,
                        &mut session,
                    )
                    .unwrap();
                assert_eq!(ids(&source_set), [0]);
            } else {
                assert!(matches!(
                    core.hybrid
                        .is_match(&mut source_cache.hybrid, LONG.as_bytes(), &mut session)
                        .unwrap(),
                    Err(hybrid::Failure::GaveUp { .. })
                ));
                session.check().unwrap();
                session.items(1).unwrap();
                let bb = core
                    .backtrack
                    .is_match(&mut source_cache.backtrack, LONG.as_bytes(), &mut session)
                    .unwrap();
                assert_eq!(
                    bb,
                    if kind == info::Kind::LeftmostFirst {
                        Some(true)
                    } else {
                        None
                    }
                );
                if bb.is_none() {
                    assert!(core
                        .pike
                        .is_match(&mut source_cache.pike, LONG.as_bytes(), &mut session)
                        .unwrap());
                }
            }
            session.finish(Ok(())).unwrap();
            let mut cache = core.create_cache(&mut Session::new(&mut Account)).unwrap();
            let mut count = Count::default();
            if overlap {
                let mut found = set(2, &[]);
                core.overlapping(
                    &mut cache,
                    LONG.as_bytes(),
                    &mut found,
                    &mut Session::new(&mut count),
                )
                .unwrap();
                assert_eq!(ids(&found), [0]);
            } else {
                assert!(core
                    .is_match(&mut cache, LONG.as_bytes(), &mut Session::new(&mut count))
                    .unwrap());
            }
            assert_eq!(trace(&count), trace(&expected));
        }
    }
}
