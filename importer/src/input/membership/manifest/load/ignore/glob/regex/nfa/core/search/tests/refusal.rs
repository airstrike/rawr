use super::*;

fn query<A: Admission>(
    core: &Core,
    cache: &mut Cache,
    bytes: &[u8],
    found: &mut Set,
    overlap: bool,
    session: &mut Session<'_, A>,
) -> Step<()> {
    if overlap {
        core.overlapping(cache, bytes, found, session)
    } else {
        core.is_match(cache, bytes, session).map(|_| ())
    }
}

#[test]
fn every_cold_and_warm_dispatch_refusal_stays_terminal_without_fallback_or_retry() {
    let mut dense_quit = fallback::both(info::Kind::All);
    dense_quit.dfa.test_quit_at_start(b'a');
    let mut dense_partial = fallback::both(info::Kind::All);
    dense_partial.dfa.test_match_then_quit(b"a", b'!', 0);
    let mut fixtures = Vec::new();
    for kind in [info::Kind::All, info::Kind::LeftmostFirst] {
        for case in [Case::Default, Case::OnlyHybrid, Case::Neither] {
            fixtures.push((core(&["a", "a", "b"], kind, case), b"a".as_slice(), false));
        }
    }
    fixtures.push((dense_quit, b"a", false));
    fixtures.push((dense_partial, b"a!!", true));
    // Completed BB false and its >128-byte earliest decline reach different
    // branches; both must retain original refusal instead of starting Pike on
    // a fresh account or returning an already observed Boolean result.
    fixtures.push((
        core(&["a"], info::Kind::LeftmostFirst, Case::Neither),
        b"b",
        false,
    ));
    let long = [b'a'; 129];
    fixtures.push((
        core(&["*"], info::Kind::LeftmostFirst, Case::Neither),
        &long,
        false,
    ));
    for (core, bytes, partial) in fixtures {
        for overlap in [false, true] {
            for warm in [false, true] {
                let mut cache = core.create_cache(&mut Session::new(&mut Account)).unwrap();
                let mut found = set(4, if partial { &[2] } else { &[] });
                let mut baseline = Count::default();
                let requests = std::rc::Rc::clone(&baseline.requests);
                let mut session = Session::new(&mut baseline);
                if warm {
                    query(&core, &mut cache, bytes, &mut found, overlap, &mut session).unwrap();
                    found.clear(&mut session).unwrap();
                    if partial {
                        found.insert(2, &mut session).unwrap().unwrap();
                    }
                }
                let before = requests.borrow().len();
                query(&core, &mut cache, bytes, &mut found, overlap, &mut session).unwrap();
                session.finish(Ok(())).unwrap();
                let expected = trace(&baseline);
                for refuse in before..expected.len() {
                    let mut cache = core.create_cache(&mut Session::new(&mut Account)).unwrap();
                    let mut found = set(4, if partial { &[2] } else { &[] });
                    let mut count = Count {
                        refuse: Some(refuse),
                        ..Count::default()
                    };
                    let mut session = Session::new(&mut count);
                    if warm {
                        query(&core, &mut cache, bytes, &mut found, overlap, &mut session).unwrap();
                        found.clear(&mut session).unwrap();
                        if partial {
                            found.insert(2, &mut session).unwrap().unwrap();
                        }
                    }
                    assert!(matches!(
                        query(&core, &mut cache, bytes, &mut found, overlap, &mut session),
                        Err(Stop::Refused)
                    ));
                    let retained = ids(&found);
                    if partial {
                        assert!(retained.contains(&2));
                    }
                    assert!(matches!(
                        core.is_match(&mut cache, bytes, &mut session),
                        Err(Stop::Refused)
                    ));
                    assert!(matches!(
                        core.overlapping(&mut cache, bytes, &mut found, &mut session),
                        Err(Stop::Refused)
                    ));
                    assert!(matches!(
                        cache.reset(&core, &mut session),
                        Err(Stop::Refused)
                    ));
                    assert_eq!(ids(&found), retained);
                    assert!(
                        matches!(session.finish(Ok(())), Err(Error::Admission(cause)) if cause == refuse)
                    );
                    assert_eq!(trace(&count), expected[..=refuse]);
                }
            }
        }
    }
}

