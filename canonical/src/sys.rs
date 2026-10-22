//! The only unsafe boundary. Returned native pointers are compared, not read.

use std::ffi::CStr;

use crate::{Buffer, Error};

#[cfg(any(test, all(target_os = "macos", target_pointer_width = "64")))]
use crate::Framing;
#[cfg(any(test, all(target_os = "macos", target_pointer_width = "64")))]
use std::ffi::c_char;

#[cfg(all(target_os = "macos", target_pointer_width = "64"))]
unsafe extern "C" {
    // This is the exact Apple variant selected by pinned Rust std's libc.
    #[link_name = "realpath$DARWIN_EXTSN"]
    fn realpath(path: *const c_char, resolved: *mut c_char) -> *mut c_char;
}

#[cfg(all(target_os = "macos", target_pointer_width = "64"))]
pub(super) fn canonicalize<'b>(path: &CStr, buffer: &'b mut Buffer) -> Result<&'b CStr, Error> {
    let expected = buffer.as_mut_ptr().cast();
    // SAFETY: path is a live NUL-terminated C string. The distinct mutable
    // buffer is initialized and writable for Darwin PATH_MAX bytes. The native
    // caller-buffer contract retains neither pointer, writes within that extent,
    // and returns its base or NULL. No pointer returned by native code is ever
    // dereferenced here. Installed-contract qualification remains a release gate.
    let returned = unsafe { realpath(path.as_ptr(), expected) };
    if returned.is_null() {
        // Capture errno immediately, before framing or any other native call.
        return Err(Error::Native(std::io::Error::last_os_error()));
    }
    frame(buffer, expected, returned).map_err(Error::Framing)
}

#[cfg(not(all(target_os = "macos", target_pointer_width = "64")))]
pub(super) fn canonicalize<'b>(_path: &CStr, _buffer: &'b mut Buffer) -> Result<&'b CStr, Error> {
    Err(Error::Unsupported)
}

#[cfg(any(test, all(target_os = "macos", target_pointer_width = "64")))]
fn frame(buffer: &Buffer, expected: *mut c_char, returned: *mut c_char) -> Result<&CStr, Framing> {
    if returned != expected {
        return Err(Framing::Pointer);
    }
    CStr::from_bytes_until_nul(buffer).map_err(|_| Framing::Termination)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_pointer_before_bounded_termination() {
        let mut buffer = [255; crate::BUFFER_BYTES];
        let expected = buffer.as_mut_ptr().cast();
        let mut other = [0_u8; 1];
        assert_eq!(
            frame(&buffer, expected, other.as_mut_ptr().cast()),
            Err(Framing::Pointer)
        );
        assert_eq!(
            frame(&buffer, expected, std::ptr::null_mut()),
            Err(Framing::Pointer)
        );
        assert_eq!(
            frame(&buffer, expected, expected),
            Err(Framing::Termination)
        );
        buffer[crate::BUFFER_BYTES - 1] = 0;
        assert_eq!(
            frame(&buffer, expected, expected)
                .expect("last-byte terminator")
                .to_bytes()
                .len(),
            crate::BUFFER_BYTES - 1
        );
        buffer[0] = b'/';
        buffer[1] = 0;
        assert_eq!(
            frame(&buffer, expected, expected)
                .expect("bounded prefix")
                .to_bytes(),
            b"/"
        );
    }
}
