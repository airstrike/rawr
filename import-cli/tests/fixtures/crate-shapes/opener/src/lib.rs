//! A crate of functions of open's shape, as `open::that_in_background` is:
//! an `impl AsRef<OsStr>` argument and a `std::thread::JoinHandle` result.
//! A program that names only `that_if_given`, which rawr can't use yet,
//! has a crate none of whose items it can use.

use std::ffi::OsStr;
use std::io;
use std::thread::{self, JoinHandle};

/// Opens `path` on another thread, which says whether it was opened.
pub fn that_in_background(path: impl AsRef<OsStr>) -> JoinHandle<io::Result<()>> {
    let path = path.as_ref().to_os_string();
    thread::spawn(move || {
        if path.is_empty() {
            Err(io::Error::other("no path"))
        } else {
            Ok(())
        }
    })
}

/// Opens `path` on another thread when there is one.
pub fn that_if_given(path: Option<&str>) -> Option<JoinHandle<io::Result<()>>> {
    path.map(that_in_background)
}
