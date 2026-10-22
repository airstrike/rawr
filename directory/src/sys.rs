//! The entire production unsafe boundary. See ../SAFETY.md for ABI provenance.

use std::ffi::CStr;
use std::io;

#[cfg(all(target_os = "macos", target_pointer_width = "64"))]
use std::os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd};

#[cfg(all(target_os = "macos", target_pointer_width = "64"))]
pub(super) struct Directory(OwnedFd);

#[cfg(not(all(target_os = "macos", target_pointer_width = "64")))]
pub(super) struct Directory;

#[cfg(all(target_os = "macos", target_pointer_width = "64"))]
unsafe extern "C" {
    // Apple Libc telldir.h declares size_t, not int. SIZE_MAX is the -1 error
    // sentinel. Mach-O adds the third leading underscore in the export table.
    #[link_name = "__getdirentries64"]
    fn getdirentries64(
        fd: libc::c_int,
        buffer: *mut libc::c_void,
        bytes: libc::size_t,
        position: *mut libc::off_t,
    ) -> libc::size_t;
}

#[cfg(all(target_os = "macos", target_pointer_width = "64"))]
impl Directory {
    pub(super) fn open(path: &CStr) -> io::Result<Self> {
        Self::open_with(path, libc::O_NOFOLLOW)
    }

    pub(super) fn open_follow(path: &CStr) -> io::Result<Self> {
        Self::open_with(path, 0)
    }

    fn open_with(path: &CStr, follow: libc::c_int) -> io::Result<Self> {
        // SAFETY: CStr supplies a live terminated path. O_CREAT is absent, so
        // open's variadic mode argument is not required. The returned fd is a
        // fresh open file description, never a dup of unrelated lookup authority.
        let fd = unsafe {
            libc::open(
                path.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | follow,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful open gave unique ownership of this valid fd. No
        // other owner or externally visible raw descriptor is created.
        Ok(Self(unsafe { OwnedFd::from_raw_fd(fd) }))
    }

    pub(super) fn reopen(&self) -> io::Result<Self> {
        // SAFETY: the private live owned descriptor supplies the directory;
        // the only relative name is this fixed terminated '.'. No fd or general
        // lookup service escapes. This creates an independent file description
        // so union enumeration cannot retarget the original lookup directory.
        let fd = unsafe {
            libc::openat(
                self.0.as_raw_fd(),
                c".".as_ptr(),
                libc::O_RDONLY | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful openat returns this uniquely owned descriptor.
        Ok(Self(unsafe { OwnedFd::from_raw_fd(fd) }))
    }

    pub(super) fn entry_type(&self, name: &CStr) -> io::Result<libc::mode_t> {
        let mut stat = std::mem::MaybeUninit::<libc::stat>::zeroed();
        // SAFETY: the name is a framed single native entry of this stream,
        // terminated in caller-owned fixed storage. The lookup descriptor is
        // alive and never exported. stat points to correctly aligned writable
        // storage of libc's exact platform type. The call retains no pointers.
        if unsafe {
            libc::fstatat(
                self.0.as_raw_fd(),
                name.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: all stat fields are integer values; initial zeroes and the
        // successful syscall initialize them. Only st_mode is inspected.
        Ok(unsafe { stat.assume_init() }.st_mode)
    }

    pub(super) fn union(&self) -> io::Result<bool> {
        let mut stat = std::mem::MaybeUninit::<libc::statfs>::zeroed();
        // SAFETY: our live descriptor is valid, and stat is writable storage for
        // precisely the target libc statfs ABI. Nothing reads it on failure.
        if unsafe { libc::fstatfs(self.0.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: statfs consists solely of integer fields and integer arrays,
        // so its initial zero bits and any fields written by fstatfs are valid.
        // This does not assume the syscall overwrites reserved fields/padding.
        let stat = unsafe { stat.assume_init() };
        Ok(stat.f_flags & (libc::MNT_UNION as u32) != 0)
    }

    pub(super) fn read(&mut self, buffer: &mut crate::Buffer) -> io::Result<usize> {
        let mut position: libc::off_t = 0;
        // SAFETY: the uniquely borrowed initialized array is writable for its
        // exact length; position is correctly aligned live off_t storage. This
        // call retains neither pointer. The descriptor remains owned throughout.
        let bytes = unsafe {
            getdirentries64(
                self.0.as_raw_fd(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut position,
            )
        };
        if bytes == libc::size_t::MAX {
            Err(io::Error::last_os_error())
        } else {
            Ok(bytes)
        }
    }
}

#[cfg(all(test, target_os = "macos", target_pointer_width = "64"))]
mod tests;

// The importer already constrains its compiler host. Keeping this module
// buildable elsewhere does not turn an unimplemented ABI into a directory proof.
#[cfg(not(all(target_os = "macos", target_pointer_width = "64")))]
impl Directory {
    pub(super) fn open(_path: &CStr) -> io::Result<Self> {
        Err(io::ErrorKind::Unsupported.into())
    }

    pub(super) fn open_follow(_path: &CStr) -> io::Result<Self> {
        Err(io::ErrorKind::Unsupported.into())
    }
    pub(super) fn reopen(&self) -> io::Result<Self> {
        Err(io::ErrorKind::Unsupported.into())
    }
    pub(super) fn entry_type(&self, _name: &CStr) -> io::Result<libc::mode_t> {
        Err(io::ErrorKind::Unsupported.into())
    }

    pub(super) fn union(&self) -> io::Result<bool> {
        Err(io::ErrorKind::Unsupported.into())
    }

    pub(super) fn read(&mut self, _buffer: &mut crate::Buffer) -> io::Result<usize> {
        Err(io::ErrorKind::Unsupported.into())
    }
}
