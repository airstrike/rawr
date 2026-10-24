use std::ffi::CStr;
use std::path::Path;

use rustix::fs::{self as stat, Mode, OFlags};
use sha2::{Digest as _, Sha256};

use super::super::{Budget, Result};
use super::{io, path, Error};

/// One successful open owns this descriptor, including every error exit. A
/// close error is not retried against a descriptor number that the OS could
/// already have reused.
struct Descriptor(std::os::fd::OwnedFd);

enum Extent {
    Initial(usize),
    Opened,
}

/// rustix keeps the native errno, Darwin's 107 included.
fn raw<T>(result: rustix::io::Result<T>) -> std::io::Result<T> {
    result.map_err(|error| std::io::Error::from_raw_os_error(error.raw_os_error()))
}

/// One freshness file's length; the `input_proof_bytes` target is only
/// recorded.
pub(super) fn length(length: u64, budget: &mut Budget<'_>) -> Result<usize> {
    let length = usize::try_from(length).map_err(|_| budget.overflow())?;
    budget.limited("input proof bytes", budget.limits.input_proof_bytes, length)?;
    Ok(length)
}

/// Stream one freshness file into SHA-256 through a fixed chunk. The content
/// extent stays charged to the original input account exactly as the buffer
/// it replaced, but no buffer of the file is reserved: an opened extent past
/// `expected` refuses as a read bound while a shorter one is drift.
pub(super) fn digest(path: &Path, expected: usize, budget: &mut Budget<'_>) -> Result<[u8; 32]> {
    budget.admit(1, expected)?;
    // An identity unchanged since it was hashed is not reopened:
    // the path's metadata names the identity an open would. The same path
    // checks are charged as the open, its retry and the handle inspection.
    if let Ok(metadata) = std::fs::metadata(path)
        && usize::try_from(metadata.len()).is_ok_and(|actual| actual == expected)
        && let Some(digest) = crate::file_digest::get(
            crate::file_digest::Identity::of(&metadata),
            crate::file_digest::Domain::Sha256,
        )
    {
        drop(path::terminated(path, budget)?);
        budget.paths(3)?;
        budget.physical("freshness file", expected, expected)?;
        length(metadata.len(), budget)?;
        return Ok(digest);
    }
    let source = open(path, budget)?;
    budget.paths(1)?;
    let opened =
        raw(stat::fstat(&source.0)).map_err(|error| io("inspect bounded input handle", error))?;
    let actual = usize::try_from(opened.st_size as u64).map_err(|_| budget.overflow())?;
    budget.physical("freshness file", expected, actual)?;
    length(opened.st_size as u64, budget)?;
    // The digest is a pure function of the file bytes: one computation per
    // physical identity serves every later freshness check in this process.
    let identity = crate::file_digest::Identity::of_stat(&opened);
    if actual == expected
        && let Some(digest) = crate::file_digest::get(identity, crate::file_digest::Domain::Sha256)
    {
        return Ok(digest);
    }
    let mut hasher = Sha256::new();
    let mut chunk = [0_u8; crate::input::DIGEST_CHUNK_BYTES];
    let mut remaining = expected;
    while remaining != 0 {
        budget.paths(1)?;
        let wanted = remaining.min(chunk.len());
        match raw(rustix::io::read(&source.0, &mut chunk[..wanted])) {
            Ok(0) => return Err(Error::Drift("input-changed-during-read").into()),
            Ok(count) => {
                hasher.update(&chunk[..count]);
                remaining -= count;
            }
            Err(error) if error.raw_os_error() == Some(libc::EINTR) => {}
            Err(error) => return Err(io("read bounded input", error)),
        }
    }
    probe(&source, budget)?;
    budget.hash(expected)?;
    let digest: [u8; 32] = hasher.finalize().into();
    if actual == expected
        && raw(stat::fstat(&source.0))
            .ok()
            .and_then(|after| crate::file_digest::Identity::of_stat(&after))
            == identity
    {
        crate::file_digest::insert(identity, crate::file_digest::Domain::Sha256, digest);
    }
    Ok(digest)
}

