use super::{
    observed, After, Attempt, Budget, Error, Exists, Filename, Identity, NativeError, Stored,
    Transcript,
};
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

fn transcript(filename: Filename, warn: bool, legacy: Attempt, after: After) -> Transcript {
    Transcript::checked(filename, warn, legacy, after).expect("reachable fixture")
}

fn record(value: Transcript) -> observed::Record {
    observed::Record {
        query: observed::Query::Discovery {
            directory: with_budget(|budget| {
                observed::Path::from_bytes(b"/p/\xff", budget).expect("native directory")
            }),
            filename: value.filename(),
            warn: value.warn(),
        },
        value: observed::Value::Discovery(value),
    }
}

fn encoded(record: &observed::Record) -> Vec<u8> {
    with_budget(|budget| {
        Stored::from_records(std::slice::from_ref(record), budget)
            .expect("checked record")
            .encode(budget)
            .expect("encoded record")
    })
}

fn acquired(inode: u64) -> Attempt {
    Attempt::Acquired(Identity {
        device: u64::MAX,
        inode,
    })
}

#[test]
fn discovery_codec_preserves_each_reachable_acquisition_shape() {
    for filename in [Filename::Config, Filename::Credentials] {
        for warn in [false, true] {
            let mut values = Vec::new();
            if warn {
                for second in [
                    acquired(7),
                    acquired(8),
                    Attempt::OpenFailed(NativeError::Os(13)),
                    Attempt::StatFailed(nix::libc::EINTR),
                ] {
                    values.push(transcript(
                        filename,
                        warn,
                        acquired(7),
                        After::Second(second),
                    ));
                }
            } else {
                values.push(transcript(filename, warn, acquired(7), After::None));
            }
            for legacy in [
                Attempt::OpenFailed(NativeError::Os(13)),
                Attempt::StatFailed(nix::libc::EINTR),
            ] {
                for exists in [
                    Exists::Present,
                    Exists::Failed(NativeError::Os(2)),
                    Exists::Failed(NativeError::Os(nix::libc::EINTR)),
                ] {
                    values.push(transcript(filename, warn, legacy, After::Fallback(exists)));
                }
            }
            for value in values {
                let record = record(value);
                let bytes = encoded(&record);
                with_budget(|budget| {
                    let image = Stored::decode(&bytes, budget).expect("decode discovery");
                    assert_eq!(image.records(), &[record]);
                    assert_eq!(image.encode(budget).expect("reencode"), bytes);
                });
            }
        }
    }
}

#[test]
fn discovery_checker_binds_query_and_rejects_impossible_path_failures() {
    let value = transcript(Filename::Config, false, acquired(7), After::None);
    let mut wrong_filename = record(value);
    let observed::Query::Discovery { filename, .. } = &mut wrong_filename.query else {
        panic!("fixture")
    };
    *filename = Filename::Credentials;
    let mut wrong_warn = record(value);
    let observed::Query::Discovery { warn, .. } = &mut wrong_warn.query else {
        panic!("fixture")
    };
    *warn = true;
    let mut wrong_query = record(value);
    wrong_query.query = observed::Query::Identity;
    let mut absent = record(value);
    absent.value = observed::Value::Absent;
    let mut error = record(value);
    error.value = observed::Value::Error(13);
    let nul = record(transcript(
        Filename::Config,
        false,
        Attempt::OpenFailed(NativeError::InvalidInput),
        After::Fallback(Exists::Failed(NativeError::InvalidInput)),
    ));
    for invalid in [wrong_filename, wrong_warn, wrong_query, absent, error, nul] {
        with_budget(|budget| {
            assert!(matches!(
                Stored::from_records(std::slice::from_ref(&invalid), budget),
                Err(Error::ObservationType)
            ))
        });
        // Deliberately serialize unchecked test data to exercise the decoder's
        // cross-record checker, rather than only rejecting retention.
        with_budget(|budget| {
            let bytes = Stored {
                records: vec![super::retain(&invalid, budget).expect("retain raw test data")],
            }
            .encode(budget)
            .expect("encode raw test data");
            assert!(matches!(
                Stored::decode(&bytes, budget),
                Err(Error::ObservationType)
            ));
        });
        assert!(matches!(invalid.checked(), Err(Error::ObservationType)));
    }
}

