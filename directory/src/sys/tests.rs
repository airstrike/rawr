use std::ffi::CString;
use std::fs;
use std::os::fd::AsRawFd as _;
use std::os::unix::ffi::OsStrExt as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{getdirentries64, Directory};
use crate::tests::Audit;
use crate::{Reader, Record, Step, Work, BUFFER_BYTES};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rawr-directory-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn c_path(&self) -> CString {
        CString::new(self.0.as_os_str().as_bytes()).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn installed_abi_layout_widths_and_error_sentinel_match_declared_contract() {
    assert_eq!(std::mem::size_of::<libc::size_t>(), 8);
    assert_eq!(std::mem::size_of::<libc::off_t>(), 8);
    assert_eq!(std::mem::offset_of!(libc::dirent, d_ino), 0);
    assert_eq!(std::mem::offset_of!(libc::dirent, d_seekoff), 8);
    assert_eq!(std::mem::offset_of!(libc::dirent, d_reclen), 16);
    assert_eq!(std::mem::offset_of!(libc::dirent, d_namlen), 18);
    assert_eq!(std::mem::offset_of!(libc::dirent, d_type), 20);
    assert_eq!(std::mem::offset_of!(libc::dirent, d_name), 21);
    assert_eq!(std::mem::size_of::<libc::statfs>(), 2168);
    assert_eq!(std::mem::offset_of!(libc::statfs, f_flags), 64);
    assert_eq!(libc::MNT_UNION, 0x20);
    let mut buffer = [0; BUFFER_BYTES];
    let mut offset = 0;
    // SAFETY: valid initialized buffers, correct declared ABI, deliberately
    // invalid fd. The actual system call must reject the fd without retaining
    // either pointer; this tests the installed return width and errno contract.
    let result =
        unsafe { getdirentries64(-1, buffer.as_mut_ptr().cast(), buffer.len(), &mut offset) };
    let error = std::io::Error::last_os_error();
    assert_eq!(result, libc::size_t::MAX);
    assert_eq!(error.raw_os_error(), Some(libc::EBADF));
}

#[test]
fn real_filesystem_multiple_batches_native_names_and_eof_match_reference() {
    let fixture = Fixture::new();
    for index in 0..400 {
        fs::write(fixture.0.join(format!("entry-{index:04}-abcdefghijk")), []).unwrap();
    }
    fs::write(fixture.0.join("native-é-π"), []).unwrap();
    fs::create_dir(fixture.0.join("empty-directory")).unwrap();
    std::os::unix::fs::symlink("missing-target", fixture.0.join("symlink-entry")).unwrap();
    let expected = fs::read_dir(&fixture.0)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().as_bytes().to_vec())
        .collect::<std::collections::BTreeSet<_>>();
    let path = fixture.c_path();
    let mut audit = Audit::default();
    let names = crate::collect(&path, &mut [0; BUFFER_BYTES], &mut audit)
        .unwrap()
        .sorted(&mut audit)
        .unwrap();
    assert_eq!(
        names
            .iter()
            .map(|name| name.bytes().to_vec())
            .collect::<Vec<_>>(),
        expected.into_iter().collect::<Vec<_>>()
    );
    assert!(
        audit
            .events
            .iter()
            .filter(|event| matches!(event, Work::Read { .. }))
            .count()
            >= 3
    );
    assert!(audit.events.contains(&Work::Visible { entries: 403 }));
}

#[test]
fn independently_opened_streams_have_independent_positions_and_cloexec() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("unique"), []).unwrap();
    let path = fixture.c_path();
    let directory = Directory::open(&path).unwrap();
    // SAFETY: this private test borrows the live owned fd only for a read-only
    // flag query. The production interface never exposes the descriptor.
    let flags = unsafe { libc::fcntl(directory.0.as_raw_fd(), libc::F_GETFD) };
    assert!(flags >= 0);
    assert_ne!(flags & libc::FD_CLOEXEC, 0);
    let mut first = Reader::open(&path, &mut Audit::default()).unwrap();
    let mut second = Reader::open(&path, &mut Audit::default()).unwrap();
    let mut buffer = [0; BUFFER_BYTES];
    let mut first_names = Vec::new();
    let mut second_names = Vec::new();
    for (reader, names) in [
        (&mut first, &mut first_names),
        (&mut second, &mut second_names),
    ] {
        while let Step::Batch(mut batch) = reader.read(&mut buffer, &mut Audit::default()).unwrap()
        {
            while let Some(record) = batch.next(&mut Audit::default()).unwrap() {
                if let Record::Entry(entry) = record {
                    names.push(entry.name().to_vec());
                }
            }
        }
    }
    first_names.sort();
    second_names.sort();
    assert_eq!(first_names, second_names);
    assert!(first_names.iter().any(|name| name == b"unique"));
}

#[test]
fn real_open_failures_are_errors_and_final_symlinks_are_not_followed() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("file"), []).unwrap();
    std::os::unix::fs::symlink(&fixture.0, fixture.0.join("link")).unwrap();
    for child in ["missing", "file", "link"] {
        let path = CString::new(fixture.0.join(child).as_os_str().as_bytes()).unwrap();
        let mut audit = Audit::default();
        assert!(matches!(
            crate::collect(&path, &mut [0; BUFFER_BYTES], &mut audit),
            Err(crate::Error::Io { .. })
        ));
        assert_eq!(audit.events.len(), 1);
    }
}

