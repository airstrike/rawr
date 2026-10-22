use super::{Batch, Framing, Record};
use crate::tests::{record, Audit};
use crate::{Error, Work};

#[test]
fn native_bytes_types_wide_names_and_four_byte_framing_are_preserved() {
    for name in [b"\xff\xfe".to_vec(), vec![b'x'; 1023]] {
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
                    bytes: name.len() + 1
                },
                Work::Record
            ]
        );
        assert!(matches!(batch.next(&mut audit), Err(Error::Terminal)));
    }
}

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
    for (bytes, expected) in cases {
        let mut batch = Batch::new(&bytes);
        let mut audit = Audit::default();
        assert!(
            matches!(batch.next(&mut audit), Err(Error::Framing(actual)) if actual == expected)
        );
        let calls = audit.events.len();
        assert!(matches!(batch.next(&mut audit), Err(Error::Terminal)));
        assert_eq!(audit.events.len(), calls);
    }
}

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

#[test]
fn partial_trailing_record_is_an_error_and_admission_precedes_inspection() {
    let mut bytes = record(b"a", 1, 8);
    bytes.extend([0; 20]);
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
