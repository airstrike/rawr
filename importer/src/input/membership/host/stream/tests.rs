use super::*;
use crate::input::membership::{
    observed::stream::{Row, Transcript},
    Budget,
};
use crate::{cargo, input, node};
use nix::libc;

fn rows() -> Vec<Row> {
    vec![
        Row::checked(b"z".to_vec(), 0, Some(Ok(observed::Kind::Directory))).unwrap(),
        Row::checked(b".skip".to_vec(), 0, None).unwrap(),
        Row::checked(b"z".to_vec(), 8, Some(Ok(observed::Kind::File))).unwrap(),
    ]
}
fn image(transcript: Transcript) -> stored::Stored {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let record = observed::Record {
        query: observed::Query::DirectoryStream(
            observed::Path::from_bytes(b"/dir", &mut budget).unwrap(),
        ),
        value: observed::Value::DirectoryStream(transcript),
    }
    .checked()
    .unwrap();
    stored::Stored::from_records(&[record], &mut budget).unwrap()
}
fn sample() -> stored::Stored {
    image(Transcript::Started {
        mount: rawr_directory::Mount::Failed(libc::EIO),
        rows: rows(),
        end: observed::End::Error(libc::EINTR),
    })
}
fn consume(host: &mut Host<'_, '_>) -> Result<()> {
    let mut cursor = host.stream(b"/dir")?.expect("opened");
    let mut names = Vec::new();
    loop {
        match cursor.next(host)? {
            Next::Entry(entry) => {
                names.push(entry.name().to_vec());
                if !entry.name().starts_with(b".") {
                    assert!(entry.inspect(host)?.is_ok());
                }
            }
            Next::End(end) => {
                assert_eq!(end, observed::End::Error(libc::EINTR));
                break;
            }
        }
    }
    assert_eq!(names, [b"z".to_vec(), b".skip".to_vec(), b"z".to_vec()]);
    cursor.finish(host)
}
#[test]
fn ordered_roundtrip_and_exact_cursor_completion() {
    let original = sample();
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let encoded = original.encode(&mut budget).unwrap();
    let decoded = stored::Stored::decode(&encoded, &mut budget).unwrap();
    assert_eq!(original, decoded);
    let mut host = Host::from_image(&decoded, budget).unwrap();
    consume(&mut host).unwrap();
    host.finish().unwrap();
}
#[test]
fn abandoned_missing_and_extra_inspections_cannot_consume_image() {
    let image = sample();
    let limits = cargo::Limits::standard();
    for mode in 0..3 {
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
        let mut cursor = host.stream(b"/dir").unwrap().unwrap();
        if mode == 0 {
        } else if mode == 1 {
            assert!(matches!(cursor.next(&mut host), Ok(Next::Entry(_))));
            assert!(matches!(cursor.next(&mut host), Err(Error::Unconsumed)));
        } else {
            let Next::Entry(entry) = cursor.next(&mut host).unwrap() else {
                panic!("entry")
            };
            assert!(entry.inspect(&mut host).unwrap().is_ok());
            let Next::Entry(entry) = cursor.next(&mut host).unwrap() else {
                panic!("entry")
            };
            assert!(matches!(entry.inspect(&mut host), Err(Error::Unobserved)));
            assert!(matches!(cursor.next(&mut host), Err(Error::Unconsumed)));
        }
        assert!(matches!(host.finish(), Err(Error::Unconsumed)));
    }
}
#[test]
fn open_failure_is_distinct_from_empty_completed_stream() {
    for transcript in [
        Transcript::OpenFailed(libc::ENOENT),
        Transcript::UnionOpenFailed {
            mount: rawr_directory::Mount::Known(true),
            error: libc::EIO,
        },
    ] {
        let image = image(transcript);
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
        assert!(host.stream(b"/dir").unwrap().is_err());
        host.finish().unwrap();
    }
    let image = image(Transcript::Started {
        mount: rawr_directory::Mount::Known(false),
        rows: vec![],
        end: observed::End::Eof,
    });
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
    let mut cursor = host.stream(b"/dir").unwrap().unwrap();
    assert!(matches!(
        cursor.next(&mut host),
        Ok(Next::End(observed::End::Eof))
    ));
    cursor.finish(&mut host).unwrap();
    host.finish().unwrap();
}
#[test]
fn foreign_image_cursor_is_rejected() {
    let first = sample();
    let second = sample();
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut a = input::Allocation::new(&limits);
    let mut b = input::Allocation::new(&limits);
    let mut first_host = Host::from_image(
        &first,
        Budget {
            graph: &graph,
            allocation: &mut a,
        },
    )
    .unwrap();
    let mut second_host = Host::from_image(
        &second,
        Budget {
            graph: &graph,
            allocation: &mut b,
        },
    )
    .unwrap();
    let mut cursor = first_host.stream(b"/dir").unwrap().unwrap();
    assert!(matches!(
        cursor.next(&mut second_host),
        Err(Error::ObservationType)
    ));
    assert!(matches!(first_host.finish(), Err(Error::Unconsumed)));
}
#[test]
fn malformed_results_truncations_and_old_schema_are_rejected() {
    assert!(Row::checked(b"file".to_vec(), 8, Some(Err(libc::EIO))).is_err());
    assert!(Row::checked(b"../bad".to_vec(), 0, None).is_err());
    assert!(Transcript::UnionOpenFailed {
        mount: rawr_directory::Mount::Known(false),
        error: libc::EIO
    }
    .checked()
    .is_err());
    let image = sample();
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let bytes = image.encode(&mut budget).unwrap();
    for end in 0..bytes.len() {
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::new(&limits);
        assert!(stored::Stored::decode(
            &bytes[..end],
            &mut Budget {
                graph: &graph,
                allocation: &mut allocation
            }
        )
        .is_err());
    }
    let mut old = bytes;
    old[b"rawr-package-observations\0".len()] = 2;
    assert!(stored::Stored::decode(&old, &mut budget).is_err());
}
fn budget_run(items: usize, bytes: usize) -> (Result<()>, input::Usage, crate::exceedance::Record) {
    let image = sample();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::with_limits(items, bytes, usize::MAX);
    let result = (|| {
        let mut host = Host::from_image(
            &image,
            Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
        )?;
        consume(&mut host)?;
        host.finish()?;
        Ok(())
    })();
    (result, allocation.usage(), allocation.exceedances())
}
#[test]
fn cursor_uses_the_original_exact_account() {
    let (result, usage, _) = budget_run(usize::MAX, usize::MAX);
    result.unwrap();
    let (result, exact, exceeded) = budget_run(usage.items, usage.bytes);
    result.unwrap();
    assert_eq!(exact, usage);
    assert!(!exceeded.is_exceeded());
    // One less on either axis still completes the cursor and records
    // that axis with the exact total as its high water.
    let (result, actual, exceeded) = budget_run(usage.items - 1, usage.bytes);
    result.unwrap();
    assert_eq!(actual, usage);
    let axis = exceeded.axis(input::INPUT_ITEMS_RESOURCE).unwrap();
    assert_eq!(
        (axis.target, axis.high_water),
        (usage.items as u64 - 1, usage.items as u64)
    );
    assert!(exceeded.axis(input::INPUT_BYTES_RESOURCE).is_none());
    let (result, actual, exceeded) = budget_run(usage.items, usage.bytes - 1);
    result.unwrap();
    assert_eq!(actual, usage);
    let axis = exceeded.axis(input::INPUT_BYTES_RESOURCE).unwrap();
    assert_eq!(
        (axis.target, axis.high_water),
        (usage.bytes as u64 - 1, usage.bytes as u64)
    );
    assert!(exceeded.axis(input::INPUT_ITEMS_RESOURCE).is_none());
}