#[test]
fn cargo_stream_stat_abi_and_following_open_match_installed_contract() {
    assert_eq!(std::mem::size_of::<libc::stat>(), 144);
    assert_eq!(std::mem::align_of::<libc::stat>(), 8);
    assert_eq!(std::mem::offset_of!(libc::stat, st_mode), 4);
    assert_eq!(std::mem::size_of::<libc::mode_t>(), 2);
    assert_eq!(libc::AT_SYMLINK_NOFOLLOW, 0x20);
    let _: unsafe extern "C" fn(
        libc::c_int,
        *const libc::c_char,
        *mut libc::stat,
        libc::c_int,
    ) -> libc::c_int = libc::fstatat;
    let fixture = Fixture::new();
    fs::create_dir(fixture.0.join("directory")).unwrap();
    std::os::unix::fs::symlink("directory", fixture.0.join("alias")).unwrap();
    let alias = CString::new(fixture.0.join("alias").as_os_str().as_bytes()).unwrap();
    assert!(Directory::open(&alias).is_err());
    let followed = Directory::open_follow(&alias).unwrap();
    assert_eq!(
        followed.entry_type(c".").unwrap() & libc::S_IFMT,
        libc::S_IFDIR
    );
    let opening = crate::Opening::open(&alias, &mut Audit::default()).unwrap();
    let mut stream = opening.finish(&mut Audit::default()).unwrap();
    assert!(stream.next(&mut Audit::default()).unwrap().is_none());
}

#[test]
fn cargo_entry_fallback_remains_anchored_across_rename_and_does_not_follow_links() {
    let fixture = Fixture::new();
    let before = fixture.0.join("before");
    fs::create_dir(&before).unwrap();
    fs::write(before.join("file"), b"").unwrap();
    fs::create_dir(before.join("dir")).unwrap();
    fs::write(before.join("removed"), b"").unwrap();
    std::os::unix::fs::symlink("absent", before.join("dangling")).unwrap();
    let path = CString::new(before.as_os_str().as_bytes()).unwrap();
    let original = Directory::open_follow(&path).unwrap();
    let mut enumeration = original.reopen().unwrap();
    // SAFETY: test-only read-only flag query against this live owned fd.
    let flags = unsafe { libc::fcntl(enumeration.0.as_raw_fd(), libc::F_GETFD) };
    assert_ne!(flags & libc::FD_CLOEXEC, 0);
    let mut first = [0; BUFFER_BYTES];
    let first_len = enumeration.read(&mut first).unwrap();
    assert!(first_len > 0);
    let mut another = original.reopen().unwrap();
    let mut second = [0; BUFFER_BYTES];
    let second_len = another.read(&mut second).unwrap();
    assert!(second_len > 0);
    let names = |bytes: &[u8]| {
        let mut batch = crate::Batch::new(bytes);
        let mut result = Vec::new();
        while let Some(record) = batch.next(&mut Audit::default()).unwrap() {
            if let Record::Entry(entry) = record {
                result.push((entry.name().to_vec(), entry.kind()));
            }
        }
        result
    };
    assert_eq!(names(&first[..first_len]), names(&second[..second_len]));
    let after = fixture.0.join("after");
    fs::rename(&before, &after).unwrap();
    fs::create_dir(&before).unwrap();
    // A path-based lookup would now inspect this conflicting directory.
    fs::create_dir(before.join("file")).unwrap();
    assert_eq!(
        original.entry_type(c"file").unwrap() & libc::S_IFMT,
        libc::S_IFREG
    );
    assert_eq!(
        original.entry_type(c"dir").unwrap() & libc::S_IFMT,
        libc::S_IFDIR
    );
    assert_eq!(
        original.entry_type(c"dangling").unwrap() & libc::S_IFMT,
        libc::S_IFLNK
    );
    fs::remove_file(after.join("removed")).unwrap();
    assert_eq!(
        original.entry_type(c"removed").unwrap_err().raw_os_error(),
        Some(libc::ENOENT)
    );
}

#[test]
fn cargo_stream_native_encounter_order_and_types_match_std() {
    let fixture = Fixture::new();
    for name in ["z.rs", "a.rs", ".hidden"] {
        fs::write(fixture.0.join(name), b"").unwrap();
    }
    fs::create_dir(fixture.0.join("subdir")).unwrap();
    std::os::unix::fs::symlink("missing", fixture.0.join("link")).unwrap();
    let expected = fs::read_dir(&fixture.0)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().as_bytes().to_vec(),
                entry.file_type().unwrap().is_dir(),
                entry.file_type().unwrap().is_symlink(),
            )
        })
        .collect::<Vec<_>>();
    let mut audit = Audit::default();
    let mut stream = crate::Opening::open(&fixture.c_path(), &mut audit)
        .unwrap()
        .finish(&mut audit)
        .unwrap();
    let mut actual = Vec::new();
    while let Some(entry) = stream.next(&mut audit).unwrap() {
        let name = entry.name().to_vec();
        let kind = entry.file_type(&mut audit).unwrap().unwrap();
        actual.push((
            name,
            kind == crate::FileType::Directory,
            kind == crate::FileType::Symlink,
        ));
    }
    assert_eq!(actual, expected);
}
