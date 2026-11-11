//! A crate of functions of open's shape, as `open::that_in_background` is:
//! an `impl AsRef<OsStr>` argument and a `std::thread::JoinHandle` result.
//! Others give a handle in a tuple or an `Option`, take one, give one of a
//! crate's value, or hold one of any type. A program that names only
//! `number_of`, which rawr can't use yet, has a crate none of whose items
//! it uses.

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

/// Opens each of two paths on a thread of its own.
pub fn both_in_background(
    first: &str,
    second: &str,
) -> (JoinHandle<io::Result<()>>, JoinHandle<io::Result<()>>) {
    (that_in_background(first), that_in_background(second))
}

/// Whether `value` is positive, on another thread.
pub fn check_in_background(value: i32) -> JoinHandle<bool> {
    thread::spawn(move || value > 0)
}

/// Whether the thread of `handle` gave `true`, as a crate that joins a
/// handle it is given does.
pub fn finished(handle: JoinHandle<bool>) -> bool {
    handle.join().unwrap_or(false)
}

/// A page a thread reads.
#[derive(Debug)]
pub struct Page {
    number: u32,
    text: String,
}

impl Page {
    /// The page's number.
    pub fn number(&self) -> u32 {
        self.number
    }

    /// The page's text.
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// Reads page `number` on another thread.
pub fn read_in_background(number: u32) -> JoinHandle<Page> {
    thread::spawn(move || Page {
        number,
        text: format!("page {number}"),
    })
}

/// Reads page `number` on another thread, which finds none for page 0.
pub fn find_in_background(number: u32) -> JoinHandle<Option<Page>> {
    thread::spawn(move || {
        (number > 0).then(|| Page {
            number,
            text: format!("page {number}"),
        })
    })
}

/// The number of the page the thread of `handle` read.
pub fn number_of(handle: JoinHandle<Page>) -> u32 {
    handle.join().map_or(0, |page| page.number)
}

/// A value of any type the crate holds.
pub struct Held<T>(T);

impl<T> Held<T> {
    /// Holds `value`.
    pub fn new(value: T) -> Self {
        Self(value)
    }

    /// The value it holds.
    pub fn into_inner(self) -> T {
        self.0
    }
}
