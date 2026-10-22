use super::{observed, stored, Budget, Error, Host};
use crate::{cargo, input, node};

fn path(bytes: &[u8]) -> observed::Path {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    observed::Path::from_bytes(
        bytes,
        &mut Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    )
    .expect("native fixture path")
}

fn image(records: &[observed::Record]) -> stored::Stored {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    stored::Stored::from_records(
        records,
        &mut Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    )
    .expect("canonical fixture image")
}

fn native_image() -> stored::Stored {
    image(&[
        observed::Record {
            query: observed::Query::Read {
                path: path(b"/p/\xff"),
                offset: 4,
                length: 3,
            },
            value: observed::Value::Bytes(vec![0, 0xff, b'x']),
        },
        observed::Record {
            query: observed::Query::Link(path(b"/p/\xff")),
            value: observed::Value::Link(b"../\xfe/./dest".to_vec()),
        },
        observed::Record {
            query: observed::Query::Directory(path(b"/p/\xff")),
            value: observed::Value::Directory {
                entries: vec![
                    observed::Entry {
                        name: b"a".to_vec(),
                        kind: observed::Kind::Symlink,
                        inode: 7,
                    },
                    observed::Entry {
                        name: vec![0xff],
                        kind: observed::Kind::Unknown,
                        inode: 9,
                    },
                ],
                end: observed::End::Eof,
            },
        },
    ])
}

#[test]
fn helpers_borrow_exact_native_payloads_without_owning_output() {
    let image = native_image();
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut host = Host::from_image(
        &image,
        Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    )
    .expect("recorded host");
    super::super::OWNED_VECTORS.with(|count| count.set(0));
    super::super::OWNED_BYTES.with(|count| count.set(0));
    let before = host.budget().allocation.usage();
    let bytes = host.read(b"/p/\xff", 4, 3).expect("read").expect("bytes");
    let target = host.link(b"/p/\xff").expect("link").expect("target");
    let (entries, end) = host
        .directory(b"/p/\xff")
        .expect("directory")
        .expect("frontier");
    match &image.records()[0].value {
        observed::Value::Bytes(stored) => assert!(std::ptr::eq(bytes, stored.as_slice())),
        _ => panic!("fixture read"),
    }
    match &image.records()[1].value {
        observed::Value::Link(stored) => assert!(std::ptr::eq(target, stored.as_slice())),
        _ => panic!("fixture link"),
    }
    match &image.records()[2].value {
        observed::Value::Directory {
            entries: stored, ..
        } => {
            assert!(std::ptr::eq(entries, stored.as_slice()));
        }
        _ => panic!("fixture directory"),
    }
    assert_eq!(bytes, &[0, 0xff, b'x']);
    assert_eq!(target, b"../\xfe/./dest");
    assert_eq!(entries[0].kind, observed::Kind::Symlink);
    assert_eq!(entries[1].name.as_slice(), &[0xff]);
    assert_eq!(entries[1].kind, observed::Kind::Unknown);
    assert_eq!(entries[1].inode, 9);
    assert_eq!(end, observed::End::Eof);
    let after = host.budget().allocation.usage();
    assert_eq!(after.items - before.items, 3);
    assert_eq!(after.bytes - before.bytes, 3 * b"/p/\xff".len());
    assert_eq!(super::super::OWNED_VECTORS.with(std::cell::Cell::get), 0);
    assert_eq!(super::super::OWNED_BYTES.with(std::cell::Cell::get), 3);
    assert_eq!(
        host.finish()
            .expect("all helpers consumed their query")
            .image(),
        &image
    );
}

