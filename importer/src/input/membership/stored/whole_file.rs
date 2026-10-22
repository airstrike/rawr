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

fn record(path: &[u8], value: observed::Value) -> observed::Record {
    observed::Record {
        query: observed::Query::File(with_budget(|budget| {
            observed::Path::from_bytes(path, budget).unwrap()
        })),
        value,
    }
}

fn content(bytes: &[u8], end: observed::End) -> observed::Value {
    observed::Value::File {
        bytes: bytes.to_vec(),
        end,
    }
}

fn encoded(records: &[observed::Record]) -> Vec<u8> {
    with_budget(|budget| {
        Stored::from_records(records, budget)
            .unwrap()
            .encode(budget)
            .unwrap()
    })
}

#[test]
fn codec_retains_native_whole_file_bytes_and_all_terminal_outcomes() {
    let records = [
        record(b"/absent", observed::Value::Absent),
        record(b"/error", observed::Value::Error(13)),
        record(b"/late", content(b"a = 1\n", observed::End::Error(5))),
        record(
            b"/native/\xff",
            content(&[0xff, 0, b'x'], observed::End::Eof),
        ),
        record(b"/zero", content(b"", observed::End::Eof)),
    ];
    let bytes = encoded(&records);
    with_budget(|budget| {
        let image = Stored::decode(&bytes, budget).unwrap();
        assert_eq!(image.records(), records.as_slice());
        assert_eq!(image.encode(budget).unwrap(), bytes);
    });
}

#[test]
fn checker_rejects_range_file_mixing_and_noncanonical_file_errors() {
    let mut invalid = vec![
        record(b"/file", observed::Value::Bytes(b"a = 1\n".to_vec())),
        record(b"/file", observed::Value::Error(0)),
        record(b"/file", content(b"partial", observed::End::Error(0))),
        record(b"/file", content(b"", observed::End::Error(5))),
    ];
    invalid.push(observed::Record {
        query: observed::Query::Read {
            path: with_budget(|budget| observed::Path::from_bytes(b"/file", budget).unwrap()),
            offset: 0,
            length: 6,
        },
        value: content(b"a = 1\n", observed::End::Eof),
    });
    for record in invalid {
        // Exercise retain/check as well as the direct checker on raw caller data.
        with_budget(|budget| {
            assert!(Stored::from_records(std::slice::from_ref(&record), budget).is_err())
        });
        assert!(record.checked().is_err());
    }
}

#[test]
fn codec_rejects_truncated_terminal_unknown_end_trailing_and_old_schema() {
    let bytes = encoded(&[record(b"/file", content(b"a = 1\n", observed::End::Eof))]);
    for length in 0..bytes.len() {
        with_budget(|budget| assert!(Stored::decode(&bytes[..length], budget).is_err()));
    }
    let mut unknown_end = bytes.clone();
    *unknown_end.last_mut().unwrap() = 2;
    let mut trailing = bytes.clone();
    trailing.push(0);
    let mut old = bytes.clone();
    old[super::MAGIC.len() - 1] = 1;
    for malformed in [&unknown_end, &trailing, &old] {
        with_budget(|budget| assert!(Stored::decode(malformed, budget).is_err()));
    }
    // The old header must reject even when its body contains only old query tags.
    let mut old_range = with_budget(|budget| {
        let records = [observed::Record {
            query: observed::Query::Read {
                path: observed::Path::from_bytes(b"/file", budget).unwrap(),
                offset: 0,
                length: 0,
            },
            value: observed::Value::Bytes(Vec::new()),
        }];
        encoded(&records)
    });
    old_range[super::MAGIC.len() - 1] = 1;
    with_budget(|budget| {
        assert!(matches!(
            Stored::decode(&old_range, budget),
            Err(Error::Encoding("unknown observation schema or Cargo pin"))
        ))
    });
}

