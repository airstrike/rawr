use super::*;
use crate::input::membership::manifest::{reference, session::Session};
use std::{ffi::OsStr, os::unix::ffi::OsStrExt, path::Path};

#[test]
fn native_file_paths_keep_components_and_encode_original_bytes() {
    for (bytes, expected) in [
        (&b"/"[..], "file:///"),
        (&b"/a//./b/"[..], "file:///a/b"),
        (&b"/a/../b"[..], "file:///a/../b"),
        (&b"/a%?#\\\xff\0"[..], "file:///a%25%3F%23%5C%FF%00"),
    ] {
        let mut account = reference::Account;
        let mut session = Session::new(&mut account);
        let url = Url::from_file_path(Path::new(OsStr::from_bytes(bytes)), &mut session)
            .unwrap()
            .ok()
            .unwrap();
        assert_eq!(url.as_str(&mut session).unwrap(), expected);
        session.finish(Ok(())).unwrap();
    }
}

#[test]
fn relative_paths_reject_after_initial_serialization_admission() {
    struct Refuse;
    impl document::Admission for Refuse {
        type Error = ();
        fn admit(&mut self, _: document::Request) -> Result<(), ()> {
            Err(())
        }
    }
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    let result = Url::from_file_path(Path::new("relative"), &mut session);
    assert!(session.finish(result).unwrap().is_err());
    let mut account = Refuse;
    let mut session = Session::new(&mut account);
    let result = Url::from_file_path(Path::new("relative"), &mut session);
    assert!(result.is_err());
    assert!(Url::from_file_path(Path::new("/valid"), &mut session).is_err());
    assert!(session.finish(result).is_err());
}
