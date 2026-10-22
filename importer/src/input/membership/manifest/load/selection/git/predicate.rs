// Copyright the Rust Project Developers. MIT OR Apache-2.0.
//! Source-local path conversion for Git metadata and recognition predicates.

use std::{alloc::Layout, os::unix::ffi::OsStrExt, path::Path};

use rawr_toml::document;

use crate::input::membership::{
    host,
    manifest::{io, session::Session, Error, Step},
    observed,
};

enum Check {
    Exists,
    Directory,
}

pub(super) fn exists(path: &Path, session: &mut Session<'_, host::Host<'_, '_>>) -> Step<bool> {
    query(path.as_os_str().as_bytes(), Check::Exists, session)
}

pub(super) fn directory(path: &Path, session: &mut Session<'_, host::Host<'_, '_>>) -> Step<bool> {
    query(path.as_os_str().as_bytes(), Check::Directory, session)
}

fn query(bytes: &[u8], check: Check, session: &mut Session<'_, host::Host<'_, '_>>) -> Step<bool> {
    convert(bytes, session, |session| {
        observe(bytes, check, session).map(Ok)
    })
    .map(|result| result.unwrap_or(false))
}

/// Preserve exact recorded native failure; scalar absence has no errno and is
/// terminal at the existing metadata accessor. Local NUL has its static error.
pub(super) fn metadata(
    path: &Path,
    follow: bool,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<Result<observed::Metadata, io::Failure>> {
    let bytes = path.as_os_str().as_bytes();
    convert(bytes, session, |session| {
        session
            .metadata(bytes, follow)
            .map(|value| value.map_err(io::Failure::Native))
    })
}

fn convert<T>(
    bytes: &[u8],
    session: &mut Session<'_, host::Host<'_, '_>>,
    observe: impl FnOnce(&mut Session<'_, host::Host<'_, '_>>) -> Step<Result<T, io::Failure>>,
) -> Step<Result<T, io::Failure>> {
    session.string(bytes.len())?;
    let Some(work) = bytes.len().checked_mul(2).and_then(|n| n.checked_add(1)) else {
        return session.fail(Error::Overflow);
    };
    session.items(work)?; // source copy plus C-string validation
                          // Pinned std small_c_string.rs uses stack storage only below 384 bytes.
                          // Both branches copy before NUL validation. The owner spans the query.
    if bytes.len() < 384 {
        let mut native = [0u8; 384];
        native[..bytes.len()].copy_from_slice(bytes);
        if std::ffi::CStr::from_bytes_with_nul(&native[..bytes.len() + 1]).is_err() {
            return Ok(Err(io::Failure::Nul));
        }
        observe(session)
    } else {
        let Some(capacity) = bytes.len().checked_add(1) else {
            return session.fail(Error::Overflow);
        };
        let Ok(layout) = Layout::array::<u8>(capacity) else {
            return session.fail(Error::Overflow);
        };
        session.items(1)?;
        session.admit(document::Request::Allocation {
            bytes: layout.size(),
            alignment: layout.align(),
        })?;
        let mut native = Vec::new();
        if let Err(error) = native.try_reserve_exact(capacity) {
            return session.fail(Error::Allocation(error));
        }
        native.extend_from_slice(bytes);
        let result = if bytes.contains(&0) {
            Ok(Err(io::Failure::Nul))
        } else {
            native.push(0);
            observe(session)
        };
        drop(native);
        result
    }
}

fn observe(
    bytes: &[u8],
    check: Check,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<bool> {
    match check {
        Check::Exists => session.exists(bytes),
        Check::Directory => session.is_dir(bytes),
    }
}
