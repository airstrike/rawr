use super::{observed, Budget, Error, Stored};
use crate::{cargo, input, node};

fn with_budget<T>(run: impl FnOnce(&mut Budget<'_>) -> T) -> T {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    run(&mut Budget {
        graph: &graph,
        allocation: &mut allocation,
    })
}

fn record(value: std::result::Result<&[u8], i32>) -> observed::Record {
    with_budget(|budget| observed::Record {
        query: observed::Query::Canonical(
            observed::Path::from_bytes(b"/p/\xff", budget).expect("input"),
        ),
        value: observed::Value::Canonical(match value {
            Ok(bytes) => Ok(observed::Path::from_bytes(bytes, budget).expect("output")),
            Err(code) => Err(code),
        }),
    })
}

fn encode(record: &observed::Record) -> Vec<u8> {
    with_budget(|budget| {
        Stored::from_records(std::slice::from_ref(record), budget)
            .expect("checked")
            .encode(budget)
            .expect("encode")
    })
}

#[test]
fn canonical_codec_retains_native_paths_and_positive_error_without_absence() {
    for value in [
        Ok(b"/r/\xfe".as_slice()),
        Ok(b"/"),
        Err(1),
        Err(nix::libc::EINTR),
        Err(107),
        Err(i32::MAX),
    ] {
        let record = record(value);
        let bytes = encode(&record);
        with_budget(|budget| {
            let image = Stored::decode(&bytes, budget).expect("decode");
            assert_eq!(image.records(), std::slice::from_ref(&record));
            assert_eq!(image.encode(budget).expect("reencode"), bytes);
        });
    }
    for value in [
        observed::Value::Absent,
        observed::Value::Error(13),
        observed::Value::Link(b"/output".to_vec()),
    ] {
        let mut bad = record(Err(13));
        bad.value = value;
        with_budget(|budget| {
            assert!(matches!(
                Stored::from_records(std::slice::from_ref(&bad), budget),
                Err(Error::ObservationType)
            ))
        });
    }
    let mut bad = record(Ok(b"/output"));
    bad.query = with_budget(|budget| {
        observed::Query::Link(observed::Path::from_bytes(b"/input", budget).expect("input"))
    });
    assert!(matches!(bad.checked(), Err(Error::ObservationType)));
}

