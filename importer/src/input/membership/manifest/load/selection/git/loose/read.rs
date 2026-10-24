// Copyright the gitoxide contributors. MIT OR Apache-2.0.
//! One reached loose-only find_inner, after its source-shaped file acquisition.

use std::{ffi::OsStr, os::unix::ffi::OsStrExt, path};

use super::{Name, Reference};
use crate::input::membership::manifest::{load, session::Session, Error, Step};
use crate::input::membership::{git_read, host, observed};

#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub(in super::super) enum Failure {
    #[error("reading loose reference {path:?} failed with OS error {code}")]
    ReadFileContents { code: i32, path: path::PathBuf },
    #[error("invalid loose reference {relative_path:?}: {source}")]
    ReferenceCreation {
        source: super::Failure,
        relative_path: path::PathBuf,
    },
}

/// The reached loose-only find_inner operation for one fixed HEAD candidate.
/// Store creation, candidate iteration and repository recognition remain outside.
pub(super) fn one(
    base: &path::Path,
    name: Name<'_>,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<Result<Option<Reference>, Failure>> {
    let contents = {
        let path = super::super::path::file::join(
            base,
            path::Path::new(OsStr::from_bytes(name.bytes)),
            session,
        )?;
        session.git_read(
            path.as_os_str().as_bytes(),
            observed::git_read::Role::LooseReference,
        )?
    };
    // ref_contents has closed File and retired its path before this match.
    let bytes = match contents {
        git_read::Outcome::Bytes(bytes) => bytes,
        git_read::Outcome::OpenFailed(code)
            if code == libc::ENOENT || code == libc::ENOTDIR =>
        {
            return Ok(Ok(None))
        }
        git_read::Outcome::ReadFailed {
            directory: Some(Ok(true)),
            ..
        } => return Ok(Ok(None)),
        git_read::Outcome::ReadFailed { code, .. } if code == libc::ENOTDIR => {
            return Ok(Ok(None))
        }
        git_read::Outcome::OpenFailed(code) | git_read::Outcome::ReadFailed { code, .. } => {
            let path = super::super::path::file::join(
                base,
                path::Path::new(OsStr::from_bytes(name.bytes)),
                session,
            )?;
            session.items(1)?;
            return Ok(Err(Failure::ReadFileContents { code, path }));
        }
        _ => return session.fail(Error::State),
    };
    match super::decode(name.candidate, &bytes, session)? {
        Ok(reference) => Ok(Ok(Some(reference))),
        Err(source) => {
            let relative_path =
                load::path::copy(path::Path::new(OsStr::from_bytes(name.bytes)), session)?;
            session.items(1)?;
            Ok(Err(Failure::ReferenceCreation {
                source,
                relative_path,
            }))
        }
    }
}
