use std::collections::VecDeque;

use super::{run, Collector, Source};
use crate::tests::{record, Audit};
use crate::{reader, Admission, Batch, Buffer, Error, Step, Storage, Work, BUFFER_BYTES};

struct Stream {
    union: bool,
    terminal: bool,
    calls: usize,
    pending: VecDeque<Result<Vec<u8>, i32>>,
}

impl Source for Stream {
    fn is_union(&self) -> bool {
        self.union
    }

    fn read<'buffer, A: Admission>(
        &mut self,
        buffer: &'buffer mut Buffer,
        admission: &mut A,
    ) -> Result<Step<'buffer>, Error<A::Error>> {
        reader::read(&mut self.terminal, buffer, admission, |buffer| {
            self.calls += 1;
            match self
                .pending
                .pop_front()
                .expect("no read after terminal result")
            {
                Ok(bytes) => {
                    buffer[..bytes.len()].copy_from_slice(&bytes);
                    Ok(bytes.len())
                }
                Err(errno) => Err(std::io::Error::from_raw_os_error(errno)),
            }
        })
    }
}

fn stream(union: bool, batches: impl IntoIterator<Item = Result<Vec<u8>, i32>>) -> Stream {
    Stream {
        union,
        terminal: false,
        calls: 0,
        pending: batches.into_iter().collect(),
    }
}

fn layers() -> Vec<Result<Vec<u8>, i32>> {
    let mut first = record(b".", 1, 4);
    first.extend(record(b"..", 1, 4));
    first.extend(record(b"hidden", 2, 14));
    first.extend(record(b"keep", 3, 8));
    first.extend(record(b"zero", 0, 14));
    let mut second = record(b"keep", 4, 14);
    second.extend(record(b"hidden", 5, 8));
    second.extend(record(b"zero", 6, 8));
    second.extend(record(b"keep", 7, 4));
    vec![Ok(first), Ok(second), Ok(Vec::new())]
}

#[test]
fn union_first_nonzero_occurrence_and_whiteout_precedence_cross_batches() {
    let mut source = stream(true, layers());
    let mut audit = Audit::default();
    let names = run(&mut source, &mut [0; BUFFER_BYTES], &mut audit).unwrap();
    let observed: Vec<_> = names
        .iter()
        .map(|name| (name.bytes(), name.kind()))
        .collect();
    assert_eq!(observed, [(b"keep".as_slice(), 8), (b"zero".as_slice(), 8)]);
    assert_eq!(source.calls, 3);
    assert_eq!(
        audit
            .events
            .iter()
            .filter(|event| matches!(event, Work::Read { .. }))
            .count(),
        3
    );
    assert_eq!(
        audit
            .events
            .iter()
            .filter(|event| matches!(event, Work::Retain { .. }))
            .count(),
        6
    );
    assert_eq!(audit.events.last(), Some(&Work::Visible { entries: 2 }));
}

#[test]
fn ordinary_reader_keeps_duplicates_and_skips_whiteout_and_inode_zero() {
    let mut source = stream(false, layers());
    let mut audit = Audit::default();
    let names = run(&mut source, &mut [0; BUFFER_BYTES], &mut audit).unwrap();
    let observed: Vec<_> = names
        .iter()
        .map(|name| (name.bytes(), name.kind()))
        .collect();
    assert_eq!(
        observed,
        [
            (b"keep".as_slice(), 8),
            (b"hidden".as_slice(), 8),
            (b"zero".as_slice(), 8),
            (b"keep".as_slice(), 4),
        ]
    );
    assert!(!audit
        .events
        .iter()
        .any(|event| matches!(event, Work::Order | Work::Compare { .. })));
}

#[test]
fn logical_member_limit_does_not_limit_temporary_union_record_count() {
    let mut bytes = record(b"only", 1, 8);
    for _ in 0..20 {
        bytes.extend(record(b"hidden", 2, 14));
        bytes.extend(record(b"hidden", 3, 8));
        bytes.extend(record(b"only", 4, 8));
    }
    let mut source = stream(true, [Ok(bytes.clone()), Ok(Vec::new())]);
    let mut audit = Audit {
        visible_limit: Some(1),
        ..Audit::default()
    };
    let names = run(&mut source, &mut [0; BUFFER_BYTES], &mut audit).unwrap();
    assert_eq!(names.len(), 1);
    assert_eq!(names.iter().next().unwrap().bytes(), b"only");
    assert!(audit.events.contains(&Work::Retain {
        entries: 61,
        name_bytes: 4
    }));
    assert_eq!(audit.events.last(), Some(&Work::Visible { entries: 1 }));

    bytes.extend(record(b"second", 5, 8));
    let mut source = stream(true, [Ok(bytes), Ok(Vec::new())]);
    let mut audit = Audit {
        visible_limit: Some(1),
        ..Audit::default()
    };
    assert!(matches!(
        run(&mut source, &mut [0; BUFFER_BYTES], &mut audit),
        Err(Error::Admission(()))
    ));
    assert_eq!(audit.events.last(), Some(&Work::Visible { entries: 2 }));
}

