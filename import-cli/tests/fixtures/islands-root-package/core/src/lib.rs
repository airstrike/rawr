//! The root package's path dependency, over libc.

/// The status a program that succeeds exits with.
pub fn success() -> i32 {
    libc::EXIT_SUCCESS
}
