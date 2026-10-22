#[cfg(all(target_os = "macos", target_pointer_width = "64"))]
#[test]
fn native_results_match_pinned_std() {
    use std::ffi::{CString, OsStr};
    use std::os::unix::ffi::OsStrExt as _;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};

    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn equivalent(path: &Path) {
        let input = CString::new(path.as_os_str().as_bytes()).expect("fixture has no NUL");
        let mut buffer = [255; crate::BUFFER_BYTES];
        let actual = crate::canonicalize(&input, &mut buffer);
        let expected = std::fs::canonicalize(path);
        match (actual, expected) {
            (Ok(actual), Ok(expected)) => {
                assert_eq!(actual.to_bytes(), expected.as_os_str().as_bytes())
            }
            (Err(crate::Error::Native(actual)), Err(expected)) => {
                assert!(actual.raw_os_error().is_some());
                assert_eq!(actual.raw_os_error(), expected.raw_os_error());
            }
            (actual, expected) => {
                panic!("canonicalization mismatch for {path:?}: {actual:?} versus {expected:?}")
            }
        }
    }

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("rawr-canonical-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&root).expect("create fixture root");
    let fixture = Fixture(root);
    assert!(fixture.0.is_absolute());
    std::fs::create_dir(fixture.0.join("nested")).expect("create nested directory");
    std::fs::write(fixture.0.join("native-é"), b"fixture").expect("write native-byte file");
    std::fs::write(fixture.0.join("MiXeD-Case"), b"case fixture").expect("write case fixture");
    symlink("native-é", fixture.0.join("link")).expect("create relative symlink");
    symlink("absent", fixture.0.join("dangling")).expect("create dangling symlink");
    symlink("loop", fixture.0.join("loop")).expect("create loop");

    // One test owns all live native calls, with no process-CWD mutation. This
    // does not certify concurrent callers elsewhere in a production process.
    equivalent(Path::new("/"));
    equivalent(Path::new("."));
    equivalent(&fixture.0);
    equivalent(&fixture.0.join("MiXeD-Case"));
    // Success and native failure are both compared to std: the fixture does
    // not assume the mounted filesystem is case-sensitive or insensitive.
    equivalent(&fixture.0.join("mixed-case"));
    equivalent(&fixture.0.join("nested/../native-é"));
    equivalent(&fixture.0.join("link"));
    equivalent(&fixture.0.join("link/"));
    equivalent(&fixture.0.join("absent"));
    equivalent(&fixture.0.join("dangling"));
    equivalent(&fixture.0.join("loop"));
    equivalent(&fixture.0.join("native-é/child"));
    equivalent(&fixture.0.join(OsStr::from_bytes(b"missing-\xff-\xfe")));
    let native = fixture.0.join(OsStr::from_bytes(b"existing-\xff-\xfe"));
    match std::fs::write(&native, b"native fixture") {
        Ok(()) => {
            assert!(
                std::fs::canonicalize(&native).is_ok(),
                "existing native-byte fixture must canonicalize"
            );
            equivalent(&native);
        }
        // The earlier directory fixture on this host established EPERM (1)
        // for invalid-UTF8 creation. Preserve the explicit coverage limitation;
        // no other creation failure is treated as an expected fixture refusal.
        Err(error) if error.raw_os_error() == Some(1) => {
            eprintln!("native-byte creation refused: errno=1 ({error}); successful non-UTF8 filename coverage NOT exercised");
        }
        Err(error) => panic!("unexpected native-byte fixture creation failure: {error}"),
    }
    equivalent(&fixture.0.join("x".repeat(crate::BUFFER_BYTES * 2)));

    // Native failure is returned without inspecting its partial output. Empty
    // input is another native outcome, not an adapter-created input policy.
    equivalent(Path::new(""));

    // The borrowed output does not borrow the input C string.
    let mut buffer = [255; crate::BUFFER_BYTES];
    let output = {
        let input = CString::new("/").expect("root C string");
        crate::canonicalize(&input, &mut buffer).expect("root canonicalizes")
    };
    assert_eq!(output.to_bytes(), b"/");
}

#[cfg(not(all(target_os = "macos", target_pointer_width = "64")))]
#[test]
fn unsupported_target_is_explicit() {
    let mut buffer = [255; crate::BUFFER_BYTES];
    assert!(matches!(
        crate::canonicalize(c"/", &mut buffer),
        Err(crate::Error::Unsupported)
    ));
    assert_eq!(buffer, [255; crate::BUFFER_BYTES]);
}
#[cfg(all(target_os = "macos", target_pointer_width = "64"))]
mod cwd;