#[test]
fn failed_read_after_retention_never_returns_a_partial_visible_set() {
    let mut source = stream(true, [Ok(record(b"first", 1, 8)), Err(libc::EIO)]);
    let mut audit = Audit::default();
    assert!(
        matches!(run(&mut source, &mut [0; BUFFER_BYTES], &mut audit), Err(Error::Io { source, .. }) if source.raw_os_error() == Some(libc::EIO))
    );
    assert_eq!(source.calls, 2);
    assert!(!audit
        .events
        .iter()
        .any(|event| matches!(event, Work::Visible { .. } | Work::Order)));
}

#[test]
fn interruption_after_retention_cannot_skip_to_later_entries_or_publish_names() {
    // The interrupted syscall may have advanced its native offset while
    // returning no bytes. A later successful batch cannot repair that gap.
    let mut source = stream(
        true,
        [
            Ok(record(b"retained", 1, 8)),
            Err(libc::EINTR),
            Ok(record(b"after-unreturned-gap", 3, 8)),
            Ok(Vec::new()),
        ],
    );
    let mut audit = Audit::default();
    assert!(
        matches!(run(&mut source, &mut [0; BUFFER_BYTES], &mut audit), Err(Error::Io { source, .. }) if source.raw_os_error() == Some(libc::EINTR))
    );
    assert_eq!(source.calls, 2);
    assert_eq!(source.pending.len(), 2);
    assert!(source.terminal);
    assert!(!audit.events.iter().any(|event| matches!(
        event,
        Work::Visible { .. } | Work::Order | Work::Compare { .. }
    )));
}

#[test]
fn every_admission_floor_stops_the_production_loop_before_later_work() {
    let mut source = stream(true, layers());
    let mut baseline = Audit::default();
    let names = run(&mut source, &mut [0; BUFFER_BYTES], &mut baseline).unwrap();
    assert_eq!(names.len(), 2);
    for floor in 0..baseline.events.len() {
        let mut source = stream(true, layers());
        let mut audit = Audit {
            reject_at: Some(floor),
            ..Audit::default()
        };
        assert!(matches!(
            run(&mut source, &mut [0; BUFFER_BYTES], &mut audit),
            Err(Error::Admission(()))
        ));
        assert_eq!(audit.events, baseline.events[..=floor]);
        assert_eq!(
            source.calls,
            baseline.events[..floor]
                .iter()
                .filter(|event| matches!(event, Work::Read { .. }))
                .count()
        );
    }
}

#[test]
fn ownership_growth_is_admitted_before_reservation_or_name_copy() {
    let mut collector = Collector::new(true);
    let mut audit = Audit {
        reject_at: Some(0),
        ..Audit::default()
    };
    assert!(matches!(
        collector.push(b"a", 8, &mut audit),
        Err(Error::Admission(()))
    ));
    assert_eq!(collector.entries.capacity(), 0);
    assert_eq!(collector.capacity, 0);
    audit = Audit {
        reject_at: Some(1),
        ..Audit::default()
    };
    assert!(matches!(
        collector.push(b"a", 8, &mut audit),
        Err(Error::Admission(()))
    ));
    assert_eq!(collector.entries.capacity(), 0);
    assert_eq!(collector.capacity, 0);
    assert!(
        matches!(audit.events[1], Work::Allocate { storage: Storage::Entries, bytes, .. } if bytes == 2 * std::mem::size_of::<super::Name>())
    );

    let mut audit = Audit::default();
    collector.push(b"a", 8, &mut audit).unwrap();
    collector.push(b"b", 8, &mut audit).unwrap();
    assert_eq!(collector.entries.len(), 2);
    assert_eq!(collector.capacity, 2);
    let prior = collector.entries.capacity();
    let mut audit = Audit {
        reject_at: Some(2),
        ..Audit::default()
    };
    assert!(matches!(
        collector.push(b"c", 8, &mut audit),
        Err(Error::Admission(()))
    ));
    assert_eq!(collector.entries.capacity(), prior);
    assert_eq!(collector.capacity, 2);
    assert!(
        matches!(audit.events[2], Work::Copy { bytes } if bytes == 2 * std::mem::size_of::<super::Name>())
    );
}

#[test]
fn canonical_sort_is_native_byte_exact_and_has_no_heap_allocation() {
    let names = [b"z".as_slice(), b"\xff", b"a", b"a\xfe", b"a\x80", b"a"];
    let mut bytes = Vec::new();
    for name in names {
        bytes.extend(record(name, 1, 8));
    }
    let mut collector = Collector::new(false);
    collector
        .batch(&mut Batch::new(&bytes), &mut Audit::default())
        .unwrap();
    let names = collector.finish(&mut Audit::default()).unwrap();
    let mut audit = Audit::default();
    let names = names.sorted(&mut audit).unwrap();
    assert_eq!(
        names.iter().map(|name| name.bytes()).collect::<Vec<_>>(),
        [b"a".as_slice(), b"a", b"a\x80", b"a\xfe", b"z", b"\xff"]
    );
    assert!(audit.events.iter().all(|event| matches!(
        event,
        Work::Order | Work::Compare { .. } | Work::Copy { .. }
    )));
}

#[test]
fn empty_directory_still_requires_a_real_eof_and_visible_admission() {
    let mut source = stream(false, [Ok(Vec::new())]);
    let mut audit = Audit::default();
    let names = run(&mut source, &mut [0; BUFFER_BYTES], &mut audit).unwrap();
    assert!(names.is_empty());
    assert_eq!(
        audit.events,
        [
            Work::Read {
                buffer_bytes: BUFFER_BYTES
            },
            Work::Visible { entries: 0 }
        ]
    );
}
