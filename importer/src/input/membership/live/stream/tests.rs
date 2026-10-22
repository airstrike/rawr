use super::*;
use crate::{input, node};
use std::{
    fs,
    os::unix::ffi::OsStrExt as _,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "rawr-stream-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn bytes(&self) -> &[u8] {
        self.0.as_os_str().as_bytes()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn run(
    path: &[u8],
    items: usize,
    bytes: usize,
) -> (
    Result<observed::Record>,
    input::Usage,
    crate::exceedance::Record,
) {
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::with_limits(items, bytes, usize::MAX);
    let result = (|| {
        let mut budget = Budget {
            graph: &graph,
            allocation: &mut allocation,
        };
        match open(path, &mut budget)? {
            Start::Failed(record) => Ok(record),
            Start::Opened(mut capture) => {
                while let Next::Entry(entry) = capture.next()? {
                    // Pinned Cargo skips dot/non-UTF8 names before type.
                    if std::str::from_utf8(entry.name()).is_ok_and(|name| !name.starts_with('.')) {
                        let _ = entry.inspect()?;
                    }
                }
                capture.finish()
            }
        }
    })();
    (result, allocation.usage, allocation.exceedances())
}

#[test]
fn native_capture_preserves_order_and_only_reached_inspections() {
    let fixture = Fixture::new();
    for name in ["z.rs", "a.rs", ".hidden"] {
        fs::write(fixture.0.join(name), b"").unwrap();
    }
    fs::create_dir(fixture.0.join("nested")).unwrap();
    let expected = fs::read_dir(&fixture.0)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().as_bytes().to_vec())
        .collect::<Vec<_>>();
    let (record, _, _) = run(fixture.bytes(), usize::MAX, usize::MAX);
    let observed::Value::DirectoryStream(Transcript::Started { rows, end, .. }) =
        record.unwrap().value
    else {
        panic!("started")
    };
    assert_eq!(end, observed::End::Eof);
    assert_eq!(
        rows.iter().map(|row| &row.name).collect::<Vec<_>>(),
        expected.iter().collect::<Vec<_>>()
    );
    for row in rows {
        if row.name == b".hidden" || std::str::from_utf8(&row.name).is_err() {
            assert_eq!(row.inspected, None);
        } else if row.name == b"nested" {
            assert_eq!(row.inspected, Some(Ok(observed::Kind::Directory)));
        } else {
            assert_eq!(row.inspected, Some(Ok(observed::Kind::File)));
        }
    }
}

#[test]
fn unfinished_or_refused_capture_never_returns_a_record() {
    let fixture = Fixture::new();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::with_limits(usize::MAX, 4096, usize::MAX);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let Start::Opened(capture) = open(fixture.bytes(), &mut budget).unwrap() else {
        panic!("opened")
    };
    assert!(matches!(capture.finish(), Err(Error::Unconsumed)));
    let Start::Opened(mut capture) = open(fixture.bytes(), &mut budget).unwrap() else {
        panic!("opened")
    };
    assert!(capture
        .probe(|budget| Ok(budget.allocation.bytes(usize::MAX)?))
        .is_err());
    assert!(capture.next().is_err());
    assert!(capture.finish().is_err());
}

#[test]
fn failed_external_graph_probe_poisoning_survives_observed_eof() {
    let fixture = Fixture::new();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, usize::MAX);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let Start::Opened(mut capture) = open(fixture.bytes(), &mut budget).unwrap() else {
        panic!("opened")
    };
    assert!(matches!(
        capture.next().unwrap(),
        Next::End(observed::End::Eof)
    ));
    assert!(capture
        .probe(|budget| Ok(budget.graph.paths(usize::MAX)?))
        .is_err());
    assert!(matches!(capture.finish(), Err(Error::Unconsumed)));
}

