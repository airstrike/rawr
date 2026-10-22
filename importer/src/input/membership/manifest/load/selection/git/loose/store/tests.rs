use rawr_toml::document;

use super::{Buffer, Empty, Store};
use crate::input::membership::manifest::load::selection::git::loose::Candidate;
use crate::input::membership::manifest::{reference::Account, session::Session};

#[test]
fn head_candidate_buffer_preserves_source_append_growth() {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut buffer = Buffer {
        bytes: vec![],
        admitted: 0,
    };
    for (candidate, capacity) in Candidate::ALL.into_iter().zip([8, 16, 16, 16, 32]) {
        let name = buffer.select(candidate, &mut session).unwrap();
        assert_eq!(name.bytes, candidate.bytes());
        assert_eq!(name.candidate, candidate);
        assert_eq!(buffer.admitted, capacity);
    }
    session.finish(Ok(())).unwrap();
}

#[test]
fn default_store_admits_path_then_actual_empty_arc_and_refuses_before_publication() {
    struct Account {
        allocations: Vec<usize>,
        refuse: bool,
    }
    impl document::Admission for Account {
        type Error = ();
        fn admit(&mut self, request: document::Request) -> Result<(), ()> {
            if let document::Request::Allocation { bytes, .. } = request {
                self.allocations.push(bytes);
                if self.refuse && self.allocations.len() == 2 {
                    return Err(());
                }
            }
            Ok(())
        }
    }
    for refuse in [false, true] {
        let mut account = Account {
            allocations: vec![],
            refuse,
        };
        let mut session = Session::new(&mut account);
        let result = Store::at(std::path::Path::new("/fixture"), &mut session);
        if refuse {
            assert!(result.is_err());
            assert!(session.finish(result).is_err());
        } else {
            let store = session.finish(result).unwrap();
            assert_eq!(store.directory, std::path::Path::new("/fixture"));
            assert_eq!(std::sync::Arc::strong_count(&store._packed), 1);
            assert_eq!(
                store
                    ._packed
                    ._state
                    .load(std::sync::atomic::Ordering::Relaxed),
                0
            );
        }
        let layout = std::alloc::Layout::new::<[std::sync::atomic::AtomicUsize; 2]>()
            .extend(std::alloc::Layout::new::<Empty>())
            .unwrap()
            .0
            .pad_to_align();
        assert_eq!(account.allocations, [8, layout.size()]);
        assert_eq!(layout.size(), 32);
    }
}
