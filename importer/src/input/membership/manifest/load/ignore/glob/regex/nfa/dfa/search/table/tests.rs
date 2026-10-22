use super::super::super::{info, tests::Count};
use super::super::tests::engine;
use super::{accelerate, accelerated, Error, Session, HELPER_CALLS, MIN_ACCEL};
use crate::input::membership::manifest::{reference::Account, Stop};

#[test]
fn actual_pinned_helpers_cover_empty_suffix_needles_offsets_hits_and_misses() {
    for (glob, needles) in [
        // Actual Cargo glob configuration excludes slash from '*' self-loops.
        ("*", b"/".as_slice()),
        ("*a", b"/a".as_slice()),
        ("*{a,b}", b"/ab".as_slice()),
    ] {
        let dfa = engine(&[glob], info::Kind::All);
        let dense = &dfa.completed.as_ref().unwrap().forward;
        let start = dense.starts.table.values[2];
        let id = dense.transitions.values
            [start as usize + usize::from(dense.classes[usize::from(b'x')])];
        assert!(accelerated(dense, id));
        let index = ((id - dense.special[MIN_ACCEL]) as usize) >> dense.stride2;
        assert_eq!(
            usize::from(dense.accelerators.values[1 + index * 2].to_ne_bytes()[0]),
            needles.len()
        );
        for prefix in [0, 1, 15, 16, 17, 31, 32, 63, 64] {
            let mut bytes = vec![b'x'; prefix + 129];
            for at in [prefix, bytes.len()] {
                let before = HELPER_CALLS.with(|calls| calls.get());
                let mut count = Count::default();
                assert_eq!(
                    accelerate(dense, id, &bytes, at, &mut Session::new(&mut count)).unwrap(),
                    bytes.len()
                );
                assert_eq!(HELPER_CALLS.with(|calls| calls.get()), before + 1);
                assert_eq!((count.calls, count.items), (1, 1));
                assert!(count.allocations.is_empty());
            }
            for needle in needles {
                for offset in [0, 1, 15, 16, 17, 127, 128] {
                    bytes[prefix + offset] = *needle;
                    assert_eq!(
                        accelerate(dense, id, &bytes, prefix, &mut Session::new(&mut Account))
                            .unwrap(),
                        prefix + offset
                    );
                    bytes[prefix + offset] = b'x';
                }
            }
            // The earliest occurrence wins regardless of needle order.
            bytes[prefix + 5] = *needles.last().unwrap();
            bytes[prefix + 7] = needles[0];
            assert_eq!(
                accelerate(dense, id, &bytes, prefix, &mut Session::new(&mut Account)).unwrap(),
                prefix + 5
            );
        }
    }
}

#[test]
fn refusing_search_event_never_enters_helper_or_publishes_cursor_advance() {
    let dfa = engine(&["*a"], info::Kind::All);
    let dense = &dfa.completed.as_ref().unwrap().forward;
    let start = dense.starts.table.values[2];
    let id =
        dense.transitions.values[start as usize + usize::from(dense.classes[usize::from(b'x')])];
    assert!(accelerated(dense, id));
    let before = HELPER_CALLS.with(|calls| calls.get());
    let mut count = Count {
        refuse: Some(0),
        ..Count::default()
    };
    let mut session = Session::new(&mut count);
    let mut cursor = 1;
    let result = (|| {
        cursor = accelerate(dense, id, b"xxxa", cursor, &mut session)?;
        Ok(())
    })();
    assert!(matches!(result, Err(Stop::Refused)));
    assert_eq!(cursor, 1);
    assert_eq!(HELPER_CALLS.with(|calls| calls.get()), before);
    assert!(matches!(
        accelerate(dense, id, b"xxxa", cursor, &mut session),
        Err(Stop::Refused)
    ));
    assert!(matches!(session.finish(result), Err(Error::Admission(0))));
    assert_eq!(count.calls, 1);
    assert!(count.allocations.is_empty());
}