#[test]
fn helpers_preserve_absence_immediate_error_and_terminal_frontier_states() {
    let image = image(&[
        observed::Record {
            query: observed::Query::Read {
                path: path(b"/absent"),
                offset: 0,
                length: 1,
            },
            value: observed::Value::Absent,
        },
        observed::Record {
            query: observed::Query::Read {
                path: path(b"/error"),
                offset: 0,
                length: 1,
            },
            value: observed::Value::Error(13),
        },
        observed::Record {
            query: observed::Query::Read {
                path: path(b"/zero"),
                offset: u64::MAX,
                length: 0,
            },
            value: observed::Value::Bytes(Vec::new()),
        },
        observed::Record {
            query: observed::Query::Link(path(b"/absent")),
            value: observed::Value::Absent,
        },
        observed::Record {
            query: observed::Query::Link(path(b"/error")),
            value: observed::Value::Error(22),
        },
        observed::Record {
            query: observed::Query::Directory(path(b"/absent")),
            value: observed::Value::Absent,
        },
        observed::Record {
            query: observed::Query::Directory(path(b"/empty")),
            value: observed::Value::Directory {
                entries: Vec::new(),
                end: observed::End::Eof,
            },
        },
        observed::Record {
            query: observed::Query::Directory(path(b"/empty-error")),
            value: observed::Value::Directory {
                entries: Vec::new(),
                end: observed::End::Error(5),
            },
        },
        observed::Record {
            query: observed::Query::Directory(path(b"/error")),
            value: observed::Value::Error(20),
        },
        observed::Record {
            query: observed::Query::Directory(path(b"/partial")),
            value: observed::Value::Directory {
                entries: vec![observed::Entry {
                    name: b"kept".to_vec(),
                    kind: observed::Kind::File,
                    inode: 2,
                }],
                end: observed::End::Error(5),
            },
        },
    ]);
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut host = Host::from_image(
        &image,
        Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    )
    .expect("recorded host");
    assert_eq!(host.read(b"/absent", 0, 1).expect("absence"), None);
    assert!(matches!(host.read(b"/error", 0, 1), Err(Error::Io(13))));
    assert_eq!(
        host.read(b"/zero", u64::MAX, 0).expect("zero range"),
        Some(b"".as_slice())
    );
    assert_eq!(host.link(b"/absent").expect("absence"), None);
    assert!(matches!(host.link(b"/error"), Err(Error::Io(22))));
    assert_eq!(host.directory(b"/absent").expect("absence"), None);
    let (empty, end) = host
        .directory(b"/empty")
        .expect("empty EOF")
        .expect("present");
    assert!(empty.is_empty());
    assert_eq!(end, observed::End::Eof);
    let (empty, end) = host
        .directory(b"/empty-error")
        .expect("error frontier")
        .expect("present");
    assert!(empty.is_empty());
    assert_eq!(end, observed::End::Error(5));
    assert!(matches!(host.directory(b"/error"), Err(Error::Io(20))));
    let (entries, end) = host
        .directory(b"/partial")
        .expect("partial frontier")
        .expect("present");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name.as_slice(), b"kept");
    assert_eq!(end, observed::End::Error(5));
    host.finish()
        .expect("negative and error answers were consumed");
}

#[test]
fn omitted_ranges_paths_and_other_query_kinds_refuse_without_normalization() {
    let image = image(&[observed::Record {
        query: observed::Query::Read {
            path: path(b"/file"),
            offset: 7,
            length: 1,
        },
        value: observed::Value::Bytes(vec![b'x']),
    }]);
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut host = Host::from_image(
        &image,
        Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    )
    .expect("recorded host");
    assert!(matches!(host.read(b"/file", 8, 1), Err(Error::Unobserved)));
    assert!(matches!(host.read(b"/file", 7, 0), Err(Error::Unobserved)));
    assert!(matches!(
        host.read(b"/./file", 7, 1),
        Err(Error::Unobserved)
    ));
    assert!(matches!(
        host.read(b"/missing", 7, 1),
        Err(Error::Unobserved)
    ));
    assert!(matches!(host.link(b"/file"), Err(Error::Unobserved)));
    assert!(matches!(host.directory(b"/file"), Err(Error::Unobserved)));
    assert!(matches!(host.finish(), Err(Error::Unconsumed)));
}

