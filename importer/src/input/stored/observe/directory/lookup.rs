//! Independently owned lookup association; never the union enumeration stream.

use std::ffi::CStr;

use nix::fcntl::{self, AtFlags, OFlag};
use nix::sys::stat::{self, SFlag};
use nix::unistd;

use super::super::super::{Budget, Result};
use super::super::{io, Error};

pub(super) struct Lookup {
    descriptor: Descriptor,
    identity: Identity,
}

struct Descriptor(i32);

impl Drop for Descriptor {
    fn drop(&mut self) {
        // Reserved before open. Never retry close on a possibly reused number.
        let _ = raw(unistd::close(self.0));
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct Identity {
    device: u64,
    inode: u64,
}

impl Identity {
    fn from_stat(value: &stat::FileStat) -> Self {
        Self {
            device: value.st_dev as u64,
            inode: value.st_ino,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Metadata {
    mode: nix::libc::mode_t,
    length: u64,
}

impl Metadata {
    fn from_stat(value: &stat::FileStat) -> Self {
        Self {
            mode: value.st_mode,
            length: value.st_size as u64,
        }
    }

    pub(super) fn is_dir(self) -> bool {
        self.mode & SFlag::S_IFMT.bits() == SFlag::S_IFDIR.bits()
    }
    pub(super) fn is_file(self) -> bool {
        self.mode & SFlag::S_IFMT.bits() == SFlag::S_IFREG.bits()
    }
    pub(super) fn is_symlink(self) -> bool {
        self.mode & SFlag::S_IFMT.bits() == SFlag::S_IFLNK.bits()
    }
    pub(super) const fn len(self) -> u64 {
        self.length
    }

    #[cfg(test)]
    pub(super) fn from_metadata(value: &std::fs::Metadata) -> Self {
        use std::os::unix::fs::MetadataExt as _;
        Self {
            mode: value.mode() as nix::libc::mode_t,
            length: value.len(),
        }
    }
}

impl Lookup {
    pub(super) fn open(path: &CStr, budget: &mut Budget<'_>) -> Result<Self> {
        budget.paths(1)?;
        let descriptor = loop {
            budget.paths(1)?;
            match raw(fcntl::open(
                path,
                OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
                stat::Mode::empty(),
            )) {
                Ok(descriptor) => break Descriptor(descriptor),
                Err(error) if error.raw_os_error() == Some(nix::libc::EINTR) => {}
                Err(error) => return Err(io("open independent input lookup directory", error)),
            }
        };
        budget.paths(1)?;
        let metadata = raw(stat::fstat(descriptor.0))
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
        let metadata = raw(stat::fstatat(
            self.descriptor.0,
            name,
            AtFlags::AT_SYMLINK_NOFOLLOW,
        ))
        .map_err(|error| io("inspect anchored input member", error))?;
        Ok(Metadata::from_stat(&metadata))
    }
}

fn raw<T>(result: nix::Result<T>) -> std::io::Result<T> {
    match result {
        Ok(value) => Ok(value),
        // Capture native errno before callbacks or descriptor cleanup. The
        // pinned nix enum cannot represent every Darwin error, including107.
        Err(_) => Err(std::io::Error::from_raw_os_error(nix::errno::errno())),
    }
}
