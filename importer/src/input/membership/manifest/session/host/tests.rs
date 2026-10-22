use crate::input::membership::manifest::{
    load, schema::manifest::Original, session::Session, value, Error, Stop,
};
use crate::input::membership::{host, observed, stored, Budget, Error as InputError};
use crate::{cargo, input, node};

mod canonical;
mod environment;

fn image(values: &[(&[u8], observed::Value, bool)]) -> stored::Stored {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut records: Vec<_> = values
        .iter()
        .map(|(path, value, file)| {
            let path = observed::Path::from_bytes(path, &mut budget).unwrap();
            let query = if *file {
                observed::Query::File(path)
            } else {
                observed::Query::Metadata { path, follow: true }
            };
            let value = match value {
                observed::Value::Absent => observed::Value::Absent,
                observed::Value::Error(code) => observed::Value::Error(*code),
                observed::Value::Metadata(metadata) => observed::Value::Metadata(*metadata),
                observed::Value::File { bytes, end } => observed::Value::File {
                    bytes: bytes.clone(),
                    end: *end,
                },
                _ => panic!("fixture family"),
            };
            observed::Record { query, value }
        })
        .collect();
    records.sort_by(|left, right| left.query.cmp(&right.query));
    stored::Stored::from_records(&records, &mut budget).unwrap()
}

fn metadata(kind: observed::Kind) -> observed::Value {
    observed::Value::Metadata(observed::Metadata {
        kind,
        device: 1,
        inode: 2,
        owner: 3,
        mode: 0o644,
        length: 4,
        modified_seconds: 0,
        modified_nanos: 0,
        changed_seconds: 0,
        changed_nanos: 0,
    })
}

#[test]
fn predicates_preserve_all_present_kinds_absence_and_recorded_os_errors() {
    for (value, exists, file) in [
        (observed::Value::Absent, false, false),
        (observed::Value::Error(13), false, false),
        (metadata(observed::Kind::File), true, true),
        (metadata(observed::Kind::Directory), true, false),
        (metadata(observed::Kind::Symlink), true, false),
        (metadata(observed::Kind::Fifo), true, false),
        (metadata(observed::Kind::Socket), true, false),
        (metadata(observed::Kind::Character), true, false),
        (metadata(observed::Kind::Block), true, false),
        (metadata(observed::Kind::Unknown), true, false),
    ] {
        let directory = matches!(&value, observed::Value::Metadata(metadata) if metadata.kind == observed::Kind::Directory);
        let image = image(&[(b"/native/\xff", value, false)]);
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
        .unwrap();
        let mut session = Session::new(&mut host);
        assert_eq!(session.exists(b"/native/\xff").unwrap(), exists);
        assert_eq!(session.is_file(b"/native/\xff").unwrap(), file);
        assert_eq!(session.is_dir(b"/native/\xff").unwrap(), directory);
        session.finish(Ok(())).unwrap();
        host.finish().unwrap();
    }
}

#[test]
fn missing_observation_is_terminal_and_not_an_absent_file() {
    let image = image(&[(b"/known", metadata(observed::Kind::File), false)]);
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
    .unwrap();
    let mut session = Session::new(&mut host);
    assert!(matches!(session.is_file(b"/missing"), Err(Stop::Refused)));
    assert!(matches!(session.exists(b"/known"), Err(Stop::Refused)));
    assert!(matches!(
        session.finish(Ok(())),
        Err(Error::Host(InputError::Unobserved))
    ));
    // The attempted post-failure read never consumed the known record.
    assert!(matches!(host.finish(), Err(InputError::Unconsumed)));
}

#[test]
fn whole_file_read_error_precedes_utf8_and_syntax_and_stays_terminal() {
    for bytes in [vec![0xff], b"invalid={".to_vec()] {
        let image = image(&[(
            b"/Cargo.toml",
            observed::Value::File {
                bytes,
                end: observed::End::Error(5),
            },
            true,
        )]);
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
        .unwrap();
        let mut session = Session::new(&mut host);
        assert!(matches!(
            session.document(b"/Cargo.toml", &limits),
            Err(Stop::Refused)
        ));
        assert!(matches!(session.items(1), Err(Stop::Refused)));
        assert!(matches!(
            session.finish(Ok(())),
            Err(Error::Host(InputError::Io(5)))
        ));
        host.finish().unwrap();
    }
}

