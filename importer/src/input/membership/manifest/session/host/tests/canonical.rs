use super::*;

fn records() -> stored::Stored {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let records = [
        observed::Record {
            query: observed::Query::Canonical(
                observed::Path::from_bytes(b"/bad", &mut budget).expect("path"),
            ),
            value: observed::Value::Canonical(Err(13)),
        },
        observed::Record {
            query: observed::Query::Canonical(
                observed::Path::from_bytes(b"/good", &mut budget).expect("path"),
            ),
            value: observed::Value::Canonical(Ok(observed::Path::from_bytes(
                b"/native/\xff",
                &mut budget,
            )
            .expect("output"))),
        },
    ];
    let image = stored::Stored::from_records(&records, &mut budget).expect("image");
    let bytes = image.encode(&mut budget).expect("encode");
    stored::Stored::decode(&bytes, &mut budget).expect("decode")
}

#[test]
fn native_canonical_failure_is_data_and_does_not_latch_session() {
    let image = records();
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut host = host::Host::from_image(
        &image,
        Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    )
    .expect("host");
    let mut session = Session::new(&mut host);
    assert_eq!(
        session
            .canonicalize(b"/bad")
            .expect("recorded native failure"),
        Err(13)
    );
    assert_eq!(
        session.canonicalize(b"/good").expect("recorded success"),
        Ok(b"/native/\xff".as_slice())
    );
    session.finish(Ok(())).expect("no source/resource failure");
    host.finish().expect("both outcomes consumed");
}

#[test]
fn missing_malformed_or_refused_query_latches_without_native_fallback() {
    let image = records();
    for cause in 0..4 {
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        if cause == 2 {
            // An exhausted path target is recorded, not refused; overflow
            // the account so the first path check is a genuine failure.
            graph
                .paths(usize::MAX)
                .expect("the next path check overflows");
        }
        let mut allocation = input::Allocation::new(&limits);
        let mut host = host::Host::from_image(
            &image,
            Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
        )
        .expect("host");
        if cause == 3 {
            // Only a genuinely refused original account refuses.
            host.budget().allocation.refuse();
        }
        let mut session = Session::new(&mut host);
        let query = match cause {
            0 => b"/missing".as_slice(),
            1 => b"/nu\0",
            _ => b"/bad",
        };
        assert!(matches!(session.canonicalize(query), Err(Stop::Refused)));
        assert!(matches!(session.canonicalize(b"/good"), Err(Stop::Refused)));
        let error = session.finish(Ok(()));
        match cause {
            0 => assert!(matches!(error, Err(Error::Host(InputError::Unobserved)))),
            1 => assert!(matches!(error, Err(Error::Host(InputError::Encoding(_))))),
            2 => assert!(matches!(error, Err(Error::Host(InputError::Budget(_))))),
            3 => assert!(matches!(error, Err(Error::Host(InputError::Input(_))))),
            _ => unreachable!(),
        }
        assert!(matches!(host.finish(), Err(InputError::Unconsumed)));
    }
}