#[test]
fn discovery_wire_rejects_bad_tags_flags_branches_errno_and_query_mismatch() {
    let base = encoded(&record(transcript(
        Filename::Config,
        false,
        Attempt::OpenFailed(NativeError::Os(13)),
        After::Fallback(Exists::Present),
    )));
    let query = super::MAGIC.len() + super::CARGO_COMMIT.len() + 8;
    let family = query + 1 + 8 + 4;
    let value = family + 2;
    assert_eq!(base[query], 9); // Appended beyond the previous reader's 0..=8 tags.
    assert_eq!(base[value], 12); // Previous value reader accepts only 0..=11.
    for (index, byte) in [
        (query, 10),
        (family, 2),
        (family + 1, 2),
        (family, 1),
        (family + 1, 1), // individually valid but mismatched query
        (value, 13),
        (value + 1, 2),
        (value + 2, 2),
        (value + 1, 1),
        (value + 2, 1), // individually valid but mismatched value
        (value + 3, 3),
        (value + 4, 2),
        (value + 9, 3),
        (value + 9, 0),
        (value + 10, 2),
    ] {
        let mut bytes = base.clone();
        bytes[index] = byte;
        with_budget(|budget| {
            assert!(
                Stored::decode(&bytes, budget).is_err(),
                "mutation {index}={byte}"
            )
        });
    }
    for errno in [0_i32, -1, nix::libc::EINTR] {
        let mut bytes = base.clone();
        bytes[value + 5..value + 9].copy_from_slice(&errno.to_le_bytes());
        with_budget(|budget| {
            assert!(matches!(
                Stored::decode(&bytes, budget),
                Err(Error::Encoding("invalid discovery transcript"))
            ))
        });
    }
    // Secondary open is subject to the same EINTR exclusion; fstat/fallback
    // EINTR are exercised as accepted round trips in the shape test.
    let mut second = encoded(&record(transcript(
        Filename::Config,
        true,
        acquired(7),
        After::Second(Attempt::OpenFailed(NativeError::Os(13))),
    )));
    let end = second.len();
    second[end - 4..].copy_from_slice(&nix::libc::EINTR.to_le_bytes());
    with_budget(|budget| assert!(Stored::decode(&second, budget).is_err()));
    for value in [
        transcript(
            Filename::Config,
            true,
            acquired(7),
            After::Second(Attempt::StatFailed(5)),
        ),
        transcript(
            Filename::Config,
            false,
            Attempt::StatFailed(5),
            After::Fallback(Exists::Failed(NativeError::Os(2))),
        ),
    ] {
        for errno in [0_i32, -1] {
            let mut bytes = encoded(&record(value));
            let end = bytes.len();
            bytes[end - 4..].copy_from_slice(&errno.to_le_bytes());
            with_budget(|budget| {
                assert!(matches!(
                    Stored::decode(&bytes, budget),
                    Err(Error::Encoding("invalid discovery transcript"))
                ))
            });
        }
    }
    for legacy in [true, false] {
        let interrupted = Attempt::OpenFailed(NativeError::Os(nix::libc::EINTR));
        assert!(if legacy {
            Transcript::checked(
                Filename::Config,
                true,
                interrupted,
                After::Fallback(Exists::Present),
            )
        } else {
            Transcript::checked(
                Filename::Config,
                true,
                acquired(7),
                After::Second(interrupted),
            )
        }
        .is_none());
    }
    for length in 0..base.len() {
        with_budget(|budget| assert!(Stored::decode(&base[..length], budget).is_err()));
    }
    let mut trailing = base.clone();
    trailing.push(0);
    let mut old = base;
    old[super::MAGIC.len() - 1] = 1;
    for bytes in [trailing, old] {
        with_budget(|budget| assert!(Stored::decode(&bytes, budget).is_err()));
    }
    for invalid in [0, b'r'] {
        let mut bytes = encoded(&record(transcript(
            Filename::Config,
            false,
            acquired(7),
            After::None,
        )));
        bytes[query + 1 + 8] = invalid;
        with_budget(|budget| {
            assert!(matches!(
                Stored::decode(&bytes, budget),
                Err(Error::Encoding("expected absolute native path"))
            ))
        });
    }
    // Existing tags retain their bytes and remain ordered before discovery.
    let prefix = observed::Record {
        query: observed::Query::EnvironmentPrefix(b"CARGO_".to_vec()),
        value: observed::Value::EnvironmentPrefix(false),
    };
    let prefix_bytes = encoded(&prefix);
    assert_eq!(prefix_bytes[query], 8);
    assert_eq!(*prefix_bytes.last().expect("value tag"), 10);
    assert!(
        prefix.query
            < record(transcript(
                Filename::Config,
                false,
                acquired(7),
                After::None
            ))
            .query
    );
}

#[test]
fn discovery_retention_and_decode_preserve_original_ownership_floors() {
    let record = record(transcript(
        Filename::Config,
        false,
        acquired(7),
        After::None,
    ));
    let bytes = encoded(&record);
    let items = 7 + 2; // prior, record vector and directory bytes; inert transcript
    let owned = 19 + std::mem::size_of::<observed::Record>() + 4;
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
    // records the axis; only checked-arithmetic overflow refuses, before any
    // ownership.
    let graph = node::GraphBudget::new();
    graph
        .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize - bytes.len() + 1)
        .expect("prior decode work");
    let limits = cargo::Limits::standard();
    let mut allocation = input::Allocation::new(&limits);
    allocation.items(7).expect("prior items");
    allocation.bytes(19).expect("prior bytes");
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
    assert_eq!(allocation.usage().items, items);
    assert_eq!(allocation.usage().bytes, owned);
    let graph = node::GraphBudget::new();
    graph
        .decode(usize::MAX)
        .expect("saturate the decode account");
    let mut allocation = input::Allocation::new(&limits);
    allocation.items(7).expect("prior items");
    allocation.bytes(19).expect("prior bytes");
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
