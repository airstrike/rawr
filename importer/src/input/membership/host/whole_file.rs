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
    .unwrap()
}

fn record(path_bytes: &[u8], value: observed::Value) -> observed::Record {
    observed::Record {
        query: observed::Query::File(path(path_bytes)),
        value,
    }
}

fn content(bytes: &[u8], end: observed::End) -> observed::Value {
    observed::Value::File {
        bytes: bytes.to_vec(),
        end,
    }
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
    .unwrap()
}

#[test]
fn whole_file_borrows_raw_eof_bytes_and_preserves_exact_native_query() {
    let image = image(&[record(
        b"/p/\xff",
        content(&[0xff, 0, b'x'], observed::End::Eof),
    )]);
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
    .unwrap();
    assert!(matches!(host.file(b"/p/./\xff"), Err(Error::Unobserved)));
    assert!(matches!(host.file(b"/p/\xfe"), Err(Error::Unobserved)));
    super::super::OWNED_BYTES.with(|count| count.set(0));
    super::super::OWNED_VECTORS.with(|count| count.set(0));
    let before = host.budget().allocation.usage();
    let file = host.file(b"/p/\xff").unwrap().unwrap();
    let observed::Value::File { bytes, .. } = &image.records()[0].value else {
        panic!("whole-file fixture");
    };
    assert!(std::ptr::eq(file.bytes(), bytes.as_slice()));
    assert_eq!(file.bytes(), &[0xff, 0, b'x']);
    assert!(std::str::from_utf8(file.bytes()).is_err());
    let after = host.budget().allocation.usage();
    assert_eq!(after.items - before.items, 1);
    assert_eq!(after.bytes - before.bytes, 4);
    assert_eq!(super::super::OWNED_BYTES.with(std::cell::Cell::get), 1);
    assert_eq!(super::super::OWNED_VECTORS.with(std::cell::Cell::get), 0);
    host.finish().unwrap();
}

