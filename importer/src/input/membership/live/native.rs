use std::collections::BTreeMap;
use std::ffi::CStr;
use std::os::fd::{AsRawFd as _, OwnedFd};
use std::sync::{Mutex, PoisonError};

use rustix::fs::{self as stat, Mode, OFlags};

use super::{canonical, discovery, file, git_read, Metadata, Stat};
use crate::input::membership::{observed, Result};

pub(super) struct System;

/// The descriptors this module opened, by number. The `System` traits pass
/// descriptor numbers; the owned descriptor stays here until `close`.
static OPEN: Mutex<BTreeMap<i32, OwnedFd>> = Mutex::new(BTreeMap::new());

fn with<T>(
    descriptor: i32,
    operation: impl FnOnce(&OwnedFd) -> rustix::io::Result<T>,
) -> std::result::Result<T, i32> {
    let open = OPEN.lock().unwrap_or_else(PoisonError::into_inner);
    let owned = open.get(&descriptor).ok_or(libc::EBADF)?;
    raw(operation(owned))
}

impl canonical::System for System {
    fn canonicalize<'buffer>(
        &mut self,
        path: &CStr,
        buffer: &'buffer mut rawr_canonical::Buffer,
    ) -> std::result::Result<&'buffer CStr, rawr_canonical::Error> {
        rawr_canonical::canonicalize(path, buffer)
    }
}

impl file::System for System {
    fn stat(&mut self, path: &CStr) -> Result<Stat> {
        Ok(metadata(raw(stat::stat(path))))
    }

    fn open(&mut self, path: &CStr) -> std::result::Result<i32, i32> {
        let owned = raw(stat::open(
            path,
            OFlags::RDONLY | OFlags::CLOEXEC,
            Mode::from_bits_truncate(0o666),
        ))?;
        let descriptor = owned.as_raw_fd();
        OPEN.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(descriptor, owned);
        Ok(descriptor)
    }

    fn fstat(&mut self, descriptor: i32) -> Result<Stat> {
        Ok(metadata(with(descriptor, |owned| stat::fstat(owned))))
    }

    fn read(&mut self, descriptor: i32, bytes: &mut [u8]) -> std::result::Result<usize, i32> {
        with(descriptor, |owned| rustix::io::read(owned, bytes))
    }

    fn close(&mut self, descriptor: i32) {
        // Match std's one-shot close/drop behavior: dropping closes once.
        drop(
            OPEN.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&descriptor),
        );
    }
}

impl discovery::System for System {
    fn open(&mut self, path: &CStr) -> std::result::Result<i32, i32> {
        <Self as file::System>::open(self, path)
    }

    fn fstat(&mut self, descriptor: i32) -> Result<Stat> {
        <Self as file::System>::fstat(self, descriptor)
    }

    fn stat(&mut self, path: &CStr) -> Result<Stat> {
        <Self as file::System>::stat(self, path)
    }

    fn close(&mut self, descriptor: i32) {
        <Self as file::System>::close(self, descriptor);
    }
}

impl git_read::System for System {
    fn position(&mut self, descriptor: i32) -> std::result::Result<u64, i32> {
        // Pinned File::stream_position is one lseek(SEEK_CUR), without retry.
        with(descriptor, |owned| rustix::fs::seek(owned, rustix::fs::SeekFrom::Current(0)))
    }
}

/// rustix keeps the native errno, Darwin's 107 included.
fn raw<T>(result: rustix::io::Result<T>) -> std::result::Result<T, i32> {
    result.map_err(rustix::io::Errno::raw_os_error)
}

fn metadata(result: std::result::Result<stat::Stat, i32>) -> Stat {
    let metadata = match result {
        Ok(metadata) => metadata,
        Err(code) => return Stat::Error(code),
    };
    let mode = metadata.st_mode as u32;
    let kind = match metadata.st_mode & libc::S_IFMT {
        libc::S_IFREG => observed::Kind::File,
        libc::S_IFDIR => observed::Kind::Directory,
        libc::S_IFLNK => observed::Kind::Symlink,
        libc::S_IFIFO => observed::Kind::Fifo,
        libc::S_IFSOCK => observed::Kind::Socket,
        libc::S_IFCHR => observed::Kind::Character,
        libc::S_IFBLK => observed::Kind::Block,
        _ => observed::Kind::Unknown,
    };
    Stat::Metadata(Metadata {
        kind,
        // These casts match pinned std MetadataExt, including signed dev_t.
        device: metadata.st_dev as u64,
        inode: metadata.st_ino,
        owner: metadata.st_uid,
        mode,
        length: metadata.st_size as u64,
        modified_seconds: metadata.st_mtime,
        modified_nanos: metadata.st_mtime_nsec,
        changed_seconds: metadata.st_ctime,
        changed_nanos: metadata.st_ctime_nsec,
    })
}

#[cfg(test)]
mod tests;