#[test]
fn canonical_wire_checks_output_domain_errno_tags_and_truncation() {
    let bytes = encode(&record(Ok(b"/r/\xfe")));
    let query = super::MAGIC.len() + super::CARGO_COMMIT.len() + 8;
    let input = query + 1 + 8;
    let value = input + 4;
    let output = value + 1 + 8;
    assert_eq!(bytes[query], 10); // Appended after previous reader's tags 0..=9.
    assert_eq!(bytes[value], 13); // Previous value dispatch ended at 12.
    for (offset, byte) in [
        (query, 11),
        (value, 15),
        (input, b'r'),
        (input + 1, 0),
        (output, b'r'),
        (output + 1, 0),
    ] {
        let mut bad = bytes.clone();
        bad[offset] = byte;
        with_budget(|budget| assert!(Stored::decode(&bad, budget).is_err()));
    }
    for length in 0..bytes.len() {
        with_budget(|budget| assert!(Stored::decode(&bytes[..length], budget).is_err()));
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    with_budget(|budget| assert!(Stored::decode(&trailing, budget).is_err()));
    let mut old = bytes;
    old[super::MAGIC.len() - 1] = 1;
    with_budget(|budget| assert!(Stored::decode(&old, budget).is_err()));
    for code in [0_i32, -1] {
        assert!(record(Err(code)).checked().is_err());
        let mut bad = encode(&record(Err(13)));
        assert_eq!(bad[value], 14);
        bad[value + 1..].copy_from_slice(&code.to_le_bytes());
        with_budget(|budget| {
            assert!(matches!(
                Stored::decode(&bad, budget),
                Err(Error::Encoding(
                    "canonicalization native error must be positive"
                ))
            ))
        });
    }
    let previous = with_budget(|budget| observed::Record {
        query: observed::Query::Discovery {
            directory: observed::Path::from_bytes(b"/p", budget).expect("directory"),
            filename: super::Filename::Config,
            warn: false,
        },
        value: observed::Value::Discovery(
            super::Transcript::checked(
                super::Filename::Config,
                false,
                super::Attempt::Acquired(super::Identity {
                    device: 1,
                    inode: 2,
                }),
                super::After::None,
            )
            .expect("legacy"),
        ),
    });
    assert_eq!(encode(&previous)[query], 9);
    assert!(previous.query < record(Err(13)).query);
}

#[test]
fn canonical_retention_and_decode_admit_actual_input_and_output_ownership() {
    for value in [Ok(b"/r/\xfe".as_slice()), Err(13)] {
        let record = record(value);
        let bytes = encode(&record);
        let output = usize::from(value.is_ok());
        let items = 7 + 2 + output;
        let owned = 19 + std::mem::size_of::<observed::Record>() + 4 + output * 4;
        for decode in [false, true] {
            for floor in 0..3 {
                let graph = node::GraphBudget::new();
                let mut allocation = input::Allocation::with_limits(
                    items - usize::from(floor == 1),
                    owned - usize::from(floor == 2),
                    4,
                );
                allocation.items(7).expect("prior items");
                allocation.bytes(19).expect("prior bytes");
                let mut budget = Budget {
                    graph: &graph,
                    allocation: &mut allocation,
                };
                let result = if decode {
                    Stored::decode(&bytes, &mut budget)
                } else {
                    Stored::from_records(std::slice::from_ref(&record), &mut budget)
                };
                // Every floor retains the record in full; one less on an
                // axis records that axis with the exact total as high water.
                assert_eq!(
                    result.expect("ownership targets never refuse").records(),
                    std::slice::from_ref(&record)
                );
                assert_eq!(allocation.usage().items, items);
                assert_eq!(allocation.usage().bytes, owned);
                let exceeded = allocation.exceedances();
                match floor {
                    0 => assert!(!exceeded.is_exceeded()),
                    1 => {
                        let axis = exceeded.axis(input::INPUT_ITEMS_RESOURCE).unwrap();
                        assert_eq!(
                            (axis.target, axis.high_water),
                            (items as u64 - 1, items as u64)
                        );
                        assert!(exceeded.axis(input::INPUT_BYTES_RESOURCE).is_none());
                    }
                    _ => {
                        let axis = exceeded.axis(input::INPUT_BYTES_RESOURCE).unwrap();
                        assert_eq!(
                            (axis.target, axis.high_water),
                            (owned as u64 - 1, owned as u64)
                        );
                        assert!(exceeded.axis(input::INPUT_ITEMS_RESOURCE).is_none());
                    }
                }
                assert!(allocation.items(0).is_ok(), "nothing latched");
            }
        }
        // Decoding one byte past the graph's decode target still decodes and
        // records the axis; only checked-arithmetic overflow refuses, before
        // any ownership.
        let graph = node::GraphBudget::new();
        graph
            .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize - bytes.len() + 1)
            .expect("prior decode");
        let limits = cargo::Limits::standard();
        let mut allocation = input::Allocation::new(&limits);
        Stored::decode(
            &bytes,
            &mut Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
        )
        .expect("decode past the graph target");
        let axis = graph
            .exceedances()
            .axis(node::DECODED_RESOURCE)
            .copied()
            .unwrap();
        assert_eq!(axis.target, node::MAX_DECODED_BYTES_PER_GRAPH);
        assert_eq!(axis.first_requested, node::MAX_DECODED_BYTES_PER_GRAPH + 1);
        assert_eq!(axis.high_water, node::MAX_DECODED_BYTES_PER_GRAPH + 1);
        assert_eq!(allocation.usage().items, items - 7);
        assert_eq!(allocation.usage().bytes, owned - 19);
        let graph = node::GraphBudget::new();
        node::DECODE_SITES.with(|sites| sites.borrow_mut().clear());
        graph
            .decode(usize::MAX)
            .expect("saturate the decode account");
        let mut allocation = input::Allocation::new(&limits);
        let before = allocation.usage();
        assert!(matches!(
            Stored::decode(
                &bytes,
                &mut Budget {
                    graph: &graph,
                    allocation: &mut allocation
                }
            ),
            Err(Error::Budget(node::Error::Allocation))
        ));
        assert_eq!(allocation.usage(), before);
    }
}