#[test]
fn range_and_native_path_rejections_precede_ownership_and_lookup() {
    let image = image(&[]);
    let graph = node::GraphBudget::new();
    graph
        .paths(node::MAX_PATH_CHECKS_PER_GRAPH - 4)
        .expect("four path checks remain");
    let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, 4);
    let mut host = Host::from_image(
        &image,
        Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    )
    .expect("empty host");
    super::super::OWNED_BYTES.with(|count| count.set(0));
    let before = host.budget().allocation.usage();
    assert!(matches!(
        host.read(b"/p", u64::MAX, 1),
        Err(Error::Encoding("read range overflow"))
    ));
    assert!(matches!(host.read(b"rel", 0, 1), Err(Error::Encoding(_))));
    assert!(matches!(host.link(b"/p\0"), Err(Error::Encoding(_))));
    assert!(matches!(host.directory(b"rel"), Err(Error::Encoding(_))));
    assert_eq!(host.budget().allocation.usage(), before);
    assert_eq!(super::super::OWNED_BYTES.with(std::cell::Cell::get), 0);
    // A path past the string target is recorded, then owned and looked
    // up like any other query; each helper finds it missing from the image.
    assert!(matches!(host.read(b"/long", 0, 1), Err(Error::Unobserved)));
    assert!(matches!(host.link(b"/long"), Err(Error::Unobserved)));
    assert!(matches!(host.directory(b"/long"), Err(Error::Unobserved)));
    let after = host.budget().allocation.usage();
    assert_eq!(
        (after.items - before.items, after.bytes - before.bytes),
        (3, 15)
    );
    assert_eq!(super::super::OWNED_BYTES.with(std::cell::Cell::get), 3);
    let axis = host
        .budget()
        .allocation
        .exceedances()
        .axis("membership native path bytes")
        .copied()
        .expect("string axis");
    assert_eq!(
        (axis.target, axis.first_requested, axis.high_water),
        (4, 5, 5)
    );
    let (_, paths, _) = graph.census_usage().expect("census");
    assert_eq!(paths, node::MAX_PATH_CHECKS_PER_GRAPH - 1);
    // A genuinely latched account refuses before ownership and lookup.
    host.budget().allocation.refuse();
    assert!(matches!(
        host.read(b"/p", 0, 1),
        Err(Error::Input(crate::Error::InputAccountRefused))
    ));
    assert!(matches!(
        host.read(b"/p", 0, 1),
        Err(Error::Input(crate::Error::InputAccountRefused))
    ));
    assert_eq!(host.budget().allocation.usage(), after);
    assert_eq!(super::super::OWNED_BYTES.with(std::cell::Cell::get), 3);
    graph
        .paths(1)
        .expect("refused paths never reached graph lookup");
    assert!(!graph.exceedances().is_exceeded());
    host.finish()
        .expect("empty image has no records to consume");
}

#[test]
fn helper_query_ownership_is_cumulative_with_exact_and_one_over_limits() {
    let image = native_image();
    let exercise = |items, bytes| {
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::with_limits(items, bytes, 4);
        allocation.items(7)?;
        allocation.bytes(19)?;
        let mut host = Host::from_image(
            &image,
            Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
        )?;
        for _ in 0..2 {
            host.read(b"/p/\xff", 4, 3)?;
            host.link(b"/p/\xff")?;
            host.directory(b"/p/\xff")?;
        }
        host.finish()?;
        Ok::<_, Error>((allocation.usage(), allocation.exceedances()))
    };
    let (exact, _) = exercise(usize::MAX, usize::MAX).expect("measure helper ownership");
    assert_eq!(exact.items, 7 + image.records().len() + 6);
    assert_eq!(
        exact.bytes,
        19 + image.records().len() * std::mem::size_of::<bool>() + 6 * 4
    );
    let (usage, exceeded) = exercise(exact.items, exact.bytes).expect("exact bound");
    assert_eq!(usage, exact);
    assert!(!exceeded.is_exceeded());
    // One less on either axis still completes every helper and records
    // that axis with the exact cumulative total as its high water.
    let (usage, exceeded) = exercise(exact.items - 1, exact.bytes).expect("items target");
    assert_eq!(usage, exact);
    let axis = exceeded.axis(input::INPUT_ITEMS_RESOURCE).unwrap();
    assert_eq!(
        (axis.target, axis.high_water),
        (exact.items as u64 - 1, exact.items as u64)
    );
    assert!(exceeded.axis(input::INPUT_BYTES_RESOURCE).is_none());
    let (usage, exceeded) = exercise(exact.items, exact.bytes - 1).expect("bytes target");
    assert_eq!(usage, exact);
    let axis = exceeded.axis(input::INPUT_BYTES_RESOURCE).unwrap();
    assert_eq!(
        (axis.target, axis.high_water),
        (exact.bytes as u64 - 1, exact.bytes as u64)
    );
    assert!(exceeded.axis(input::INPUT_ITEMS_RESOURCE).is_none());
}