#[test]
fn codec_rejects_mixed_value_tags_and_invalid_error_terminals() {
    let path = b"/file";
    let bytes = encoded(&[record(path, content(b"a = 1\n", observed::End::Eof))]);
    let value_tag = super::MAGIC.len() + super::CARGO_COMMIT.len() + 8 + 1 + 8 + path.len();
    let mut range_value = bytes;
    range_value[value_tag] = 3;
    range_value.pop();
    with_budget(|budget| {
        assert!(matches!(
            Stored::decode(&range_value, budget),
            Err(Error::ObservationType)
        ))
    });
    let mut error = encoded(&[record(path, content(b"a = 1\n", observed::End::Error(5)))]);
    let length = error.len();
    error[length - 4..].fill(0);
    with_budget(|budget| {
        assert!(matches!(
            Stored::decode(&error, budget),
            Err(Error::Encoding("zero is not an OS error"))
        ))
    });
    let mut empty_error = encoded(&[record(path, content(b"", observed::End::Eof))]);
    *empty_error.last_mut().unwrap() = 1;
    empty_error.extend_from_slice(&5_i32.to_le_bytes());
    with_budget(|budget| assert!(Stored::decode(&empty_error, budget).is_err()));
}

#[test]
fn whole_file_retention_and_decode_preserve_prior_ownership_and_exact_limits() {
    let records = [record(
        b"/raw",
        content(&[0xff, 0, 1, 2, 3, 4, 5, 6], observed::End::Eof),
    )];
    let bytes = encoded(&records);
    let items = 7 + 3; // prior + record + native path + raw content
    let owned = 19 + std::mem::size_of::<observed::Record>() + 4 + 8;
    for decode in [false, true] {
        for floor in 0..3 {
            let graph = node::GraphBudget::new();
            graph.decode(31).unwrap();
            let mut allocation = input::Allocation::with_limits(
                items - usize::from(floor == 1),
                owned - usize::from(floor == 2),
                4,
            );
            allocation.items(7).unwrap();
            allocation.bytes(19).unwrap();
            super::super::OWNED_BYTES.with(|count| count.set(0));
            let mut budget = Budget {
                graph: &graph,
                allocation: &mut allocation,
            };
            let result = if decode {
                Stored::decode(&bytes, &mut budget)
            } else {
                Stored::from_records(&records, &mut budget)
            };
            // Every floor retains the record in full, copying both the
            // path and the payload; one less on an axis records that axis with
            // the exact total as its high water and the payload's charge as the
            // crossing request.
            assert_eq!(result.unwrap().records(), records.as_slice());
            assert_eq!(allocation.usage().items, items);
            assert_eq!(allocation.usage().bytes, owned);
            assert_eq!(super::super::OWNED_BYTES.with(std::cell::Cell::get), 2);
            let exceeded = allocation.exceedances();
            match floor {
                0 => assert!(!exceeded.is_exceeded()),
                1 => {
                    let axis = exceeded.axis(input::INPUT_ITEMS_RESOURCE).unwrap();
                    assert_eq!(
                        (axis.target, axis.first_requested, axis.high_water),
                        (items as u64 - 1, items as u64, items as u64)
                    );
                    assert!(exceeded.axis(input::INPUT_BYTES_RESOURCE).is_none());
                }
                _ => {
                    let axis = exceeded.axis(input::INPUT_BYTES_RESOURCE).unwrap();
                    assert_eq!(
                        (axis.target, axis.first_requested, axis.high_water),
                        (owned as u64 - 1, owned as u64, owned as u64)
                    );
                    assert!(exceeded.axis(input::INPUT_ITEMS_RESOURCE).is_none());
                }
            }
            assert!(allocation.items(0).is_ok(), "nothing latched");
        }
    }
}