#[test]
fn whole_file_io_errors_precede_utf8_or_document_consumption() {
    let image = image(&[
        record(b"/absent", observed::Value::Absent),
        record(b"/before", observed::Value::Error(13)),
        record(b"/invalid", content(&[0xff], observed::End::Error(5))),
        record(b"/prefix", content(b"a = 1\n", observed::End::Error(5))),
        record(b"/zero", content(b"", observed::End::Eof)),
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
    .unwrap();
    assert!(host.file(b"/absent").unwrap().is_none());
    let consumed = std::cell::Cell::new(0);
    for (path, code) in [
        (b"/before".as_slice(), 13),
        (b"/invalid", 5),
        (b"/prefix", 5),
    ] {
        let result = host.file(path).map(|file| {
            consumed.set(consumed.get() + 1);
            let file = file.unwrap();
            std::str::from_utf8(file.bytes()).expect("consumer sees only complete UTF-8 fixture");
        });
        assert!(matches!(result, Err(Error::Io(actual)) if actual == code));
    }
    assert_eq!(consumed.get(), 0);
    assert!(host.file(b"/zero").unwrap().unwrap().bytes().is_empty());
    host.finish().unwrap();
}

#[test]
fn whole_file_and_range_observations_cannot_upgrade_each_other() {
    let image = image(&[
        observed::Record {
            query: observed::Query::Read {
                path: path(b"/range"),
                offset: 0,
                length: 6,
            },
            value: observed::Value::Bytes(b"a = 1\n".to_vec()),
        },
        observed::Record {
            query: observed::Query::Read {
                path: path(b"/zero"),
                offset: 0,
                length: 0,
            },
            value: observed::Value::Bytes(Vec::new()),
        },
        record(b"/whole", content(b"a = 1\n", observed::End::Eof)),
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
    .unwrap();
    assert!(matches!(host.file(b"/range"), Err(Error::Unobserved)));
    assert!(matches!(host.file(b"/zero"), Err(Error::Unobserved)));
    assert!(matches!(host.read(b"/whole", 0, 6), Err(Error::Unobserved)));
    assert_eq!(
        host.read(b"/range", 0, 6).unwrap(),
        Some(b"a = 1\n".as_slice())
    );
    assert_eq!(host.read(b"/zero", 0, 0).unwrap(), Some(b"".as_slice()));
    assert_eq!(host.file(b"/whole").unwrap().unwrap().bytes(), b"a = 1\n");
    host.finish().unwrap();
}

#[test]
fn whole_file_query_uses_original_ownership_with_exact_and_floor_limits() {
    let image = image(&[record(b"/raw", content(b"content", observed::End::Eof))]);
    for floor in 0..3 {
        let graph = node::GraphBudget::new();
        graph.paths(node::MAX_PATH_CHECKS_PER_GRAPH - 2).unwrap();
        let mut allocation = input::Allocation::with_limits(
            10 - usize::from(floor == 1),
            19 + std::mem::size_of::<bool>() + 8 - usize::from(floor == 2),
            4,
        );
        allocation.items(7).unwrap();
        allocation.bytes(19).unwrap();
        let mut host = Host::from_image(
            &image,
            Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
        )
        .unwrap();
        host.file(b"/raw").unwrap();
        super::super::OWNED_BYTES.with(|count| count.set(0));
        // The repeated query owns its path and spends its lookup on every
        // floor; crossing an ownership target is recorded, never refused.
        let result = host.file(b"/raw");
        assert_eq!(result.unwrap().unwrap().bytes(), b"content");
        let total_bytes = 19 + std::mem::size_of::<bool>() + 8;
        assert_eq!(host.budget().allocation.usage().items, 10);
        assert_eq!(host.budget().allocation.usage().bytes, total_bytes);
        assert_eq!(super::super::OWNED_BYTES.with(std::cell::Cell::get), 1);
        let (_, paths, _) = graph.census_usage().unwrap();
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
        host.finish().unwrap();
    }
}

#[test]
fn whole_file_repeated_outcomes_each_spend_original_graph_work() {
    for value in [
        observed::Value::Absent,
        observed::Value::Error(13),
        content(b"partial", observed::End::Error(5)),
        content(b"whole", observed::End::Eof),
    ] {
        let image = image(&[record(b"/raw", value)]);
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        graph.paths(node::MAX_PATH_CHECKS_PER_GRAPH - 2).unwrap();
        let mut allocation = input::Allocation::new(&limits);
        let mut host = Host::from_image(
            &image,
            Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
        )
        .unwrap();
        for _ in 0..2 {
            assert!(!matches!(host.file(b"/raw"), Err(Error::Budget(_))));
        }
        assert!(!graph.exceedances().is_exceeded());
        // The third repeat crosses the path-check target; the outcome is
        // still answered from the record and the graph records the exceedance.
        assert!(!matches!(host.file(b"/raw"), Err(Error::Budget(_))));
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
        host.finish().unwrap();
    }
}

#[test]
fn whole_file_invalid_or_refused_query_cannot_consume_the_image() {
    let image = image(&[record(b"/raw", content(b"whole", observed::End::Eof))]);
    let graph = node::GraphBudget::new();
    graph.paths(node::MAX_PATH_CHECKS_PER_GRAPH - 2).unwrap();
    let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, 4);
    let mut host = Host::from_image(
        &image,
        Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    )
    .unwrap();
    let before = host.budget().allocation.usage();
    super::super::OWNED_BYTES.with(|count| count.set(0));
    for invalid in [b"rel".as_slice(), b"/p\0"] {
        assert!(matches!(host.file(invalid), Err(Error::Encoding(_))));
    }
    assert_eq!(host.budget().allocation.usage(), before);
    assert_eq!(super::super::OWNED_BYTES.with(std::cell::Cell::get), 0);
    // A path past the string target is recorded, then owned and looked
    // up like any other query; it is simply missing from the image.
    assert!(matches!(host.file(b"/long"), Err(Error::Unobserved)));
    let after = host.budget().allocation.usage();
    assert_eq!(
        (after.items - before.items, after.bytes - before.bytes),
        (1, 5)
    );
    assert_eq!(super::super::OWNED_BYTES.with(std::cell::Cell::get), 1);
    let axis = host
        .budget()
        .allocation
        .exceedances()
        .axis("membership native path bytes")
        .copied()
        .expect("string axis");
    assert_eq!((axis.target, axis.high_water), (4, 5));
    let (_, paths, _) = graph.census_usage().unwrap();
    assert_eq!(paths, node::MAX_PATH_CHECKS_PER_GRAPH - 1);
    // A genuinely latched account refuses before ownership and lookup.
    host.budget().allocation.refuse();
    assert!(matches!(
        host.file(b"/new"),
        Err(Error::Input(crate::Error::InputAccountRefused))
    ));
    assert!(matches!(
        host.file(b"/raw"),
        Err(Error::Input(crate::Error::InputAccountRefused))
    ));
    assert_eq!(host.budget().allocation.usage(), after);
    assert_eq!(super::super::OWNED_BYTES.with(std::cell::Cell::get), 1);
    graph
        .paths(1)
        .expect("refused paths never reached graph lookup");
    assert!(!graph.exceedances().is_exceeded());
    assert!(matches!(host.finish(), Err(Error::Unconsumed)));
}
