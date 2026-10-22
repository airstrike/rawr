//! Development evidence: relative operations retain the actual cwd object.
//! A child owns all cwd mutation; this is not native allocation qualification.
use std::{ffi::CString, os::unix::ffi::OsStrExt, path::Path};

fn equivalent(path: &str) -> std::io::Result<std::path::PathBuf> {
    let input = CString::new(path).expect("fixture spelling");
    let mut buffer = [255; crate::BUFFER_BYTES];
    let actual = crate::canonicalize(&input, &mut buffer);
    let expected = std::fs::canonicalize(path);
    match (&actual, &expected) {
        (Ok(actual), Ok(expected)) => {
            assert_eq!(actual.to_bytes(), expected.as_os_str().as_bytes());
        }
        (Err(crate::Error::Native(actual)), Err(expected)) => {
            assert!(actual.raw_os_error().is_some());
            assert_eq!(actual.raw_os_error(), expected.raw_os_error());
        }
        _ => panic!("{path}: adapter {actual:?}, std {expected:?}"),
    }
    expected
}

#[test]
fn relative_cwd_lifecycle_matches_pinned_std() {
    const CHILD: &str = "RAWR_CANONICAL_CWD_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let root = std::env::current_dir().expect("child root");
        std::fs::create_dir("original").expect("original cwd");
        std::fs::create_dir("original/nested").expect("nested");
        std::fs::write("original/target", b"original").expect("original file");
        std::os::unix::fs::symlink("target", "original/custom.json").expect("relative link");
        std::os::unix::fs::symlink("/", "original/root.json").expect("root link");
        std::env::set_current_dir("original").expect("enter cwd");
        assert_eq!(
            equivalent("custom.json").unwrap(),
            root.join("original/target")
        );
        assert_eq!(
            equivalent("nested/../custom.json").unwrap(),
            root.join("original/target")
        );
        assert_eq!(equivalent("root.json").unwrap(), Path::new("/"));
        assert!(equivalent("missing.json").is_err());

        std::fs::rename(root.join("original"), root.join("moved")).expect("rename live cwd");
        std::fs::create_dir(root.join("original")).expect("replace old pathname");
        std::fs::write(root.join("original/target"), b"replacement").expect("replacement file");
        assert_eq!(
            equivalent("custom.json").unwrap(),
            root.join("moved/target")
        );
        assert_eq!(equivalent(".").unwrap(), root.join("moved"));

        // An unlinked cwd has no reconstructible pathname. Compare the reached
        // native error rather than assigning a portable errno in advance.
        std::env::set_current_dir(&root).expect("leave moved cwd");
        std::fs::create_dir("unlinked").expect("unlink fixture");
        std::env::set_current_dir("unlinked").expect("enter unlink fixture");
        std::fs::remove_dir(root.join("unlinked")).expect("unlink live cwd");
        assert!(equivalent(".").is_err());
        assert!(equivalent("missing.json").is_err());
        std::env::set_current_dir(root).expect("restore child cwd");
        return;
    }

    struct Fixture(std::path::PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = Fixture(
        std::env::temp_dir().join(format!("rawr-canonical-cwd-{}-{nonce}", std::process::id())),
    );
    std::fs::create_dir(&root.0).expect("exclusive fixture root");
    let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "--exact",
            "tests::cwd::relative_cwd_lifecycle_matches_pinned_std",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .current_dir(&root.0)
        .output()
        .expect("isolated child");
    assert!(
        output.status.success(),
        "child failed: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"),
        "the child must execute the selected test, not merely exit successfully"
    );
}