#[test]
fn whole_file_decode_graph_floor_is_recorded_and_overflow_refuses_before_any_owned_record() {
    let bytes = encoded(&[record(b"/raw", content(b"data", observed::End::Eof))]);
    for floor in [false, true] {
        let graph = node::GraphBudget::new();
        graph
            .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize - bytes.len() + usize::from(floor))
            .unwrap();
        let limits = cargo::Limits::standard();
        let mut allocation = input::Allocation::new(&limits);
        allocation.items(7).unwrap();
        allocation.bytes(19).unwrap();
        super::super::OWNED_VECTORS.with(|count| count.set(0));
        super::super::OWNED_BYTES.with(|count| count.set(0));
        // One byte past the decode target still decodes the image and
        // records the axis; the exact target records nothing until one more.
        Stored::decode(
            &bytes,
            &mut Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
        )
        .unwrap();
        assert_eq!(allocation.usage().items, 7 + 3);
        assert_eq!(super::super::OWNED_VECTORS.with(std::cell::Cell::get), 1);
        assert_eq!(super::super::OWNED_BYTES.with(std::cell::Cell::get), 2);
        if floor {
            let axis = graph
                .exceedances()
                .axis(node::DECODED_RESOURCE)
                .copied()
                .unwrap();
            assert_eq!(axis.target, node::MAX_DECODED_BYTES_PER_GRAPH);
            assert_eq!(axis.first_requested, node::MAX_DECODED_BYTES_PER_GRAPH + 1);
            assert_eq!(axis.high_water, node::MAX_DECODED_BYTES_PER_GRAPH + 1);
        } else {
            assert!(!graph.exceedances().is_exceeded());
            graph.decode(1).unwrap();
            assert_eq!(
                graph
                    .exceedances()
                    .axis(node::DECODED_RESOURCE)
                    .unwrap()
                    .high_water,
                node::MAX_DECODED_BYTES_PER_GRAPH + 1
            );
        }
    }
    // Checked-arithmetic overflow of the graph's decode account is a genuine
    // failure that precedes every owned record.
    let graph = node::GraphBudget::new();
    graph.decode(usize::MAX).unwrap();
    let limits = cargo::Limits::standard();
    let mut allocation = input::Allocation::new(&limits);
    allocation.items(7).unwrap();
    allocation.bytes(19).unwrap();
    super::super::OWNED_VECTORS.with(|count| count.set(0));
    super::super::OWNED_BYTES.with(|count| count.set(0));
    let result = Stored::decode(
        &bytes,
        &mut Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    );
    assert!(matches!(
        result,
        Err(Error::Budget(node::Error::Allocation))
    ));
    assert_eq!(allocation.usage().items, 7);
    assert_eq!(allocation.usage().bytes, 19);
    assert_eq!(super::super::OWNED_VECTORS.with(std::cell::Cell::get), 0);
    assert_eq!(super::super::OWNED_BYTES.with(std::cell::Cell::get), 0);
}

#[test]
fn whole_file_encoding_precharges_the_complete_result_layout() {
    let records = [record(b"/raw", content(b"data", observed::End::Error(5)))];
    let image = with_budget(|budget| Stored::from_records(&records, budget).unwrap());
    let expected = encoded(&records);
    for floor in 0..3 {
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::with_limits(
            8 - usize::from(floor == 1),
            19 + expected.len() - usize::from(floor == 2),
            4,
        );
        allocation.items(7).unwrap();
        allocation.bytes(19).unwrap();
        let result = image.encode(&mut Budget {
            graph: &graph,
            allocation: &mut allocation,
        });
        // The complete layout is charged as one item and one byte count
        // before the copy; one less on an axis records that single charge as
        // both the crossing request and the high water.
        assert_eq!(result.unwrap(), expected);
        assert_eq!(allocation.usage().items, 8);
        assert_eq!(allocation.usage().bytes, 19 + expected.len());
        let exceeded = allocation.exceedances();
        match floor {
            0 => assert!(!exceeded.is_exceeded()),
            1 => {
                let axis = exceeded.axis(input::INPUT_ITEMS_RESOURCE).unwrap();
                assert_eq!(
                    (axis.target, axis.first_requested, axis.high_water),
                    (7, 8, 8)
                );
                assert!(exceeded.axis(input::INPUT_BYTES_RESOURCE).is_none());
            }
            _ => {
                let axis = exceeded.axis(input::INPUT_BYTES_RESOURCE).unwrap();
                let total = 19 + expected.len() as u64;
                assert_eq!(
                    (axis.target, axis.first_requested, axis.high_water),
                    (total - 1, total, total)
                );
                assert!(exceeded.axis(input::INPUT_ITEMS_RESOURCE).is_none());
            }
        }
    }
}