/// Structured input bounded by an earlier pathname metadata observation. A
/// shorter opened file remains valid; File entries separately require equality.
pub(in crate::input) fn bounded(
    path: &Path,
    maximum: usize,
    budget: &mut Budget<'_>,
) -> Result<Vec<u8>> {
    content(path, Extent::Initial(maximum), budget)
}

/// Dep-info's opened extent is measured against the Cargo JSON accounting
/// target, independently of its earlier pathname metadata. Admit that exact
/// buffer before allocation.
pub(in crate::input) fn dep_info(path: &Path, budget: &mut Budget<'_>) -> Result<Vec<u8>> {
    content(path, Extent::Opened, budget)
}

fn content(path: &Path, extent: Extent, budget: &mut Budget<'_>) -> Result<Vec<u8>> {
    // This is source content, not index/envelope decoding. Its real heap buffer
    // uses the original input account; hashing uses the separate graph account.
    let buffer = match extent {
        Extent::Initial(maximum) => Some(budget.buffer(maximum)?),
        Extent::Opened => None,
    };
    let source = open(path, budget)?;
    budget.paths(1)?;
    let opened =
        raw(stat::fstat(&source.0)).map_err(|error| io("inspect bounded input handle", error))?;
    let actual = match extent {
        Extent::Initial(maximum) => {
            let actual = usize::try_from(opened.st_size as u64).map_err(|_| budget.overflow())?;
            // The buffer was admitted at the metadata length; a read that grew
            // past it is a physical bound, not an accounting target.
            budget.physical("freshness file", maximum, actual)?;
            length(opened.st_size as u64, budget)?
        }
        Extent::Opened => {
            let actual = usize::try_from(opened.st_size as u64).map_err(|_| budget.overflow())?;
            // The Cargo JSON accounting target is measured and recorded on the
            // input account, never refused.
            budget.limited("rustc dep-info", budget.limits.json_bytes, actual)?;
            actual
        }
    };
    let mut bytes = match buffer {
        Some(bytes) => bytes,
        None => budget.buffer(actual)?,
    };
    bytes.resize(actual, 0);
    let mut offset = 0;
    while offset < actual {
        budget.paths(1)?;
        // Like read_exact, request the complete remaining admitted extent. The
        // existing input ceiling is below the supported native read range.
        match raw(rustix::io::read(&source.0, &mut bytes[offset..])) {
            Ok(0) => return Err(Error::Drift("input-changed-during-read").into()),
            Ok(count) => offset += count,
            Err(error) if error.raw_os_error() == Some(libc::EINTR) => {}
            Err(error) => return Err(io("read bounded input", error)),
        }
    }
    probe(&source, budget)?;
    Ok(bytes)
}

/// Open one input path, retrying EINTR; every attempt is a path check.
fn open(path: &Path, budget: &mut Budget<'_>) -> Result<Descriptor> {
    let native = path::terminated(path, budget)?;
    let native =
        CStr::from_bytes_with_nul(&native).map_err(|_| Error::Policy("input path contains NUL"))?;
    budget.paths(1)?;
    loop {
        budget.paths(1)?;
        match raw(stat::open(
            native,
            OFlags::RDONLY | OFlags::CLOEXEC,
            Mode::from_bits_truncate(0o666),
        )) {
            Ok(descriptor) => return Ok(Descriptor(descriptor)),
            Err(error) if error.raw_os_error() == Some(libc::EINTR) => {}
            Err(error) => return Err(io("open bounded input", error)),
        }
    }
}

/// Probe for growth after the admitted extent was read.
fn probe(source: &Descriptor, budget: &mut Budget<'_>) -> Result<()> {
    let mut extra = [0; 1];
    budget.paths(1)?;
    // Match the legacy single read after read_exact: a probe EINTR is an error.
    if raw(rustix::io::read(&source.0, &mut extra))
        .map_err(|error| io("probe bounded input growth", error))?
        != 0
    {
        return Err(Error::Drift("input-changed-during-read").into());
    }
    Ok(())
}
