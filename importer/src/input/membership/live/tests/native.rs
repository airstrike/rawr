use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{content, Comparison};
use crate::input::membership::{live, observed, Budget};
use crate::{cargo, input, node};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rawr-live-file-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn acquire(path: &Path) -> live::Acquired {
    use std::os::unix::ffi::OsStrExt as _;
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    live::manifest(
        path.as_os_str().as_bytes(),
        &mut Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    )
    .unwrap()
}

#[test]
fn actual_regular_symlink_missing_and_non_directory_outcomes() {
    let fixture = Fixture::new();
    let path = fixture.0.join("Cargo.toml");
    fs::write(&path, b"name='fixture'\n").unwrap();
    let acquired = acquire(&path);
    assert_eq!(
        content(&acquired),
        (b"name='fixture'\n".as_slice(), observed::End::Eof)
    );
    assert_eq!(acquired.identity().comparison(), Comparison::Matched);

    let link = fixture.0.join("alias");
    std::os::unix::fs::symlink("Cargo.toml", &link).unwrap();
    let linked = acquire(&link);
    assert_eq!(content(&linked), content(&acquired));
    assert_eq!(linked.identity().comparison(), Comparison::Matched);
    use std::os::unix::ffi::OsStrExt as _;
    let observed::Query::File(query) = &linked.record().query else {
        panic!("file query")
    };
    assert_eq!(query.bytes(), link.as_os_str().as_bytes());

    let missing = acquire(&fixture.0.join("missing"));
    assert_eq!(missing.record().value, observed::Value::Absent);
    assert_eq!(missing.identity().opened(), None);
    let not_directory = acquire(&path.join("child"));
    assert_eq!(
        not_directory.record().value,
        observed::Value::Error(libc::ENOTDIR)
    );
}

#[test]
fn actual_directory_and_dev_null_keep_native_type_behavior() {
    let fixture = Fixture::new();
    let directory = acquire(&fixture.0);
    assert_eq!(
        directory.record().value,
        observed::Value::Error(libc::EISDIR)
    );
    let empty = acquire(Path::new("/dev/null"));
    assert_eq!(content(&empty), (b"".as_slice(), observed::End::Eof));
    let Some(live::Stat::Metadata(metadata)) = empty.identity().opened() else {
        panic!("device metadata")
    };
    assert_eq!(metadata.kind(), observed::Kind::Character);
}

#[test]
fn actual_non_utf8_filename_keeps_native_bytes_where_filesystem_allows_it() {
    use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
    let fixture = Fixture::new();
    let path = fixture
        .0
        .join(std::ffi::OsString::from_vec(b"native-\xff.toml".to_vec()));
    match fs::write(&path, b"x=1\n") {
        Ok(()) => {}
        Err(error)
            if matches!(
                error.raw_os_error(),
                Some(libc::EPERM | libc::EILSEQ)
            ) =>
        {
            // APFS rejects the invalid native spelling at creation (EILSEQ). Still
            // execute acquisition for that exact path and retain the OS result;
            // deterministic tests establish invalid-UTF8 data-path support.
            let acquired = acquire(&path);
            let observed::Query::File(query) = &acquired.record().query else {
                panic!("file query")
            };
            assert_eq!(query.bytes(), path.as_os_str().as_bytes());
            assert!(matches!(
                acquired.record().value,
                observed::Value::Absent | observed::Value::Error(_)
            ));
            eprintln!("non-UTF8 fixture creation was refused; exact native query still exercised");
            return;
        }
        Err(error) => panic!("create native filename fixture: {error}"),
    }
    let acquired = acquire(&path);
    assert_eq!(
        content(&acquired),
        (b"x=1\n".as_slice(), observed::End::Eof)
    );
    let observed::Query::File(query) = &acquired.record().query else {
        panic!("file query")
    };
    assert_eq!(query.bytes(), path.as_os_str().as_bytes());
}
