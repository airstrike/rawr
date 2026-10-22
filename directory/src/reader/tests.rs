use std::cell::Cell;

use super::{open, read};
use crate::tests::{record, Audit};
use crate::{Error, Framing, Record, Step, Work, BUFFER_BYTES};

#[test]
fn refusal_precedes_open_mount_and_read_and_drops_acquired_authority() {
    struct Owned<'a>(&'a Cell<bool>);
    impl Drop for Owned<'_> {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }
    for reject_at in 0..2 {
        let dropped = Cell::new(false);
        let opened = Cell::new(false);
        let mut audit = Audit {
            reject_at: Some(reject_at),
            ..Audit::default()
        };
        let result = open(
            c"/borrowed/path",
            &mut audit,
            |_| {
                assert_eq!(reject_at, 1);
                opened.set(true);
                Ok(Owned(&dropped))
            },
            |_| panic!("refused mount must not run"),
        );
        assert!(matches!(result, Err(Error::Admission(()))));
        assert_eq!(opened.get(), reject_at == 1);
        assert_eq!(dropped.get(), reject_at == 1);
    }
    let mut audit = Audit {
        reject_at: Some(0),
        ..Audit::default()
    };
    let mut terminal = false;
    let mut buffer = [0; BUFFER_BYTES];
    assert!(matches!(
        read(&mut terminal, &mut buffer, &mut audit, |_| panic!(
            "refused read must not run"
        )),
        Err(Error::Admission(()))
    ));
    assert!(terminal);
    assert!(matches!(
        read(&mut terminal, &mut buffer, &mut audit, |_| panic!(
            "terminal read must not run"
        )),
        Err(Error::Terminal)
    ));
}

#[test]
fn failed_mount_does_not_select_ordinary_semantics() {
    let mut audit = Audit::default();
    assert!(
        matches!(open(c"/path", &mut audit, |_| Ok(()), |_| Err(std::io::Error::from_raw_os_error(libc::EIO))), Err(Error::Io { source, .. }) if source.raw_os_error() == Some(libc::EIO))
    );
    assert_eq!(audit.events, [Work::Open { path_bytes: 6 }, Work::Mount]);
}

#[test]
fn interrupted_and_other_failed_reads_are_terminal_and_preserve_errno() {
    let mut audit = Audit::default();
    let mut buffer = [0; BUFFER_BYTES];
    for errno in [libc::EINTR, libc::EIO] {
        let mut terminal = false;
        assert!(
            matches!(read(&mut terminal, &mut buffer, &mut audit, |_| Err(std::io::Error::from_raw_os_error(errno))), Err(Error::Io { source, .. }) if source.raw_os_error() == Some(errno))
        );
        assert!(terminal);
        assert!(matches!(
            read(&mut terminal, &mut buffer, &mut audit, |_| panic!(
                "failed stream must not retry"
            )),
            Err(Error::Terminal)
        ));
    }
    assert_eq!(
        audit.events,
        vec![
            Work::Read {
                buffer_bytes: BUFFER_BYTES
            };
            2
        ]
    );
    let mut terminal = false;
    assert!(matches!(
        read(&mut terminal, &mut buffer, &mut audit, |_| Ok(0)),
        Ok(Step::Eof)
    ));
    assert!(terminal);
}

#[test]
fn only_returned_prefix_is_parsed_and_status_tail_never_establishes_eof() {
    let bytes = record(b"a", 1, 8);
    let mut audit = Audit::default();
    let mut terminal = false;
    let mut buffer = [0xa5; BUFFER_BYTES];
    let Step::Batch(mut batch) = read(&mut terminal, &mut buffer, &mut audit, |buffer| {
        buffer[..bytes.len()].copy_from_slice(&bytes);
        buffer[BUFFER_BYTES - 4..].copy_from_slice(&1u32.to_ne_bytes());
        Ok(bytes.len())
    })
    .unwrap() else {
        panic!("expected positive batch")
    };
    assert!(!terminal);
    assert!(matches!(
        batch.next(&mut audit).unwrap(),
        Some(Record::Entry(_))
    ));
    assert!(batch.next(&mut audit).unwrap().is_none());
    assert!(matches!(
        read(&mut terminal, &mut buffer, &mut audit, |_| Ok(
            BUFFER_BYTES + 1
        )),
        Err(Error::Framing(Framing::Prefix))
    ));
    assert!(terminal);
}
