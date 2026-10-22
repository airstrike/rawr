use super::*;
use crate::tests::{record, Audit};
use std::{cell::RefCell, collections::VecDeque, rc::Rc};

thread_local! { static NEXT: RefCell<Option<Rc<RefCell<Script>>>> = const { RefCell::new(None) }; }
struct Script {
    mount: Result<bool, i32>,
    batches: VecDeque<Result<Vec<u8>, i32>>,
    types: VecDeque<Result<libc::mode_t, i32>>,
    events: Vec<(u8, &'static str)>,
    tails: VecDeque<Option<u32>>,
}
struct Fake {
    id: u8,
    shared: Rc<RefCell<Script>>,
}
impl System for Fake {
    fn open(_: &CStr) -> io::Result<Self> {
        let shared = NEXT.with(|next| next.borrow_mut().take().unwrap());
        shared.borrow_mut().events.push((0, "open"));
        Ok(Self { id: 0, shared })
    }
    fn mount(&self) -> io::Result<bool> {
        let mut script = self.shared.borrow_mut();
        script.events.push((self.id, "mount"));
        script.mount.map_err(io::Error::from_raw_os_error)
    }
    fn reopen(&self) -> io::Result<Self> {
        self.shared.borrow_mut().events.push((self.id, "reopen"));
        Ok(Self {
            id: 1,
            shared: self.shared.clone(),
        })
    }
    fn read(&mut self, buffer: &mut Buffer) -> io::Result<usize> {
        let mut script = self.shared.borrow_mut();
        script.events.push((self.id, "read"));
        let bytes = script
            .batches
            .pop_front()
            .expect("no extra native read")
            .map_err(io::Error::from_raw_os_error)?;
        buffer[..bytes.len()].copy_from_slice(&bytes);
        if let Some(Some(flags)) = script.tails.pop_front() {
            buffer[crate::BUFFER_BYTES - 4..].copy_from_slice(&flags.to_ne_bytes());
        }
        Ok(bytes.len())
    }
    fn entry_type(&self, name: &CStr) -> io::Result<libc::mode_t> {
        assert!(!name.to_bytes().contains(&b'/'));
        let mut script = self.shared.borrow_mut();
        script.events.push((self.id, "type"));
        script
            .types
            .pop_front()
            .expect("no eager type probe")
            .map_err(io::Error::from_raw_os_error)
    }
}
impl Drop for Fake {
    fn drop(&mut self) {
        self.shared.borrow_mut().events.push((self.id, "drop"));
    }
}
fn setup(
    mount: Result<bool, i32>,
    batches: impl IntoIterator<Item = Result<Vec<u8>, i32>>,
    types: impl IntoIterator<Item = Result<libc::mode_t, i32>>,
) -> Rc<RefCell<Script>> {
    let shared = Rc::new(RefCell::new(Script {
        mount,
        batches: batches.into_iter().collect(),
        types: types.into_iter().collect(),
        events: vec![],
        tails: VecDeque::new(),
    }));
    NEXT.with(|next| *next.borrow_mut() = Some(shared.clone()));
    shared
}

#[test]
fn ordinary_stream_is_lazy_ordered_and_preserves_duplicate_entries() {
    let mut batch = record(b"z", 1, libc::DT_REG);
    batch.extend(record(b"a", 2, libc::DT_REG));
    batch.extend(record(b"z", 3, libc::DT_LNK));
    let shared = setup(Ok(false), [Ok(batch), Ok(vec![])], []);
    let mut audit = Audit::default();
    let mut stream = Core::<Fake>::open(c"/dir", &mut audit)
        .unwrap()
        .finish(&mut audit)
        .unwrap();
    assert_eq!(shared.borrow().events, [(0, "open"), (0, "mount")]);
    let mut output = vec![];
    while let Some(entry) = stream.next(&mut audit).unwrap() {
        output.push((
            entry.name.to_vec(),
            entry.file_type(&mut audit).unwrap().unwrap(),
        ));
    }
    assert_eq!(
        output,
        [
            (b"z".to_vec(), FileType::File),
            (b"a".to_vec(), FileType::File),
            (b"z".to_vec(), FileType::Symlink)
        ]
    );
    assert!(shared.borrow().types.is_empty());
    assert!(!shared
        .borrow()
        .events
        .iter()
        .any(|(_, event)| *event == "type"));
}

#[test]
fn union_collects_independently_then_uses_original_lookup_descriptor() {
    let mut upper = record(b"hidden", 1, 14);
    upper.extend(record(b"z", 2, 0));
    let mut lower = record(b"hidden", 3, libc::DT_DIR);
    lower.extend(record(b"a", 4, 0));
    lower.extend(record(b"z", 5, libc::DT_REG));
    let shared = setup(
        Ok(true),
        [Ok(upper), Ok(lower), Ok(vec![])],
        [Ok(libc::S_IFDIR), Err(libc::ENOENT)],
    );
    let mut audit = Audit::default();
    let mut stream = Core::<Fake>::open(c"/dir", &mut audit)
        .unwrap()
        .finish(&mut audit)
        .unwrap();
    assert_eq!(
        shared.borrow().events,
        [
            (0, "open"),
            (0, "mount"),
            (0, "reopen"),
            (1, "read"),
            (1, "read"),
            (1, "read"),
            (1, "drop")
        ]
    );
    let first = stream.next(&mut audit).unwrap().unwrap();
    assert_eq!(first.name, b"z");
    assert_eq!(
        first.file_type(&mut audit).unwrap(),
        Ok(FileType::Directory)
    );
    let second = stream.next(&mut audit).unwrap().unwrap();
    assert_eq!(second.name, b"a");
    assert_eq!(second.file_type(&mut audit).unwrap(), Err(libc::ENOENT));
    assert!(stream.next(&mut audit).unwrap().is_none());
    assert!(shared
        .borrow()
        .events
        .iter()
        .filter(|(_, e)| *e == "type")
        .all(|(id, _)| *id == 0));
}

#[test]
fn partial_native_read_error_remains_error_and_mount_failure_is_retained() {
    let shared = setup(
        Err(libc::EIO),
        [Ok(record(b"prefix", 1, libc::DT_REG)), Err(libc::EINTR)],
        [],
    );
    let mut audit = Audit::default();
    let opening = Core::<Fake>::open(c"/dir", &mut audit).unwrap();
    assert_eq!(opening.mount, Mount::Failed(libc::EIO));
    let mut stream = opening.finish(&mut audit).unwrap();
    assert_eq!(stream.next(&mut audit).unwrap().unwrap().name, b"prefix");
    assert!(
        matches!(stream.next(&mut audit),Err(Error::Io{source,..}) if source.raw_os_error()==Some(libc::EINTR))
    );
    let events = shared.borrow().events.clone();
    assert!(stream.next(&mut audit).unwrap().is_none());
    assert_eq!(shared.borrow().events, events);
    assert!(!events.iter().any(|(_, e)| *e == "reopen"));
}

#[test]
fn union_error_prevents_open_completion_and_exposes_no_partial_names() {
    let shared = setup(
        Ok(true),
        [Ok(record(b"prefix", 1, libc::DT_REG)), Err(libc::EIO)],
        [],
    );
    let mut audit = Audit::default();
    let opening = Core::<Fake>::open(c"/dir", &mut audit).unwrap();
    assert_eq!(opening.mount, Mount::Known(true));
    assert!(
        matches!(opening.finish(&mut audit),Err(Error::Io{source,..}) if source.raw_os_error()==Some(libc::EIO))
    );
    assert_eq!(shared.borrow().events.last(), Some(&(0, "drop")));
}

#[test]
fn every_unrecognized_hint_uses_the_associated_native_fallback_only_when_reached() {
    for kind in 0..=u8::MAX {
        if from_hint(kind).is_some() {
            continue;
        }
        let shared = setup(Ok(false), [], [Ok(libc::S_IFDIR)]);
        let mut audit = Audit::default();
        let stream = Core::<Fake>::open(c"/dir", &mut audit)
            .unwrap()
            .finish(&mut audit)
            .unwrap();
        let result = Bound {
            directory: &stream.directory,
            name: b"entry",
            kind,
            refused: &stream.refused,
        }
        .file_type(&mut audit)
        .unwrap();
        assert_eq!(result, Ok(FileType::Directory));
        assert_eq!(shared.borrow().events.last(), Some(&(0, "type")));
    }
}

fn run_refusal(
    reject_at: Option<usize>,
    union: bool,
) -> (Result<(), Error<()>>, Vec<Work>, Vec<(u8, &'static str)>) {
    let shared = setup(
        Ok(union),
        [Ok(record(b"entry", 1, 0)), Ok(vec![])],
        [Ok(libc::S_IFDIR)],
    );
    let mut audit = Audit {
        reject_at,
        ..Audit::default()
    };
    let result = (|| {
        let mut stream = Core::<Fake>::open(c"/dir", &mut audit)?.finish(&mut audit)?;
        let entry = stream.next(&mut audit)?.unwrap();
        assert_eq!(entry.file_type(&mut audit)?, Ok(FileType::Directory));
        assert!(stream.next(&mut audit)?.is_none());
        Ok(())
    })();
    let events = shared.borrow().events.clone();
    (result, audit.events, events)
}
#[test]
fn every_admission_refusal_precedes_its_work_and_drops_owned_descriptors() {
    for union in [false, true] {
        let (result, baseline, _) = run_refusal(None, union);
        result.unwrap();
        for index in 0..baseline.len() {
            let (result, events, native) = run_refusal(Some(index), union);
            assert!(matches!(result, Err(Error::Admission(()))), "{index}");
            assert_eq!(events, &baseline[..=index]);
            assert!(
                native
                    .iter()
                    .filter(|(_, event)| *event == "open" || *event == "reopen")
                    .count()
                    <= native.iter().filter(|(_, event)| *event == "drop").count()
            );
            match baseline[index] {
                Work::Open { .. } => assert!(native.is_empty()),
                Work::Mount => assert!(!native.iter().any(|(_, e)| *e == "mount")),
                Work::Reopen { .. } => assert!(!native.iter().any(|(_, e)| *e == "reopen")),
                Work::EntryType { .. } => assert!(!native.iter().any(|(_, e)| *e == "type")),
                _ => {}
            }
        }
    }
}

#[test]
fn ordinary_eof_tail_prevents_extra_read_but_union_collection_ignores_it() {
    let shared = setup(
        Ok(false),
        [Ok(record(b"last", 1, libc::DT_REG)), Err(libc::EIO)],
        [],
    );
    shared.borrow_mut().tails.push_back(Some(1));
    let mut audit = Audit::default();
    let mut stream = Core::<Fake>::open(c"/dir", &mut audit)
        .unwrap()
        .finish(&mut audit)
        .unwrap();
    assert_eq!(stream.next(&mut audit).unwrap().unwrap().name, b"last");
    assert!(stream.next(&mut audit).unwrap().is_none());
    assert_eq!(shared.borrow().batches.len(), 1);

    let shared = setup(
        Ok(true),
        [
            Ok(record(b"upper", 1, libc::DT_REG)),
            Ok(record(b"lower", 2, libc::DT_REG)),
            Ok(vec![]),
        ],
        [],
    );
    shared.borrow_mut().tails.extend([Some(1), Some(1), None]);
    let mut stream = Core::<Fake>::open(c"/dir", &mut audit)
        .unwrap()
        .finish(&mut audit)
        .unwrap();
    assert_eq!(stream.next(&mut audit).unwrap().unwrap().name, b"upper");
    assert_eq!(stream.next(&mut audit).unwrap().unwrap().name, b"lower");
    assert!(stream.next(&mut audit).unwrap().is_none());
    assert!(shared.borrow().batches.is_empty());
}

#[test]
fn ordinary_status_tail_is_cleared_and_only_interpreted_outside_returned_prefix() {
    let mut full = record(&vec![b'a'; 1023], 1, libc::DT_REG);
    while full.len() + 24 <= crate::BUFFER_BYTES {
        full.extend(record(b"b", 2, libc::DT_REG));
    }
    // With a full prefix trailing bytes are record padding, not status flags.
    let remaining = crate::BUFFER_BYTES - full.len();
    let last_start = full.len() - 24;
    let length = u16::try_from(24 + remaining).unwrap();
    full[last_start + 16..last_start + 18].copy_from_slice(&length.to_ne_bytes());
    full.resize(crate::BUFFER_BYTES, 0);
    full[crate::BUFFER_BYTES - 4..].copy_from_slice(&1u32.to_ne_bytes());
    let shared = setup(
        Ok(false),
        [Ok(full), Ok(record(b"next", 3, libc::DT_REG)), Ok(vec![])],
        [],
    );
    let mut audit = Audit::default();
    let mut stream = Core::<Fake>::open(c"/dir", &mut audit)
        .unwrap()
        .finish(&mut audit)
        .unwrap();
    let mut last = Vec::new();
    while let Some(entry) = stream.next(&mut audit).unwrap() {
        last = entry.name.to_vec();
    }
    assert_eq!(last, b"next");
    assert!(shared.borrow().batches.is_empty());
}

#[test]
fn type_admission_refusal_is_sticky_and_cannot_complete_as_eof() {
    let shared = setup(
        Ok(false),
        [Ok(record(b"entry", 1, 0)), Ok(vec![])],
        [Ok(libc::S_IFDIR)],
    );
    let mut audit = Audit::default();
    let mut stream = Core::<Fake>::open(c"/dir", &mut audit)
        .unwrap()
        .finish(&mut audit)
        .unwrap();
    let entry = stream.next(&mut audit).unwrap().unwrap();
    audit.reject_at = Some(audit.events.len());
    assert!(matches!(
        entry.file_type(&mut audit),
        Err(Error::Admission(()))
    ));
    audit.reject_at = None;
    let events = shared.borrow().events.clone();
    assert!(matches!(stream.next(&mut audit), Err(Error::Terminal)));
    assert_eq!(shared.borrow().events, events);
}