#[test]
fn native_failure_is_data_and_exact_original_ownership_floors_hold() {
    let fixture = Fixture::new();
    let missing = fixture.0.join("absent");
    let (record, _, _) = run(missing.as_os_str().as_bytes(), usize::MAX, usize::MAX);
    assert!(
        matches!(record.unwrap().value, observed::Value::DirectoryStream(Transcript::OpenFailed(code)) if code > 0)
    );
    fs::write(fixture.0.join("entry.rs"), b"").unwrap();
    let (expected, usage, _) = run(fixture.bytes(), usize::MAX, usize::MAX);
    let expected = expected.unwrap();
    let (result, exact, exceeded) = run(fixture.bytes(), usage.items, usage.bytes);
    assert_eq!(result.unwrap(), expected);
    assert_eq!(exact, usage);
    assert!(!exceeded.is_exceeded());
    // One less on either axis still captures the same record and
    // records that axis with the exact total as its high water.
    let (result, actual, exceeded) = run(fixture.bytes(), usage.items - 1, usage.bytes);
    assert_eq!(result.unwrap(), expected);
    assert_eq!(actual, usage);
    let axis = exceeded.axis(input::INPUT_ITEMS_RESOURCE).unwrap();
    assert_eq!(
        (axis.target, axis.high_water),
        (usage.items as u64 - 1, usage.items as u64)
    );
    assert!(exceeded.axis(input::INPUT_BYTES_RESOURCE).is_none());
    let (result, actual, exceeded) = run(fixture.bytes(), usage.items, usage.bytes - 1);
    assert_eq!(result.unwrap(), expected);
    assert_eq!(actual, usage);
    let axis = exceeded.axis(input::INPUT_BYTES_RESOURCE).unwrap();
    assert_eq!(
        (axis.target, axis.high_water),
        (usage.bytes as u64 - 1, usage.bytes as u64)
    );
    assert!(exceeded.axis(input::INPUT_ITEMS_RESOURCE).is_none());
}

#[test]
fn live_capture_roundtrips_into_consumed_replay_with_the_same_reached_queries() {
    use crate::input::membership::{host, stored};
    let fixture = Fixture::new();
    fs::write(fixture.0.join("file.rs"), b"").unwrap();
    fs::write(fixture.0.join(".skip"), b"").unwrap();
    let record = run(fixture.bytes(), usize::MAX, usize::MAX).0.unwrap();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, usize::MAX);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let retained = stored::Stored::from_records(&[record], &mut budget).unwrap();
    let bytes = retained.encode(&mut budget).unwrap();
    let image = stored::Stored::decode(&bytes, &mut budget).unwrap();
    let mut host = host::Host::from_image(&image, budget).unwrap();
    let mut cursor = host.stream(fixture.bytes()).unwrap().unwrap();
    loop {
        match cursor.next(&mut host).unwrap() {
            host::stream::Next::Entry(entry) => {
                if entry.name() == b"file.rs" {
                    assert_eq!(entry.inspect(&mut host).unwrap(), Ok(observed::Kind::File));
                }
            }
            host::stream::Next::End(end) => {
                assert_eq!(end, observed::End::Eof);
                break;
            }
        }
    }
    cursor.finish(&mut host).unwrap();
    assert!(host.finish().is_ok());
}

#[test]
fn recorded_non_utf8_name_roundtrips_without_an_unreached_type_query() {
    // The installed filesystem rejects creation of this name with EILSEQ.
    // Exercise its lossless stored/replay path with native byte data instead.
    use crate::input::membership::{host, stored};
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::with_limits(usize::MAX, usize::MAX, usize::MAX);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let record = record(
        observed::Path::from_bytes(b"/dir", &mut budget).unwrap(),
        Transcript::Started {
            mount: rawr_directory::Mount::Known(false),
            rows: vec![Row::checked(b"invalid-\xff.rs".to_vec(), 0, None).unwrap()],
            end: observed::End::Eof,
        },
    )
    .unwrap();
    let retained = stored::Stored::from_records(&[record], &mut budget).unwrap();
    let encoded = retained.encode(&mut budget).unwrap();
    let image = stored::Stored::decode(&encoded, &mut budget).unwrap();
    let mut host = host::Host::from_image(&image, budget).unwrap();
    let mut cursor = host.stream(b"/dir").unwrap().unwrap();
    let host::stream::Next::Entry(entry) = cursor.next(&mut host).unwrap() else {
        panic!("entry")
    };
    assert_eq!(entry.name(), b"invalid-\xff.rs");
    assert!(std::str::from_utf8(entry.name()).is_err());
    assert!(matches!(
        cursor.next(&mut host).unwrap(),
        host::stream::Next::End(observed::End::Eof)
    ));
    cursor.finish(&mut host).unwrap();
    host.finish().unwrap();
}