#[test]
fn hybrid_semantic_failure_boundary_keeps_original_refusal_in_cold_and_retained_epochs() {
    let core = fallback::minimum(info::Kind::All);
    for overlap in [false, true] {
        for warm in [false, true] {
            let mut direct = core.create_cache(&mut Session::new(&mut Account)).unwrap();
            let mut direct_set = set(2, &[]);
            let mut direct_count = Count::default();
            let direct_requests = std::rc::Rc::clone(&direct_count.requests);
            let mut direct_session = Session::new(&mut direct_count);
            if warm {
                query(
                    &core,
                    &mut direct,
                    fallback::LONG.as_bytes(),
                    &mut direct_set,
                    overlap,
                    &mut direct_session,
                )
                .unwrap();
                direct_set.clear(&mut direct_session).unwrap();
            }
            let direct_before = direct_requests.borrow().len();
            if overlap {
                assert!(core
                    .hybrid
                    .overlapping(
                        &mut direct.hybrid,
                        fallback::LONG.as_bytes(),
                        &mut direct_set,
                        &mut direct_session
                    )
                    .unwrap()
                    .is_err());
            } else {
                assert!(core
                    .hybrid
                    .is_match(
                        &mut direct.hybrid,
                        fallback::LONG.as_bytes(),
                        &mut direct_session
                    )
                    .unwrap()
                    .is_err());
            }
            direct_session.finish(Ok(())).unwrap();
            let direct_calls = trace(&direct_count).len() - direct_before;
            let mut cache = core.create_cache(&mut Session::new(&mut Account)).unwrap();
            let mut found = set(2, &[]);
            let mut baseline = Count::default();
            let requests = std::rc::Rc::clone(&baseline.requests);
            let mut session = Session::new(&mut baseline);
            if warm {
                query(
                    &core,
                    &mut cache,
                    fallback::LONG.as_bytes(),
                    &mut found,
                    overlap,
                    &mut session,
                )
                .unwrap();
                found.clear(&mut session).unwrap();
            }
            let before = requests.borrow().len();
            query(
                &core,
                &mut cache,
                fallback::LONG.as_bytes(),
                &mut found,
                overlap,
                &mut session,
            )
            .unwrap();
            session.finish(Ok(())).unwrap();
            let expected = trace(&baseline);
            let fallback_at = before + 2 + direct_calls;
            assert!(fallback_at < expected.len());
            // Component tests exhaust lazy subset transitions. This joined
            // case targets both sides of the actual semantic return plus each
            // reached allocation, including allocations in the later Pike path.
            let mut refusals = vec![
                before,
                before + 1,
                before + 2,
                fallback_at - 1,
                fallback_at,
                expected.len() - 1,
            ];
            refusals.extend(expected.iter().enumerate().skip(before).filter_map(
                |(index, request)| matches!(request, Request::Allocation { .. }).then_some(index),
            ));
            refusals.sort_unstable();
            refusals.dedup();
            for refuse in refusals {
                let mut cache = core.create_cache(&mut Session::new(&mut Account)).unwrap();
                let mut found = set(2, &[]);
                let mut count = Count {
                    refuse: Some(refuse),
                    ..Count::default()
                };
                let mut session = Session::new(&mut count);
                if warm {
                    query(
                        &core,
                        &mut cache,
                        fallback::LONG.as_bytes(),
                        &mut found,
                        overlap,
                        &mut session,
                    )
                    .unwrap();
                    found.clear(&mut session).unwrap();
                }
                assert!(matches!(
                    query(
                        &core,
                        &mut cache,
                        fallback::LONG.as_bytes(),
                        &mut found,
                        overlap,
                        &mut session
                    ),
                    Err(Stop::Refused)
                ));
                let retained = ids(&found);
                assert!(matches!(
                    query(
                        &core,
                        &mut cache,
                        fallback::LONG.as_bytes(),
                        &mut found,
                        overlap,
                        &mut session
                    ),
                    Err(Stop::Refused)
                ));
                assert_eq!(ids(&found), retained);
                assert!(
                    matches!(session.finish(Ok(())), Err(Error::Admission(cause)) if cause == refuse)
                );
                assert_eq!(trace(&count), expected[..=refuse]);
            }
        }
    }
}
