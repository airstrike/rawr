// Copyright the gitoxide contributors. MIT OR Apache-2.0.
//! Default upwards discovery for Cargo's actual absolute package-root domain.

use std::{
    borrow::Cow,
    ffi::OsStr,
    path::{Component, Path, PathBuf},
};

use rawr_toml::document;

use super::{
    path::{self, cursor::Cursor},
    predicate, recognize, repository, trust, whitespace,
};
use crate::input::membership::{
    host,
    manifest::{io, load, session::Session, Error, Step},
};

/// Cargo package-list workspace loading rejects relative manifest parents before
/// constructing the selected Package. This proves only that lexical precondition.
pub(super) struct Root<'a>(&'a Path);

/// Each call is a distinct source occurrence. The caller must supply admitted
/// owned results through this same Session; there is no native/default provider.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Request<'a> {
    Initial,
    Candidate(&'a Path),
    /// gix::discover's independent call after path conversion, before opening.
    Opening,
}

#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub(super) enum Failure {
    #[error("current directory failed: {0}")]
    CurrentDir(io::Failure),
    #[error("directory normalization failed: {directory:?}")]
    InvalidInput { directory: PathBuf },
    #[error("directory is inaccessible: {path:?}")]
    InaccessibleDirectory { path: PathBuf },
    #[error("no repository found above {path:?}")]
    NoGitRepository { path: PathBuf },
    #[error("filesystem boundary {limit:?} prevents discovery above {path:?}")]
    NoGitRepositoryWithinFs { path: PathBuf, limit: PathBuf },
    #[error("trust lookup failed for {path:?}: {err}")]
    CheckTrust { path: PathBuf, err: io::Failure },
}

impl<'a> Root<'a> {
    pub(super) fn absolute<A: document::Admission>(
        path: &'a Path,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.string(path.as_os_str().len())?;
        session.items(1)?;
        if !path.is_absolute() {
            return session.fail(Error::State);
        }
        Ok(Self(path))
    }
}

/// Required(Reduced), no ceilings, cross_fs=false, dot_git_only=false and no
/// configured cwd. Source semantic failures are distinct from outer refusal.
pub(super) fn discover<'image, 'budget>(
    directory: Root<'_>,
    whitespace: &mut whitespace::Owner,
    current_dir: &mut impl FnMut(
        Request<'_>,
        &mut Session<'_, host::Host<'image, 'budget>>,
    ) -> Step<Result<PathBuf, io::Failure>>,
    session: &mut Session<'_, host::Host<'image, 'budget>>,
) -> Step<Result<(repository::Path, trust::Level), Failure>> {
    let directory = directory.0;
    session.items(1)?;
    let cwd = match current_dir(Request::Initial, session)? {
        Ok(cwd) => cwd,
        Err(error) => return Ok(Err(Failure::CurrentDir(error))),
    };
    session.check()?;
    let Some(dir) = path::normalize(Cow::Borrowed(directory), &cwd, session)? else {
        return invalid(directory, session);
    };
    let dir_metadata = match predicate::metadata(&dir, true, session)? {
        Ok(metadata) => metadata,
        Err(_) => return inaccessible(&dir, session),
    };
    if dir_metadata.kind != crate::input::membership::observed::Kind::Directory {
        return Ok(Err(Failure::InaccessibleDirectory {
            path: path::into_owned(dir, session)?,
        }));
    }
    // Absolute input makes source !directory.is_absolute() false. Normalizing,
    // appending .git and popping preserve the absolute cursor, so no relative
    // fallback, cwd-prefix comparison or shorten_path_with_cwd is reachable.
    let initial_device = dir_metadata.device;
    let mut cursor = Cursor::copy(&dir, session)?; // Cow::clone().into_owned()
    let mut height = 0usize;
    let mut cursor_metadata = Some(dir_metadata);
    loop {
        session.items(1)?;
        let Some(next) = height.checked_add(1) else {
            return session.fail(Error::Overflow);
        };
        height = next;
        let metadata = match cursor_metadata.take() {
            Some(metadata) => metadata,
            None => match predicate::metadata(cursor.path(), true, session)? {
                Ok(metadata) => metadata,
                Err(_) => return inaccessible(cursor.path(), session),
            },
        };
        if metadata.device != initial_device {
            let path = path::into_owned(dir, session)?;
            let limit = copy(cursor.path(), session)?;
            return Ok(Err(Failure::NoGitRepositoryWithinFs { path, limit }));
        }
        cursor_metadata = Some(metadata);

        let mut cursor_metadata_backup = None;
        session.items(cursor.path().as_os_str().len())?;
        let started_as_dot_git = cursor.path().file_name() == Some(OsStr::new(".git"));
        for append_dot_git in [true, false] {
            session.items(1)?;
            if append_dot_git && !started_as_dot_git {
                cursor.push(Path::new(".git"), session)?;
                cursor_metadata_backup = cursor_metadata.take();
            }
            // The source if-let scrutinee, including an owned semantic error,
            // retires before the subsequent cursor pop/restoration.
            {
                let recognized = match cursor_metadata.take() {
                    Some(metadata) => recognize::with_metadata(
                        cursor.path(),
                        metadata,
                        &cwd,
                        whitespace,
                        session,
                    )?,
                    None => recognize::git(cursor.path(), whitespace, session, |session| {
                        session.items(1)?;
                        let result = current_dir(Request::Candidate(cursor.path()), session)?;
                        session.check()?;
                        Ok(result)
                    })?,
                };
                if let Ok(kind) = recognized {
                    let trust = match trust::from_path(cursor.path(), session)? {
                        Ok(trust) => trust,
                        Err(err) => {
                            let path = copy(cursor.path(), session)?;
                            return Ok(Err(Failure::CheckTrust { path, err }));
                        }
                    };
                    // Both source trust levels satisfy Required(Reduced).
                    let Some(path) = repository::Path::from_dot_git_dir(
                        cursor.into_path(),
                        kind,
                        &cwd,
                        session,
                    )?
                    else {
                        return invalid(directory, session);
                    };
                    return Ok(Ok((path, trust)));
                }
            }
            if append_dot_git || started_as_dot_git {
                cursor.pop(session)?;
                if let Some(metadata) = cursor_metadata_backup.take() {
                    cursor_metadata = Some(metadata);
                }
            }
        }
        if !cursor.pop(session)? {
            session.items(cursor.path().as_os_str().len())?;
            if !matches!(cursor.path().components().next(), Some(Component::RootDir)) {
                return session.fail(Error::State);
            }
            return Ok(Err(Failure::NoGitRepository {
                path: path::into_owned(dir, session)?,
            }));
        }
    }
}

fn copy<A: document::Admission>(path: &Path, session: &mut Session<'_, A>) -> Step<PathBuf> {
    session.items(path.as_os_str().len())?;
    load::path::copy(path, session)
}

fn invalid<T, A: document::Admission>(
    directory: &Path,
    session: &mut Session<'_, A>,
) -> Step<Result<T, Failure>> {
    let directory = copy(directory, session)?;
    Ok(Err(Failure::InvalidInput { directory }))
}

fn inaccessible<T, A: document::Admission>(
    directory: &Path,
    session: &mut Session<'_, A>,
) -> Step<Result<T, Failure>> {
    let path = copy(directory, session)?;
    Ok(Err(Failure::InaccessibleDirectory { path }))
}

#[cfg(test)]
mod tests;
