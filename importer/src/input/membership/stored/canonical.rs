use super::{observed, Budget, Error, Stored};
use crate::{cargo, input};

fn with_budget<T>(run: impl FnOnce(&mut Budget<'_>) -> T) -> T {
    let limits = cargo::Limits::standard();
    let mut allocation = input::Allocation::new(&limits);
    run(&mut Budget {
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
        Err(libc::EINTR),
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
            let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, 4);
            allocation.items(7).expect("prior items");
            allocation.bytes(19).expect("prior bytes");
            let mut budget = Budget {
                allocation: &mut allocation,
            };
            let result = if decode {
                Stored::decode(&bytes, &mut budget)
            } else {
                Stored::from_records(std::slice::from_ref(&record), &mut budget)
            };
            // The record is retained in full.
            assert_eq!(
                result.expect("the record is retained").records(),
                std::slice::from_ref(&record)
            );
            assert_eq!(allocation.usage().items, items);
            assert_eq!(allocation.usage().bytes, owned);
            assert!(allocation.items(0).is_ok());
        }
        // Decoding past the graph's decode constant still decodes; only
        // checked-arithmetic overflow refuses, before any ownership.
        let limits = cargo::Limits::standard();
        let mut allocation = input::Allocation::new(&limits);
        Stored::decode(
            &bytes,
            &mut Budget {
                allocation: &mut allocation,
            },
        )
        .expect("decode");
        assert_eq!(allocation.usage().items, items - 7);
        assert_eq!(allocation.usage().bytes, owned - 19);
    }
}
