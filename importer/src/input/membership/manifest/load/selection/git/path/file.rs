// Copyright the gitoxide contributors and Rust Project Developers.
// MIT OR Apache-2.0; see ../../LICENSE-* and PROVENANCE.md.
//! The three pinned discovery path-file consumers, after the closed read.
//! A caller-retained whitespace owner spans every call in its modeled process.

use std::{
    alloc::Layout,
    ffi::{OsStr, OsString},
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};

use rawr_toml::document;

use super::super::whitespace;
use crate::input::membership::manifest::{io, load, session::Session, Error, Step};
use crate::input::membership::{git_read, host, observed};

#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub(in super::super) enum Failure {
    #[error(transparent)]
    Io(io::Failure),
    #[error("expected gitdir: <path>, input bytes were {input:?}")]
    InvalidFormat { input: Vec<u8> },
}

pub(in super::super) fn from_gitdir_file(
    path: &Path,
    whitespace: &mut whitespace::Owner,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<Result<PathBuf, Failure>> {
    let bytes = match read(path, session)? {
        Ok(bytes) => bytes,
        Err(error) => return Ok(Err(Failure::Io(error))),
    };
    let mut gitdir = match parse(&bytes, whitespace, session)? {
        Ok(path) => path,
        Err(error) => return Ok(Err(error)),
    };
    session.items(1)?;
    session.items(path.as_os_str().len())?;
    if let Some(parent) = path.parent() {
        gitdir = join(parent, &gitdir, session)?;
    }
    // Source read buffer lives through parsing and the parent's join. The old
    // decoded path retires at assignment, then this buffer on function return.
    Ok(Ok(gitdir))
}

pub(in super::super) fn from_plain_file(
    path: &Path,
    whitespace: &mut whitespace::Owner,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<Option<Result<PathBuf, io::Failure>>> {
    Ok(plain_bytes(path, whitespace, session)?.map(|result| result.map(native)))
}

pub(in super::super) fn from_plain_file_relative_to_file(
    path: &Path,
    whitespace: &mut whitespace::Owner,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<Option<Result<PathBuf, io::Failure>>> {
    let Some(result) = plain_bytes(path, whitespace, session)? else {
        return Ok(None);
    };
    let bytes = match result {
        Ok(bytes) => bytes,
        Err(error) => return Ok(Some(Err(error))),
    };
    relative(path, native(bytes), session).map(Some)
}

fn read(
    path: &Path,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<Result<Vec<u8>, io::Failure>> {
    let bytes = path.as_os_str().as_bytes();
    session.string(bytes.len())?;
    session.items(bytes.len())?;
    if bytes.contains(&0) {
        // std's local NUL error is not an observed syscall errno. Preserve the
        // small_c_string copy/owner before returning its static InvalidInput.
        if bytes.len() < 384 {
            let mut native = [0u8; 384];
            session.items(bytes.len())?;
            native[..bytes.len()].copy_from_slice(bytes);
            if std::ffi::CStr::from_bytes_with_nul(&native[..bytes.len() + 1]).is_ok() {
                return session.fail(Error::State);
            }
        } else {
            let Some(length) = bytes.len().checked_add(1) else {
                return session.fail(Error::Overflow);
            };
            let mut native = allocate(length, session)?;
            session.items(bytes.len())?;
            native.extend_from_slice(bytes);
            if !native.contains(&0) {
                return session.fail(Error::State);
            }
            // CString::new returns its NulError holding this Vec, which the
            // source's Err(_) arm drops before the static NUL_ERR escapes.
            drop(native);
        }
        return Ok(Err(io::Failure::Nul));
    }
    match session.git_read(
        path.as_os_str().as_bytes(),
        observed::git_read::Role::DiscoveryPath,
    )? {
        git_read::Outcome::Bytes(bytes) => Ok(Ok(bytes)),
        git_read::Outcome::Oversized(error) => Ok(Err(error)),
        git_read::Outcome::OpenFailed(code)
        | git_read::Outcome::GateFailed(code)
        | git_read::Outcome::ReadFailed {
            code,
            directory: None,
        } => Ok(Err(io::Failure::Native(code))),
        _ => session.fail(Error::State),
    }
}

fn plain_bytes(
    path: &Path,
    whitespace: &mut whitespace::Owner,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<Option<Result<Vec<u8>, io::Failure>>> {
    let mut bytes = match read(path, session)? {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Ok(Some(Err(error))),
    };
    let length = whitespace.trim_end(&bytes, session)?.len();
    bytes.truncate(length);
    if bytes.is_empty() {
        // Keep the source 512-byte (or grown) buffer through error formatting
        // and both owned error allocations, then retire it here.
        let error = io::Failure::empty(path, session)?;
        return Ok(Some(Err(error)));
    }
    Ok(Some(Ok(bytes)))
}

fn parse<A: document::Admission>(
    input: &[u8],
    whitespace: &mut whitespace::Owner,
    session: &mut Session<'_, A>,
) -> Step<Result<PathBuf, Failure>> {
    session.items(8)?;
    let Some(path) = input.strip_prefix(b"gitdir: ") else {
        return invalid(input, session);
    };
    let path = whitespace.trim_end(path, session)?;
    if path.is_empty() {
        return invalid(input, session);
    }
    // Unix gix_path::try_from_bstr borrows arbitrary native bytes, then the
    // source Cow::into_owned clones exactly this trimmed path extent.
    load::path::copy(Path::new(OsStr::from_bytes(path)), session).map(Ok)
}

fn invalid<A: document::Admission>(
    input: &[u8],
    session: &mut Session<'_, A>,
) -> Step<Result<PathBuf, Failure>> {
    session.string(input.len())?;
    session.items(1)?;
    let mut bytes = allocate(input.len(), session)?;
    session.items(input.len())?;
    bytes.extend_from_slice(input);
    Ok(Err(Failure::InvalidFormat { input: bytes }))
}

fn relative<A: document::Admission>(
    file: &Path,
    plain: PathBuf,
    session: &mut Session<'_, A>,
) -> Step<Result<PathBuf, io::Failure>> {
    session.items(1)?;
    if plain.is_absolute() {
        return Ok(Ok(plain));
    }
    session.items(file.as_os_str().len())?;
    match file.parent() {
        Some(parent) => join(parent, &plain, session).map(Ok),
        None => io::Failure::parent(file, &plain, session).map(Err),
    }
}

/// Pinned Path::_join copies the parent before PathBuf::_push, even if the
/// suffix is absolute. Separate separator and suffix pushes retain its exact
/// requested Vec growth rather than allocating a final concatenation directly.
pub(in super::super) fn join<A: document::Admission>(
    root: &Path,
    suffix: &Path,
    session: &mut Session<'_, A>,
) -> Step<PathBuf> {
    session.items(root.as_os_str().len())?;
    let copy = load::path::copy(root, session)?;
    let mut bytes = copy.into_os_string().into_vec();
    let mut capacity = root.as_os_str().len();
    if suffix.is_absolute() {
        bytes.clear();
    } else if bytes.last().is_some_and(|&byte| byte != b'/') {
        append(&mut bytes, &mut capacity, b"/", session)?;
    }
    append(
        &mut bytes,
        &mut capacity,
        suffix.as_os_str().as_bytes(),
        session,
    )?;
    Ok(native(bytes))
}

/// The reached realpath(".") branch pushes cwd into a fresh PathBuf. A clone
/// would request a different capacity for short cwd spellings.
pub(in super::super) fn push_empty<A: document::Admission>(
    path: &Path,
    session: &mut Session<'_, A>,
) -> Step<PathBuf> {
    session.items(1)?;
    let mut bytes = Vec::new();
    let mut capacity = 0;
    append(
        &mut bytes,
        &mut capacity,
        path.as_os_str().as_bytes(),
        session,
    )?;
    Ok(native(bytes))
}

pub(in super::super) fn append<A: document::Admission>(
    bytes: &mut Vec<u8>,
    capacity: &mut usize,
    suffix: &[u8],
    session: &mut Session<'_, A>,
) -> Step<()> {
    let Some(required) = bytes.len().checked_add(suffix.len()) else {
        return session.fail(Error::Overflow);
    };
    session.string(required)?;
    if required > *capacity {
        let Some(doubled) = capacity.checked_mul(2) else {
            return session.fail(Error::Overflow);
        };
        let requested = required.max(doubled).max(8);
        let mut replacement = allocate(requested, session)?;
        session.items(bytes.len())?;
        replacement.extend_from_slice(bytes);
        *bytes = replacement;
        *capacity = requested;
    }
    session.items(suffix.len())?;
    bytes.extend_from_slice(suffix);
    Ok(())
}

fn allocate<A: document::Admission>(length: usize, session: &mut Session<'_, A>) -> Step<Vec<u8>> {
    let Ok(layout) = Layout::array::<u8>(length) else {
        return session.fail(Error::Overflow);
    };
    if length != 0 {
        session.admit(document::Request::Allocation {
            bytes: layout.size(),
            alignment: layout.align(),
        })?;
    }
    let mut bytes = Vec::new();
    if let Err(error) = bytes.try_reserve_exact(length) {
        return session.fail(Error::Allocation(error));
    }
    Ok(bytes)
}

fn native(bytes: Vec<u8>) -> PathBuf {
    PathBuf::from(OsString::from_vec(bytes))
}

#[cfg(test)]
mod tests;
