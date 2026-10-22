use super::{observed, stored, Budget, Error, Host};
use crate::{cargo, input, node};
use observed::discovery::{
    After, Attempt, Exists, Filename, Identity, NativeError, Selection, Transcript,
};

fn with_budget<T>(run: impl FnOnce(Budget<'_>) -> T) -> T {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    run(Budget {
        graph: &graph,
        allocation: &mut allocation,
    })
}

fn image(value: Transcript) -> stored::Stored {
    with_budget(|mut budget| {
        let record = observed::Record {
            query: observed::Query::Discovery {
                directory: observed::Path::from_bytes(b"/p/\xff", &mut budget).expect("directory"),
                filename: value.filename(),
                warn: value.warn(),
            },
            value: observed::Value::Discovery(value),
        };
        stored::Stored::from_records(&[record], &mut budget).expect("checked discovery image")
    })
}

fn no_selection() -> Transcript {
    Transcript::checked(
        Filename::Config,
        true,
        Attempt::OpenFailed(NativeError::Os(13)),
        After::Fallback(Exists::Failed(NativeError::Os(2))),
    )
    .expect("negative acquisition")
}

#[test]
fn discovery_requires_exact_native_directory_family_and_warning_mode() {
    let expected = no_selection();
    let image = image(expected);
    with_budget(|budget| {
        let mut host = Host::from_image(&image, budget).expect("host");
        for (directory, filename, warn) in [
            (b"/p/./\xff".as_slice(), Filename::Config, true),
            (b"/p/\xfe", Filename::Config, true),
            (b"/p/\xff", Filename::Credentials, true),
            (b"/p/\xff", Filename::Config, false),
        ] {
            assert!(matches!(
                host.discovery(directory, filename, warn),
                Err(Error::Unobserved)
            ));
        }
        let before = host.budget().allocation.usage();
        let actual = host
            .discovery(b"/p/\xff", Filename::Config, true)
            .expect("recorded acquisition");
        assert_eq!(actual, expected);
        assert_eq!(actual.selection(), Selection::None);
        let after = host.budget().allocation.usage();
        assert_eq!(after.items - before.items, 1);
        assert_eq!(after.bytes - before.bytes, 4);
        host.finish().expect("all records consumed");
    });
}

