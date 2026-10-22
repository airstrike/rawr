use nix::sys::stat::{self, SFlag};

use super::{metadata, raw_with};
use crate::input::membership::{live::Stat, observed};

#[test]
fn errno_capture_preserves_codes_outside_nix_enum() {
    let result = raw_with::<()>(Err(nix::errno::Errno::UnknownErrno), || 107);
    assert_eq!(result, Err(107));
    assert_eq!(
        raw_with(Ok(17), || panic!("errno must not be read on success")),
        Ok(17)
    );
    // This exercises the same immediate-error extraction helper without unsafe
    // writes to thread-local errno. Native error107 availability is not assumed.
}

#[test]
fn every_native_type_uses_exact_masked_encoding() {
    // Obtain an initialized native stat safely; mutate only copied value fields.
    let original = stat::stat(c"/dev/null").unwrap();
    for (kind, expected) in [
        (SFlag::S_IFREG.bits(), observed::Kind::File),
        (SFlag::S_IFDIR.bits(), observed::Kind::Directory),
        (SFlag::S_IFLNK.bits(), observed::Kind::Symlink),
        (SFlag::S_IFIFO.bits(), observed::Kind::Fifo),
        (SFlag::S_IFSOCK.bits(), observed::Kind::Socket),
        (SFlag::S_IFCHR.bits(), observed::Kind::Character),
        (SFlag::S_IFBLK.bits(), observed::Kind::Block),
        (0, observed::Kind::Unknown),
    ] {
        let mut native = original;
        native.st_mode = kind | 0o6754;
        let Stat::Metadata(value) = metadata(Ok(native)) else {
            panic!("metadata")
        };
        assert_eq!(value.kind(), expected);
        assert_eq!(value.mode(), u32::from(native.st_mode));
    }
}

#[test]
fn raw_signed_metadata_is_retained_without_optional_hint_rejection() {
    let mut native = stat::stat(c"/dev/null").unwrap();
    native.st_dev = -1;
    native.st_size = -2;
    native.st_mtime = -3;
    native.st_mtime_nsec = -4;
    native.st_ctime = -5;
    native.st_ctime_nsec = 1_000_000_000;
    let Stat::Metadata(value) = metadata(Ok(native)) else {
        panic!("metadata")
    };
    assert_eq!(value.device(), u64::MAX);
    assert_eq!(value.length(), u64::MAX - 1);
    assert_eq!(value.inode(), native.st_ino);
    assert_eq!(value.owner(), native.st_uid);
    assert_eq!(value.modified_seconds(), -3);
    assert_eq!(value.modified_nanos(), -4);
    assert_eq!(value.changed_seconds(), -5);
    assert_eq!(value.changed_nanos(), 1_000_000_000);
    assert_eq!(metadata(Err(107)), Stat::Error(107));
}
