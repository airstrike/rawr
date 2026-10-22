use super::*;

#[test]
fn cold_cache_and_query_refusals_keep_the_first_cause_and_publish_only_complete_caches() {
    for overlap in [false, true] {
        let mut baseline = compiled(&["a"], info::Kind::All);
        let mut set = Set::new(1, &mut Session::new(&mut Account)).unwrap();
        let mut count = Count::default();
        let mut session = Session::new(&mut count);
        if overlap {
            baseline.overlapping(b"a", &mut set, &mut session).unwrap();
        } else {
            assert!(baseline.is_match(b"a", &mut session).unwrap());
        }
        let expected = count.requests.borrow().clone();

        let regex = compiled(&["a"], info::Kind::All);
        let mut factory = Count::default();
        drop(
            regex
                .execution
                .factory
                .create_cache(&mut Session::new(&mut factory))
                .unwrap(),
        );
        let factory_len = factory.requests.borrow().len();
        let mut gates = Count::default();
        search::impossible(&regex.imp.info, 1, &mut Session::new(&mut gates)).unwrap();
        // Input event, actual impossible predicates, then execution.get.
        let factory_start = 1 + gates.requests.borrow().len() + 1;

        for refuse in 0..expected.len() {
            let mut regex = compiled(&["a"], info::Kind::All);
            let mut set = Set::new(1, &mut Session::new(&mut Account)).unwrap();
            let mut count = Count {
                refuse: Some(refuse),
                ..Count::default()
            };
            let mut session = Session::new(&mut count);
            let result = if overlap {
                regex.overlapping(b"a", &mut set, &mut session)
            } else {
                regex.is_match(b"a", &mut session).map(|_| ())
            };
            assert!(matches!(result, Err(Stop::Refused)));
            assert_eq!(
                regex.execution.cache.is_some(),
                refuse >= factory_start + factory_len
            );
            let before = ids(&set);
            assert!(matches!(
                regex.is_match(b"a", &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(
                regex.overlapping(b"a", &mut set, &mut session),
                Err(Stop::Refused)
            ));
            assert_eq!(ids(&set), before);
            assert!(matches!(
                session.finish::<()>(Err(Stop::Refused)),
                Err(Error::Admission(cause)) if cause == refuse
            ));
            assert_eq!(count.requests.borrow().as_slice(), &expected[..=refuse]);
            drop(regex);
            // Release has no fallible pool put or additional admission.
            assert_eq!(count.requests.borrow().as_slice(), &expected[..=refuse]);
        }
    }
}

#[test]
fn outer_arc_and_factory_refusal_never_publishes_an_owner_or_restarts_the_session() {
    let source = emitted("a");
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let translated = glob::regex::batch::Batch::parse(&[&source], &mut session)
        .unwrap()
        .ok()
        .unwrap()
        .translate(&mut session)
        .unwrap()
        .ok()
        .unwrap();
    let mut count = Count::default();
    drop(
        translated
            .compile(info::Kind::All, &mut Session::new(&mut count))
            .unwrap()
            .unwrap(),
    );
    let expected = count.requests.borrow().clone();
    let last = &expected[expected.len() - 3..];
    assert!(matches!(last[0], Request::Allocation { .. }));
    assert_eq!(last[1], Request::Items(1));
    assert!(matches!(last[2], Request::Allocation { .. }));
    // Include every reached post-Core strategy predicate as well as both Arc
    // allocations and the independent factory Arc clone.
    for refuse in expected.len() - 9..expected.len() {
        let translated =
            glob::regex::batch::Batch::parse(&[&source], &mut Session::new(&mut Account))
                .unwrap()
                .ok()
                .unwrap()
                .translate(&mut Session::new(&mut Account))
                .unwrap()
                .ok()
                .unwrap();
        let mut count = Count {
            refuse: Some(refuse),
            ..Count::default()
        };
        let mut session = Session::new(&mut count);
        assert!(matches!(
            translated.compile(info::Kind::All, &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(
            glob::regex::batch::Batch::parse(&[&source], &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(
            session.finish::<()>(Err(Stop::Refused)),
            Err(Error::Admission(cause)) if cause == refuse
        ));
        assert_eq!(count.requests.borrow().as_slice(), &expected[..=refuse]);
    }
}

#[test]
fn warm_refusal_retains_cache_and_cannot_be_reclassified_as_impossibility() {
    let mut baseline = compiled(&["a"], info::Kind::LeftmostFirst);
    assert!(baseline
        .is_match(b"a", &mut Session::new(&mut Account))
        .unwrap());
    let mut count = Count::default();
    assert!(baseline
        .is_match(b"a", &mut Session::new(&mut count))
        .unwrap());
    let expected = count.requests.borrow().clone();
    for refuse in 0..expected.len() {
        let mut regex = compiled(&["a"], info::Kind::LeftmostFirst);
        assert!(regex
            .is_match(b"a", &mut Session::new(&mut Account))
            .unwrap());
        let mut count = Count {
            refuse: Some(refuse),
            ..Count::default()
        };
        let mut session = Session::new(&mut count);
        assert!(matches!(
            regex.is_match(b"a", &mut session),
            Err(Stop::Refused)
        ));
        assert!(regex.execution.cache.is_some());
        assert!(matches!(
            regex.is_match(b"", &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(
            session.finish::<()>(Err(Stop::Refused)),
            Err(Error::Admission(cause)) if cause == refuse
        ));
        assert_eq!(count.requests.borrow().as_slice(), &expected[..=refuse]);
    }
}
