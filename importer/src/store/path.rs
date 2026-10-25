//! Prospective allocation admission for the pinned standard-library paths.

use std::path::{Path, PathBuf};

use crate::node::{Error, GraphBudget};

// nightly-2026-07-11's small_c_string uses a fixed stack buffer below this
// threshold. Its &[u8] CString constructor reserves exactly length + 1 above it.
const STACK_INPUT_BYTES: usize = 384;

// The supported host's realpath(path, NULL) allocates PATH_MAX bytes. This is
// an allocation bound, not an additional admitted pathname limit. The pinned
// std implementation copies at most PATH_MAX - 1 bytes into a Vec before freeing
// that libc buffer; OsString and PathBuf then take ownership without another copy.
// Apple Libc 71bbe350ab79eef58113991d817ccc6165061a64, realpath.c:107-108,
// 340-344; installed libc UUID 66EBD321-6899-3863-BA24-5CFC3076A0CB agrees
// (realpath's NULL branch allocates 0x400 bytes at 0x1803e952c). On Linux,
// glibc and musl return at most PATH_MAX (4096) bytes, so the host's PATH_MAX
// bounds both.
const CANONICAL_BUFFER_BYTES: usize = libc::PATH_MAX as usize;

/// Admit one std pathname-to-CStr conversion before its filesystem operation.
/// Descriptor-based operations and the raw directory reader do not use this seam.
pub(crate) fn input(path: &Path, budget: &GraphBudget) -> Result<(), Error> {
    let bytes = input_allocation(path)?;
    if bytes != 0 {
        budget.decode(bytes)?;
    }
    Ok(())
}

/// Allocation size for the pinned std pathname-to-CStr conversion.
pub(crate) fn input_allocation(path: &Path) -> Result<usize, Error> {
    let length = path.as_os_str().as_encoded_bytes().len();
    if length >= STACK_INPUT_BYTES {
        length.checked_add(1).ok_or(Error::Allocation)
    } else {
        Ok(0)
    }
}

/// Additional libc and Rust output storage used by std canonicalize.
pub(crate) const fn canonical_allocation() -> usize {
    std::mem::size_of::<PathBuf>() + CANONICAL_BUFFER_BYTES + (CANONICAL_BUFFER_BYTES - 1)
}

/// Admit the input, libc output, and copied Rust output before std canonicalize.
/// Native normalization and mount spelling can make the output longer than input.
pub(crate) fn canonical(path: &Path, budget: &GraphBudget) -> Result<(), Error> {
    input(path, budget)?;
    budget.decode(canonical_allocation())
}
