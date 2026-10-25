use super::{layout::MAX_NAME, Batch, Framing, Record, NAME_OFFSET};
use crate::tests::{record, Audit};
use crate::{Error, Work};

/// The name bytes a record's scan admits: Darwin's declared name and its
/// terminator, or Linux's whole name field.
fn scanned(name: &[u8], record: &[u8]) -> usize {
    if cfg!(target_os = "macos") {
        name.len() + 1
    } else {
        record.len() - NAME_OFFSET
    }
}

#[test]
fn native_bytes_types_wide_names_and_native_framing_are_preserved() {
    for name in [b"\xff\xfe".to_vec(), vec![b'x'; MAX_NAME]] {
        let bytes = record(&name, u64::MAX, 253);
        let mut audit = Audit::default();
        let mut batch = Batch::new(&bytes);
        let Some(Record::Entry(entry)) = batch.next(&mut audit).unwrap() else {
            panic!("expected present entry");
        };
        assert_eq!(entry.name(), name);
        assert_eq!(entry.kind(), 253);
        assert!(batch.next(&mut audit).unwrap().is_none());
        assert_eq!(
            audit.events,
            [
                Work::Record,
                Work::Name {
                    bytes: scanned(&name, &bytes)
                },
                Work::Record
            ]
        );
        assert!(matches!(batch.next(&mut audit), Err(Error::Terminal)));
    }
}

fn assert_malformed(cases: Vec<(Vec<u8>, Framing)>) {
    for (bytes, expected) in cases {
        let mut batch = Batch::new(&bytes);
        let mut audit = Audit::default();
        assert!(
            matches!(batch.next(&mut audit), Err(Error::Framing(actual)) if actual == expected),
            "{bytes:?} should fail as {expected:?}"
        );
        let calls = audit.events.len();
        assert!(matches!(batch.next(&mut audit), Err(Error::Terminal)));
        assert_eq!(audit.events.len(), calls);
    }
}

#[cfg(target_os = "macos")]
#[test]
fn malformed_prefix_and_name_shapes_fail_without_becoming_eof() {
    let mut cases = vec![(vec![0; 20], Framing::Header)];
    for length in [0u16, 20, 23, 28] {
        let mut bytes = record(b"a", 1, 8);
        bytes[16..18].copy_from_slice(&length.to_ne_bytes());
        cases.push((bytes, Framing::Length));
    }
    for length in [0u16, 4, 1024] {
        let mut bytes = record(b"a", 1, 8);
        bytes[18..20].copy_from_slice(&length.to_ne_bytes());
        cases.push((bytes, Framing::NameLength));
    }
    let mut missing_nul = record(b"a", 1, 8);
    missing_nul[22] = b'x';
    cases.extend([
        (missing_nul, Framing::Name),
        (record(b"a\0b", 1, 8), Framing::Name),
        (record(b"a/b", 1, 8), Framing::Name),
    ]);
    assert_malformed(cases);
}

#[cfg(not(target_os = "macos"))]
#[test]
fn malformed_prefix_and_name_shapes_fail_without_becoming_eof() {
    let mut cases = vec![(vec![0; NAME_OFFSET - 1], Framing::Header)];
    // Zero, short, unaligned, past the prefix, and aligned but not the exact
    // length filldir64 writes for this name.
    for length in [0u16, 16, 20, 23, 28, 32] {
        let mut bytes = record(b"a", 1, 8);
        bytes[16..18].copy_from_slice(&length.to_ne_bytes());
        cases.push((bytes, Framing::Length));
    }
    let mut padded = record(b"a", 1, 8);
    padded.extend([0; 8]);
    padded[16..18].copy_from_slice(&32u16.to_ne_bytes());
    cases.push((padded, Framing::Length));
    let mut empty = record(b"a", 1, 8);
    empty[NAME_OFFSET] = 0;
    cases.extend([
        (empty, Framing::NameLength),
        (record(&[b'x'; MAX_NAME + 1], 1, 8), Framing::NameLength),
    ]);
    let mut missing_nul = record(b"a", 1, 8);
    missing_nul[NAME_OFFSET + 1..].fill(b'x');
    cases.extend([
        (missing_nul, Framing::Name),
        (record(b"a/b", 1, 8), Framing::Name),
        // A terminator early enough to shorten the record is misframed.
        (record(b"a\0bcdefgh", 1, 8), Framing::Length),
    ]);
    assert_malformed(cases);
}

#[cfg(target_os = "macos")]
#[test]
fn inode_zero_is_framed_then_discarded_without_interpreting_its_name() {
    let mut bytes = record(b"?", 0, 14);
    bytes[18..20].copy_from_slice(&u16::MAX.to_ne_bytes());
    bytes[21..].fill(0xff);
    bytes.extend(record(b"same", 1, 8));
    let mut batch = Batch::new(&bytes);
    let mut audit = Audit::default();
    assert!(matches!(
        batch.next(&mut audit).unwrap(),
        Some(Record::Vacant)
    ));
    assert_eq!(audit.events, [Work::Record]);
    let Some(Record::Entry(entry)) = batch.next(&mut audit).unwrap() else {
        panic!("expected valid later entry");
    };
    assert_eq!(entry.name(), b"same");
    assert!(batch.next(&mut audit).unwrap().is_none());
}

#[cfg(not(target_os = "macos"))]
#[test]
fn inode_zero_is_an_entry_and_padding_after_the_terminator_is_never_read() {
    // Bytes past the terminator are whatever the buffer held before the call.
    let mut bytes = record(b"zeros", 0, 8);
    bytes[NAME_OFFSET + 6..].fill(b'/');
    bytes.extend(record(b"same", 1, 4));
    let mut batch = Batch::new(&bytes);
    let mut audit = Audit::default();
    for (name, kind) in [(&b"zeros"[..], 8), (b"same", 4)] {
        let Some(Record::Entry(entry)) = batch.next(&mut audit).unwrap() else {
            panic!("expected an entry");
        };
        assert_eq!((entry.name(), entry.kind()), (name, kind));
    }
    assert!(batch.next(&mut audit).unwrap().is_none());
}

#[test]
fn partial_trailing_record_is_an_error_and_admission_precedes_inspection() {
    let mut bytes = record(b"a", 1, 8);
    bytes.extend(vec![0; NAME_OFFSET - 1]);
    let mut batch = Batch::new(&bytes);
    let mut audit = Audit::default();
    assert!(batch.next(&mut audit).unwrap().is_some());
    assert!(matches!(
        batch.next(&mut audit),
        Err(Error::Framing(Framing::Header))
    ));

    for (bytes, reject_at) in [(vec![0; 20], 0), (record(b"a/b", 1, 8), 1)] {
        let mut batch = Batch::new(&bytes);
        let mut audit = Audit {
            reject_at: Some(reject_at),
            ..Audit::default()
        };
        assert!(matches!(batch.next(&mut audit), Err(Error::Admission(()))));
        assert_eq!(audit.events.len(), reject_at + 1);
    }
}
