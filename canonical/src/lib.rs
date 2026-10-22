//! Private, not-yet-activated canonicalization foundation.
//!
//! Input and initialized transfer storage belong to the caller. This Rust
//! boundary makes no owned path, C string, collection, or output allocation.
//! Before eventual integration the caller must admit their ownership and work
//! through its original cumulative account. Native transitive allocation,
//! concurrency and installed behavior remain qualification gates; see SAFETY.md.

#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

use std::ffi::CStr;

#[allow(unsafe_code)]
mod sys;

/// Darwin's native PATH_MAX transfer contract, including the terminating NUL.
/// This is not a new accepted-input length limit.
pub const BUFFER_BYTES: usize = 1024;
pub type Buffer = [u8; BUFFER_BYTES];

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum Framing {
    #[error("native canonicalization returned a different output pointer")]
    Pointer,
    #[error("native canonicalization output has no bounded terminator")]
    Termination,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Only this outcome may participate in native-error spelling fallback.
    #[error("native canonicalization failed: {0}")]
    Native(#[source] std::io::Error),
    #[error("invalid native canonicalization result: {0}")]
    Framing(#[from] Framing),
    #[error("canonicalization ABI is not implemented on this target")]
    Unsupported,
}

/// Canonicalize native bytes into caller-owned initialized storage.
///
/// A success borrows only `buffer`, never `path`. Error output is unspecified
/// and is never interpreted. The operation does not retry or use a fallback.
/// Neither relative inputs nor inputs exceeding PATH_MAX are rejected here:
/// the native operation decides their outcome. Relative-input qualification is
/// still required before production use.
///
/// ```compile_fail
/// let mut buffer = [0; rawr_canonical::BUFFER_BYTES];
/// let result = rawr_canonical::canonicalize(c"/", &mut buffer).unwrap();
/// buffer[0] = 0;
/// assert!(!result.to_bytes().is_empty());
/// ```
pub fn canonicalize<'b>(path: &CStr, buffer: &'b mut Buffer) -> Result<&'b CStr, Error> {
    sys::canonicalize(path, buffer)
}

#[cfg(test)]
mod tests;