#[test]
fn complete_document_original_initial_configuration_and_probe_share_original_accounts() {
    use crate::input::membership::manifest::tests::admits_one_short;
    let source = b"cargo-features=['edition']\nworkspace={}\n";
    let image = image(&[
        (
            b"/Cargo.toml",
            observed::Value::File {
                bytes: source.to_vec(),
                end: observed::End::Eof,
            },
            true,
        ),
        (b"/build.rs", metadata(observed::Kind::File), false),
    ]);
    let run = |items, bytes, paths_exhausted| {
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        if paths_exhausted {
            graph.paths(node::MAX_PATH_CHECKS_PER_GRAPH).unwrap();
        }
        let mut allocation = input::Allocation::with_limits(items, bytes, usize::MAX);
        allocation.items(7).unwrap();
        allocation.bytes(31).unwrap();
        let mut host = match host::Host::from_image(
            &image,
            Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
        ) {
            Ok(host) => host,
            Err(_) => return (false, allocation.usage(), allocation.exceedances()),
        };
        let mut session = Session::new(&mut host);
        let result = (|| {
            let document = session
                .document(b"/Cargo.toml", &limits)?
                .expect("observed present file");
            let original = Original::interpret(value::Input::root(&document), &mut session)?;
            let _configured = load::configure(
                &original,
                std::path::Path::new("/Cargo.toml"),
                false,
                load::Context {
                    features: load::features::Context {
                        allow_features: None,
                        nightly_features_allowed: true,
                        channel: "nightly",
                        is_local: true,
                    },
                    cargo_lints: false,
                    profiles: crate::input::membership::manifest::load::ProfileContext::fixture(),
                },
                &mut session,
            )?;
            assert!(session.is_file(b"/build.rs")?);
            Ok(())
        })();
        let result = session.finish(result);
        if paths_exhausted {
            // Every path check past the exhausted target is recorded,
            // one at a time, and none refuses.
            assert!(result.is_ok());
            let exceeded = graph.exceedances();
            let axis = exceeded
                .axis(node::PATHS_RESOURCE)
                .expect("exhausted path target recorded");
            assert_eq!(
                axis.first_requested,
                node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1
            );
            assert_eq!(axis.high_water, graph.census_usage().unwrap().1 as u64);
        } else {
            assert!(!graph.exceedances().is_exceeded());
        }
        let success = result.is_ok();
        if success {
            host.finish().unwrap();
        } else {
            drop(host);
        }
        assert!(allocation.check().is_ok(), "accounting targets never latch");
        (success, allocation.usage(), allocation.exceedances())
    };
    let (success, usage, _) = run(usize::MAX, usize::MAX, false);
    assert!(success);
    assert_eq!(
        run(usage.items, usage.bytes, false),
        (true, usage, crate::exceedance::Record::new())
    );
    // One short of an aggregate target admits and records that axis.
    admits_one_short(
        run(usage.items - 1, usage.bytes, false),
        usage,
        input::INPUT_ITEMS_RESOURCE,
        usage.items,
    );
    admits_one_short(
        run(usage.items, usage.bytes - 1, false),
        usage,
        input::INPUT_BYTES_RESOURCE,
        usage.bytes,
    );
    assert_eq!(
        run(usage.items, usage.bytes, true),
        (true, usage, crate::exceedance::Record::new())
    );
}

#[test]
fn absent_document_is_data_but_eof_parse_failures_are_terminal() {
    for value in [
        observed::Value::Absent,
        observed::Value::File {
            bytes: vec![0xff],
            end: observed::End::Eof,
        },
        observed::Value::File {
            bytes: b"invalid={".to_vec(),
            end: observed::End::Eof,
        },
    ] {
        let absent = matches!(value, observed::Value::Absent);
        let encoding = matches!(&value, observed::Value::File { bytes, .. } if bytes == &[0xff]);
        let image = image(&[(b"/Cargo.toml", value, true)]);
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
        .unwrap();
        let mut session = Session::new(&mut host);
        let result = session.document(b"/Cargo.toml", &limits);
        if absent {
            assert!(matches!(result, Ok(None)));
            session.items(1).unwrap();
            session.finish(Ok(())).unwrap();
        } else {
            assert!(matches!(result, Err(Stop::Refused)));
            assert!(matches!(
                session.exists(b"/not-observed"),
                Err(Stop::Refused)
            ));
            let result = session.finish(Ok(()));
            if encoding {
                assert!(matches!(
                    result,
                    Err(Error::Host(InputError::ManifestEncoding(_)))
                ));
            } else {
                assert!(matches!(result, Err(Error::Host(InputError::Document(_)))));
            }
        }
        host.finish().unwrap();
    }
}
