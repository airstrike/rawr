use super::*;

fn records(present: bool) -> [observed::Record; 2] {
    [
        observed::Record {
            query: observed::Query::Environment(b"CARGO_PATH_BASES_WORKSPACE".to_vec()),
            value: observed::Value::Environment(None),
        },
        observed::Record {
            query: observed::Query::EnvironmentPrefix(b"CARGO_PATH_BASES_WORKSPACE_".to_vec()),
            value: observed::Value::EnvironmentPrefix(present),
        },
    ]
}

#[test]
fn prefix_presence_survives_codec_and_is_distinct_from_exact_absence() {
    for present in [false, true] {
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::new(&limits);
        let mut budget = Budget {
            graph: &graph,
            allocation: &mut allocation,
        };
        let image = stored::Stored::from_records(&records(present), &mut budget).unwrap();
        let encoded = image.encode(&mut budget).unwrap();
        let image = stored::Stored::decode(&encoded, &mut budget).unwrap();
        assert_eq!(image.encode(&mut budget).unwrap(), encoded);
        for length in 0..encoded.len() {
            assert!(stored::Stored::decode(&encoded[..length], &mut budget).is_err());
        }
        let mut host = host::Host::from_image(&image, budget).unwrap();
        let mut session = Session::new(&mut host);
        assert_eq!(
            session.environment(b"CARGO_PATH_BASES_WORKSPACE").unwrap(),
            None
        );
        assert_eq!(
            session
                .environment_prefix(b"CARGO_PATH_BASES_WORKSPACE_")
                .unwrap(),
            present
        );
        session.finish(Ok(())).unwrap();
        host.finish().unwrap();
    }
}

#[test]
fn unobserved_prefix_is_terminal_and_does_not_consume_a_different_spelling() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let image = stored::Stored::from_records(&records(true)[1..], &mut budget).unwrap();
    let mut host = host::Host::from_image(&image, budget).unwrap();
    let mut session = Session::new(&mut host);
    assert!(matches!(
        session.environment_prefix(b"CARGO_PATH_BASES_WORKSPACE"),
        Err(Stop::Refused)
    ));
    assert!(matches!(
        session.environment_prefix(b"CARGO_PATH_BASES_WORKSPACE_"),
        Err(Stop::Refused)
    ));
    assert!(matches!(
        session.finish(Ok(())),
        Err(Error::Host(InputError::Unobserved))
    ));
    assert!(matches!(host.finish(), Err(InputError::Unconsumed)));
}

#[test]
fn prefix_observations_reject_invalid_keys_and_cross_typed_answers() {
    for key in [b"".as_slice(), b"BAD\0KEY", b"BAD=KEY"] {
        assert!(observed::Record {
            query: observed::Query::EnvironmentPrefix(key.to_vec()),
            value: observed::Value::EnvironmentPrefix(false),
        }
        .checked()
        .is_err());
    }
    assert!(observed::Record {
        query: observed::Query::EnvironmentPrefix(b"CARGO_".to_vec()),
        value: observed::Value::Environment(None),
    }
    .checked()
    .is_err());
    assert!(observed::Record {
        query: observed::Query::Environment(b"CARGO_".to_vec()),
        value: observed::Value::EnvironmentPrefix(false),
    }
    .checked()
    .is_err());
}

#[test]
fn prefix_query_records_the_original_string_target_and_still_answers() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let image = stored::Stored::from_records(
        &records(true)[1..],
        &mut Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    )
    .unwrap();
    let prefix = b"CARGO_PATH_BASES_WORKSPACE_";
    allocation.string_limit = prefix.len() - 1;
    let mut host = host::Host::from_image(
        &image,
        Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    )
    .unwrap();
    let mut session = Session::new(&mut host);
    // The owned prefix crossing the string target is recorded on the
    // original account; the query still answers and consumes its record.
    assert!(session.environment_prefix(prefix).unwrap());
    session.finish(Ok(())).unwrap();
    host.finish().unwrap();
    assert!(allocation.check().is_ok(), "accounting targets never latch");
    let exceeded = allocation.exceedances();
    let mut axes = exceeded.axes();
    let (Some(axis), None) = (axes.next(), axes.next()) else {
        panic!("only the owned string axis is recorded: {exceeded:?}");
    };
    assert_eq!(
        (
            axis.name,
            axis.target,
            axis.first_requested,
            axis.high_water
        ),
        (
            "membership owned string bytes",
            (prefix.len() - 1) as u64,
            prefix.len() as u64,
            prefix.len() as u64
        )
    );
}

#[test]
fn refused_original_account_latches_every_later_prefix_query() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let image = stored::Stored::from_records(
        &records(true)[1..],
        &mut Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    )
    .unwrap();
    let mut host = host::Host::from_image(
        &image,
        Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    )
    .unwrap();
    // Only a genuinely refused original account refuses.
    host.budget().allocation.refuse();
    let mut session = Session::new(&mut host);
    assert!(matches!(
        session.environment_prefix(b"CARGO_PATH_BASES_WORKSPACE_"),
        Err(Stop::Refused)
    ));
    assert!(matches!(
        session.environment_prefix(b"X"),
        Err(Stop::Refused)
    ));
    assert!(matches!(
        session.finish(Ok(())),
        Err(Error::Host(InputError::Input(_)))
    ));
    assert!(matches!(host.finish(), Err(InputError::Unconsumed)));
}
