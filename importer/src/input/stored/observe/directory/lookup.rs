//! Independently owned lookup association; never the union enumeration stream.

use std::ffi::CStr;

use rustix::fs::{self as stat, AtFlags, Mode, OFlags};

use super::super::super::{Budget, Result};
use super::super::{io, Error};

pub(super) struct Lookup {
    descriptor: Descriptor,
    identity: Identity,
}

/// Closed once on drop, never retried on a possibly reused number.
struct Descriptor(std::os::fd::OwnedFd);

#[derive(Clone, Copy, Eq, PartialEq)]
struct Identity {
    device: u64,
    inode: u64,
}

impl Identity {
    // dev_t is i32 on macOS and u64 on Linux.
    #[allow(clippy::unnecessary_cast)]
    fn from_stat(value: &stat::Stat) -> Self {
        Self {
            device: value.st_dev as u64,
            inode: value.st_ino,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Metadata {
    mode: libc::mode_t,
    length: u64,
}

impl Metadata {
    fn from_stat(value: &stat::Stat) -> Self {
        Self {
            mode: value.st_mode,
            length: value.st_size as u64,
        }
    }

    pub(super) fn is_dir(self) -> bool {
        self.mode & libc::S_IFMT == libc::S_IFDIR
    }
    pub(super) fn is_file(self) -> bool {
        self.mode & libc::S_IFMT == libc::S_IFREG
    }
    pub(super) fn is_symlink(self) -> bool {
        self.mode & libc::S_IFMT == libc::S_IFLNK
    }
    pub(super) const fn len(self) -> u64 {
        self.length
    }

    #[cfg(test)]
    pub(super) fn from_metadata(value: &std::fs::Metadata) -> Self {
        use std::os::unix::fs::MetadataExt as _;
        Self {
            mode: value.mode() as libc::mode_t,
            length: value.len(),
        }
    }
}

impl Lookup {
    pub(super) fn open(path: &CStr, budget: &mut Budget<'_>) -> Result<Self> {
        budget.paths(1)?;
        let descriptor = loop {
            budget.paths(1)?;
            match raw(stat::open(
                path,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                Mode::empty(),
            )) {
                Ok(descriptor) => break Descriptor(descriptor),
                Err(error) if error.raw_os_error() == Some(libc::EINTR) => {}
                Err(error) => return Err(io("open independent input lookup directory", error)),
            }
        };
        budget.paths(1)?;
        let metadata = raw(stat::fstat(&descriptor.0))
            .map_err(|error| io("inspect input lookup directory", error))?;
        if !Metadata::from_stat(&metadata).is_dir() {
            return Err(Error::Drift("lookup descriptor is not a directory").into());
        }
        Ok(Self {
            descriptor,
            identity: Identity::from_stat(&metadata),
        })
    }

    pub(super) fn revalidate(&self, path: &CStr, budget: &mut Budget<'_>) -> Result<()> {
        budget.paths(1)?;
        let metadata = raw(stat::lstat(path))
            .map_err(|error| io("recheck input enumeration association", error))?;
        if !Metadata::from_stat(&metadata).is_dir()
            || Identity::from_stat(&metadata) != self.identity
        {
            return Err(Error::Drift("input enumeration directory association changed").into());
        }
        Ok(())
    }

    pub(super) fn metadata(&self, name: &CStr, budget: &mut Budget<'_>) -> Result<Metadata> {
        budget.paths(1)?;
        let metadata = raw(stat::statat(
            &self.descriptor.0,
            name,
            AtFlags::SYMLINK_NOFOLLOW,
        ))
        .map_err(|error| io("inspect anchored input member", error))?;
        Ok(Metadata::from_stat(&metadata))
    }
}

/// rustix keeps the native errno, Darwin's 107 included.
fn raw<T>(result: rustix::io::Result<T>) -> std::io::Result<T> {
    result.map_err(|error| std::io::Error::from_raw_os_error(error.raw_os_error()))
}