#[test]
fn each_helper_latched_account_refuses_before_copy_or_lookup() {
    let image = native_image();
    for helper in 0..3 {
        for item_target in [false, true] {
            let graph = node::GraphBudget::new();
            graph
                .paths(node::MAX_PATH_CHECKS_PER_GRAPH)
                .expect("path work at its target");
            let base_items = 7 + image.records().len();
            let base_bytes = 19 + image.records().len() * std::mem::size_of::<bool>();
            let mut allocation = input::Allocation::with_limits(
                base_items + usize::from(!item_target),
                base_bytes + if item_target { 4 } else { 3 },
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
            .expect("host bitmap admitted");
            // A query crossing an ownership target is still owned and
            // looked up; the crossing axis and the graph's path target record
            // exceedances and nothing latches.
            super::super::OWNED_BYTES.with(|count| count.set(0));
            let result = match helper {
                0 => host.read(b"/p/\xff", 4, 3).map(|_| ()),
                1 => host.link(b"/p/\xff").map(|_| ()),
                _ => host.directory(b"/p/\xff").map(|_| ()),
            };
            result.expect("query past its target is answered");
            assert_eq!(super::super::OWNED_BYTES.with(std::cell::Cell::get), 1);
            assert_eq!(host.budget().allocation.usage().items, base_items + 1);
            assert_eq!(host.budget().allocation.usage().bytes, base_bytes + 4);
            let owned = host.budget().allocation.exceedances();
            let (name, target, high_water) = if item_target {
                (input::INPUT_ITEMS_RESOURCE, base_items, base_items + 1)
            } else {
                (input::INPUT_BYTES_RESOURCE, base_bytes + 3, base_bytes + 4)
            };
            let axis = owned.axis(name).expect("crossing axis");
            assert_eq!(
                (axis.target, axis.high_water),
                (target as u64, high_water as u64)
            );
            assert_eq!(owned.axes().count(), 1);
            let axis = graph
                .exceedances()
                .axis(node::PATHS_RESOURCE)
                .copied()
                .unwrap();
            assert_eq!(
                axis.first_requested,
                node::MAX_PATH_CHECKS_PER_GRAPH as u64 + 1
            );
            // A genuinely latched account refuses before the copy or the lookup.
            host.budget().allocation.refuse();
            super::super::OWNED_BYTES.with(|count| count.set(0));
            let result = match helper {
                0 => host.read(b"/p/\xff", 4, 3).map(|_| ()),
                1 => host.link(b"/p/\xff").map(|_| ()),
                _ => host.directory(b"/p/\xff").map(|_| ()),
            };
            assert!(matches!(
                result,
                Err(Error::Input(crate::Error::InputAccountRefused))
            ));
            assert_eq!(super::super::OWNED_BYTES.with(std::cell::Cell::get), 0);
            assert_eq!(host.budget().allocation.usage().bytes, base_bytes + 4);
            let (_, paths, _) = graph.census_usage().expect("census");
            assert_eq!(paths, node::MAX_PATH_CHECKS_PER_GRAPH + 1);
            assert!(matches!(host.finish(), Err(Error::Unconsumed)));
        }
    }
}

#[test]
fn repeated_helper_requests_charge_graph_work_every_time() {
    let image = native_image();
    let limits = cargo::Limits::standard();
    for helper in 0..3 {
        let graph = node::GraphBudget::new();
        graph
            .paths(node::MAX_PATH_CHECKS_PER_GRAPH - 2)
            .expect("two path checks remain");
        let mut allocation = input::Allocation::new(&limits);
        let mut host = Host::from_image(
            &image,
            Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
        )
        .expect("recorded host");
        for remaining in [true, true, false] {
            let result = match helper {
                0 => host.read(b"/p/\xff", 4, 3).map(|_| ()),
                1 => host.link(b"/p/\xff").map(|_| ()),
                _ => host.directory(b"/p/\xff").map(|_| ()),
            };
            result.expect("every repeat is answered");
            // The third repeat crosses the path-check target, which the
            // graph records instead of refusing.
            if remaining {
                assert!(!graph.exceedances().is_exceeded());
            } else {
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
            }
        }
    }
}
