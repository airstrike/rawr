use std::ffi::CStr;

use nix::fcntl::{self, OFlag};
use nix::sys::stat::{self, SFlag};
use nix::unistd;

use super::{canonical, discovery, file, git_read, Metadata, Stat};
use crate::input::membership::{observed, Result};

pub(super) struct System;

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
        raw(fcntl::open(
            path,
            OFlag::O_RDONLY | OFlag::O_CLOEXEC,
            stat::Mode::from_bits_truncate(0o666),
        ))
    }

    fn fstat(&mut self, descriptor: i32) -> Result<Stat> {
        Ok(metadata(raw(stat::fstat(descriptor))))
    }

    fn read(&mut self, descriptor: i32, bytes: &mut [u8]) -> std::result::Result<usize, i32> {
        raw(unistd::read(descriptor, bytes))
    }

    fn close(&mut self, descriptor: i32) {
        // Match std's one-shot close/drop behavior. Do not retry close errors.
        let _ = raw(unistd::close(descriptor));
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
        raw(unistd::lseek(descriptor, 0, unistd::Whence::SeekCur)).map(|position| position as u64)
    }
}

fn raw<T>(result: nix::Result<T>) -> std::result::Result<T, i32> {
    raw_with(result, nix::errno::errno)
}

fn raw_with<T>(result: nix::Result<T>, errno: fn() -> i32) -> std::result::Result<T, i32> {
    match result {
        Ok(value) => Ok(value),
        // Read native errno immediately. Casting nix::Errno would lose Darwin
        // error 107, which this pinned enum maps to UnknownErrno.
        Err(_) => Err(errno()),
    }
}

fn metadata(result: std::result::Result<stat::FileStat, i32>) -> Stat {
    let metadata = match result {
        Ok(metadata) => metadata,
        Err(code) => return Stat::Error(code),
    };
    let mode = metadata.st_mode as u32;
    let kind = match metadata.st_mode & SFlag::S_IFMT.bits() {
        mode if mode == SFlag::S_IFREG.bits() => observed::Kind::File,
        mode if mode == SFlag::S_IFDIR.bits() => observed::Kind::Directory,
        mode if mode == SFlag::S_IFLNK.bits() => observed::Kind::Symlink,
        mode if mode == SFlag::S_IFIFO.bits() => observed::Kind::Fifo,
        mode if mode == SFlag::S_IFSOCK.bits() => observed::Kind::Socket,
        mode if mode == SFlag::S_IFCHR.bits() => observed::Kind::Character,
        mode if mode == SFlag::S_IFBLK.bits() => observed::Kind::Block,
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