#[test]
fn discovery_repeated_queries_preserve_original_ownership_and_graph_floors() {
    let image = image(no_selection());
    for floor in 0..3 {
        let graph = node::GraphBudget::new();
        graph
            .paths(node::MAX_PATH_CHECKS_PER_GRAPH - 2)
            .expect("prior work");
        let mut allocation = input::Allocation::with_limits(
            10 - usize::from(floor == 1),
            19 + std::mem::size_of::<bool>() + 8 - usize::from(floor == 2),
            4,
        );
        allocation.items(7).expect("prior items");
        allocation.bytes(19).expect("prior bytes");
        let mut host = Host::from_image(
            &image,
            Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
        )
        .expect("host");
        host.discovery(b"/p/\xff", Filename::Config, true)
            .expect("first query");
        // The repeated query owns its path and spends its lookup on every
        // floor; crossing an ownership target is recorded, never refused.
        let result = host.discovery(b"/p/\xff", Filename::Config, true);
        assert_eq!(result.expect("exact account"), no_selection());
        let total_bytes = 19 + std::mem::size_of::<bool>() + 8;
        assert_eq!(host.budget().allocation.usage().items, 10);
        assert_eq!(host.budget().allocation.usage().bytes, total_bytes);
        let (_, paths, _) = graph.census_usage().expect("census");
        assert_eq!(paths, node::MAX_PATH_CHECKS_PER_GRAPH);
        assert!(!graph.exceedances().is_exceeded());
        let owned = host.budget().allocation.exceedances();
        match floor {
            0 => {
                assert!(!owned.is_exceeded());
                graph.paths(1).expect("past the target is recorded");
                assert_eq!(
                    graph
                        .exceedances()
                        .axis(node::PATHS_RESOURCE)
                        .unwrap()
                        .high_water,
                    node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1
                );
            }
            1 => {
                let axis = owned.axis(input::INPUT_ITEMS_RESOURCE).unwrap();
                assert_eq!((axis.target, axis.high_water), (9, 10));
                assert!(owned.axis(input::INPUT_BYTES_RESOURCE).is_none());
            }
            _ => {
                let axis = owned.axis(input::INPUT_BYTES_RESOURCE).unwrap();
                assert_eq!(
                    (axis.target, axis.high_water),
                    (total_bytes as u64 - 1, total_bytes as u64)
                );
                assert!(owned.axis(input::INPUT_ITEMS_RESOURCE).is_none());
            }
        }
        assert!(host.budget().allocation.items(0).is_ok(), "nothing latched");
        host.finish().expect("first query consumed record");
    }
    for transcript in [
        no_selection(),
        Transcript::checked(
            Filename::Config,
            true,
            Attempt::Acquired(Identity {
                device: 1,
                inode: 2,
            }),
            After::Second(Attempt::StatFailed(nix::libc::EINTR)),
        )
        .expect("native fstat failure"),
    ] {
        let image = self::image(transcript);
        let graph = node::GraphBudget::new();
        graph
            .paths(node::MAX_PATH_CHECKS_PER_GRAPH - 2)
            .expect("prior work");
        let limits = cargo::Limits::standard();
        let mut allocation = input::Allocation::new(&limits);
        let mut host = Host::from_image(
            &image,
            Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
        )
        .expect("host");
        for _ in 0..2 {
            assert_eq!(
                host.discovery(b"/p/\xff", Filename::Config, true)
                    .expect("query work"),
                transcript
            );
        }
        assert!(!graph.exceedances().is_exceeded());
        // The third repeat crosses the graph's path-check target: it is still
        // answered and the graph records the exceedance.
        assert_eq!(
            host.discovery(b"/p/\xff", Filename::Config, true)
                .expect("query past the target"),
            transcript
        );
        let axis = graph
            .exceedances()
            .axis(node::PATHS_RESOURCE)
            .copied()
            .unwrap();
        assert_eq!(axis.target, node::MAX_PATH_CHECKS_PER_GRAPH as u64);
        assert_eq!(
            axis.first_requested,
            node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1
        );
        assert_eq!(axis.high_water, node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1);
        host.finish().expect("record consumed");
    }
}

#[test]
fn discovery_invalid_missing_and_refused_queries_do_not_consume_record() {
    let image = image(no_selection());
    let graph = node::GraphBudget::new();
    graph
        .paths(node::MAX_PATH_CHECKS_PER_GRAPH - 2)
        .expect("prior work");
    let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, 4);
    let mut host = Host::from_image(
        &image,
        Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    )
    .expect("host");
    let before = host.budget().allocation.usage();
    for invalid in [b"rel".as_slice(), b"/p\0"] {
        assert!(matches!(
            host.discovery(invalid, Filename::Config, true),
            Err(Error::Encoding(_))
        ));
    }
    assert_eq!(host.budget().allocation.usage(), before);
    // A path past the string target is recorded, then owned and looked
    // up like any other query; it is simply missing from the image.
    assert!(matches!(
        host.discovery(b"/long", Filename::Config, true),
        Err(Error::Unobserved)
    ));
    let after = host.budget().allocation.usage();
    assert_eq!(
        (after.items - before.items, after.bytes - before.bytes),
        (1, 5)
    );
    let axis = host
        .budget()
        .allocation
        .exceedances()
        .axis("membership native path bytes")
        .copied()
        .expect("string axis");
    assert_eq!((axis.target, axis.high_water), (4, 5));
    let (_, paths, _) = graph.census_usage().expect("census");
    assert_eq!(paths, node::MAX_PATH_CHECKS_PER_GRAPH - 1);
    // A genuinely latched account refuses before ownership and lookup.
    host.budget().allocation.refuse();
    assert!(matches!(
        host.discovery(b"/new", Filename::Config, true),
        Err(Error::Input(crate::Error::InputAccountRefused))
    ));
    assert!(matches!(
        host.discovery(b"/p/\xff", Filename::Config, true),
        Err(Error::Input(crate::Error::InputAccountRefused))
    ));
    assert_eq!(host.budget().allocation.usage(), after);
    graph
        .paths(1)
        .expect("refused paths never reached graph lookup");
    assert!(!graph.exceedances().is_exceeded());
    assert!(matches!(host.finish(), Err(Error::Unconsumed)));
}