#[test]
fn same_image_cursors_cannot_transfer_work_or_completion_to_another_host() {
    let image = sample();
    let limits = cargo::Limits::standard();
    for operation in 0..3 {
        let graph = node::GraphBudget::new();
        let mut first_allocation = input::Allocation::new(&limits);
        let mut second_allocation = input::Allocation::new(&limits);
        let mut first = Host::from_image(
            &image,
            Budget {
                graph: &graph,
                allocation: &mut first_allocation,
            },
        )
        .unwrap();
        let mut second = Host::from_image(
            &image,
            Budget {
                graph: &graph,
                allocation: &mut second_allocation,
            },
        )
        .unwrap();
        let mut cursor = first.stream(b"/dir").unwrap().unwrap();
        let _other = second.stream(b"/dir").unwrap().unwrap();
        assert_eq!(first.streams, 1);
        assert_eq!(second.streams, 1);
        match operation {
            0 => assert!(matches!(
                cursor.next(&mut second),
                Err(Error::ObservationType)
            )),
            1 => {
                let Next::Entry(entry) = cursor.next(&mut first).unwrap() else {
                    panic!("entry")
                };
                assert!(matches!(
                    entry.inspect(&mut second),
                    Err(Error::ObservationType)
                ));
            }
            _ => {
                while let Next::Entry(entry) = cursor.next(&mut first).unwrap() {
                    if !entry.name().starts_with(b".") {
                        assert!(entry.inspect(&mut first).unwrap().is_ok());
                    }
                }
                assert!(matches!(cursor.finish(&mut second), Err(Error::Unconsumed)));
            }
        }
        assert!(matches!(first.finish(), Err(Error::Unconsumed)));
        assert!(matches!(second.finish(), Err(Error::Unconsumed)));
    }
}
